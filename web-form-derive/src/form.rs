//! `#[derive(WebForm)]`.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Error, Fields, Ident, Result, Type};

use crate::attrs::{CustomAttr, FieldAttrs, FormAttrs, opt_str, opt_text, opt_u32, opt_usize};

/// What a struct field maps to in a submission.
enum Shape<'a> {
    /// `#[field(skip)]` — not part of the form.
    Skip,
    /// `#[field(flatten)]` — a whole sub-form spliced in.
    Flatten,
    /// A `bool`, i.e. a checkbox: absent means `false`.
    Flag(&'a Type),
    /// `Option<T>` — blank and absent both mean `None`.
    Optional(&'a Type),
    /// `Vec<T>` — every value submitted under the name.
    Many(&'a Type),
    /// Anything else.
    Scalar(&'a Type),
}

impl<'a> Shape<'a> {
    /// The type whose [`FormValue`] impl drives this field, if any.
    fn value_type(&self) -> Option<&'a Type> {
        match self {
            Shape::Optional(ty) | Shape::Many(ty) | Shape::Scalar(ty) | Shape::Flag(ty) => Some(ty),
            Shape::Skip | Shape::Flatten => None,
        }
    }
}

pub fn derive(input: DeriveInput) -> Result<TokenStream> {
    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            &input.ident,
            "`WebForm` can only be derived for structs with named fields",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new_spanned(
            &data.fields,
            "`WebForm` can only be derived for structs with named fields",
        ));
    };

    let form = FormAttrs::parse(&input.attrs)?;
    let ident = &input.ident;

    // What this form's own functions are handed. Everything the form declares
    // — `default = ...`, `validate = ...` — is called with it, and a flattened
    // sub-form is parsed and rendered with it too.
    let context: Type = form
        .context
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(()));

    // A generic form is one whose `SPEC` names `<T as WebForm>::SPEC` — an
    // associated constant of a parameter, which the compiler resolves at
    // monomorphisation like any other. What a form *cannot* be generic over is
    // anything the spec would have to borrow from, since it is `'static`.
    let mut generics = input.generics.clone();
    let params: Vec<Ident> = generics.type_params().map(|p| p.ident.clone()).collect();

    let mut entries = Vec::new();
    let mut parse_steps = Vec::new();
    let mut construct = Vec::new();
    let mut fill_steps = Vec::new();
    // The `default = path` calls, and what says whether making them is worth a
    // walk at all — `false` unless something here, or in something flattened
    // here, produces a default at render time. A field of this form settles it
    // outright; a flattened one is a const of the sub-form's own.
    let mut default_steps = Vec::new();
    let mut generates_here = false;
    let mut generates: Vec<TokenStream> = Vec::new();
    // Bounds inferred from how each field is used, so that `struct WithCsrf<T>`
    // needs no bound written on it. Only types that mention a parameter get
    // one: adding `where Address: WebForm` for a concrete field would move the
    // error away from the field that caused it.
    let mut bounds: Vec<syn::WherePredicate> = Vec::new();

    for field in &fields.named {
        let field_ident = field.ident.as_ref().expect("named field");
        let attrs = FieldAttrs::parse(&field.attrs)?;
        let shape = shape_of(field_ident, &field.ty, &attrs)?;

        if matches!(shape, Shape::Skip) {
            construct.push(quote!(#field_ident: ::core::default::Default::default()));
            continue;
        }

        // The index this field takes in `FormSpec::entries`; the generated
        // parser looks its spec up by position.
        let index = entries.len();
        let binding = format_ident!("__field_{}", index);
        let ty = &field.ty;

        if let Some(generic) = shape.value_type().filter(|ty| mentions(ty, &params)) {
            bounds.push(syn::parse_quote!(#generic: ::web_form::FormValue));
        }

        if matches!(shape, Shape::Flatten) {
            if mentions(ty, &params) {
                bounds.push(syn::parse_quote!(#ty: ::web_form::WebForm));
            }
            // A sub-form is parsed and rendered with this form's context, or
            // with whatever that context hands down in its place. The bound is
            // only written when one side of it is a parameter: for two concrete
            // types it holds or it does not, and saying so here would move the
            // complaint away from the field that caused it.
            if mentions(ty, &params) || mentions(&context, &params) {
                bounds.push(syn::parse_quote!(
                    #context: ::web_form::Provides<<#ty as ::web_form::WebForm>::Context>
                ));
            }
            if let Some(custom) = attrs.custom.first() {
                return Err(Error::new(
                    field_ident.span(),
                    format!(
                        "`attr(...)` describes one control, and `#[field(flatten)]` splices in a \
                         whole form; move `{}` to a field of `{}`",
                        custom.name,
                        quote!(#ty),
                    ),
                ));
            }
            let prefix = attrs.prefix.clone().unwrap_or_default();
            let legend = opt_text(&attrs.legend);
            entries.push(quote! {
                ::web_form::Entry::Flatten(::web_form::Flattened {
                    prefix: #prefix,
                    legend: #legend,
                    // The sub-form's own `SPEC` constant, resolved while this
                    // one is const-evaluated.
                    spec: <#ty as ::web_form::WebForm>::SPEC,
                })
            });
            parse_steps.push(quote! {
                let #binding = ::web_form::ParseCtx::nested::<#ty>(
                    __ctx,
                    __spec.flatten_at(#index),
                );
            });
            construct.push(quote!(#field_ident: #binding?));
            fill_steps.push(quote! {
                ::web_form::WebForm::fill_in(
                    &self.#field_ident,
                    __values,
                    &::std::format!("{}{}", __prefix, #prefix),
                );
            });

            generates.push(quote!(<#ty as ::web_form::WebForm>::GENERATES_DEFAULTS));
            // One call, as on the parse side: which sub-forms are worth
            // walking, how the prefix is joined and what context the sub-form
            // is handed are the runtime's to decide, not this macro's.
            default_steps.push(quote! {
                ::web_form::__private::nested_defaults::<#ty, #context>(
                    __values,
                    __prefix,
                    #prefix,
                    __context,
                );
            });
            continue;
        }

        entries.push(field_spec(field_ident, &attrs, &shape)?);

        let spec_ref = quote!(__spec.field_at(#index));
        let read = match &shape {
            Shape::Flag(_) => quote!(::web_form::ParseCtx::flag(__ctx, #spec_ref)),
            Shape::Optional(inner) => {
                quote!(::web_form::ParseCtx::optional::<#inner>(__ctx, #spec_ref))
            }
            Shape::Many(inner) => quote!(::web_form::ParseCtx::many::<#inner>(__ctx, #spec_ref)),
            Shape::Scalar(inner) => quote!(::web_form::ParseCtx::field::<#inner>(__ctx, #spec_ref)),
            Shape::Skip | Shape::Flatten => unreachable!("handled above"),
        };

        parse_steps.push(quote!(let #binding = #read;));
        if let Some(validate) = &attrs.validate {
            // The validator sees the field's own Rust type, `Option<T>` and
            // `Vec<T>` included, so it can check emptiness or cardinality too.
            parse_steps.push(quote! {
                if let ::core::option::Option::Some(__value) = &#binding {
                    ::web_form::ParseCtx::check_custom(__ctx, #spec_ref, __value, #validate);
                }
            });
        }
        construct.push(quote!(#field_ident: #binding?));

        let name = attrs
            .name
            .clone()
            .unwrap_or_else(|| field_ident.to_string());

        if let Some(path) = &attrs.default_fn {
            generates_here = true;
            // Which of the two shapes the function has — with the context or
            // without — is settled by `DefaultSource`, since a macro cannot
            // see a signature.
            default_steps.push(quote! {
                ::web_form::__private::generate_default(
                    __values,
                    __prefix,
                    #name,
                    #path,
                    __context,
                );
            });
        }

        let push_value = |value: TokenStream| {
            quote! {
                __values.push(
                    ::std::format!("{}{}", __prefix, #name),
                    ::web_form::FormValue::to_form_value(#value).into_owned(),
                );
            }
        };
        fill_steps.push(match &shape {
            Shape::Flag(_) => quote! {
                // An unchecked box submits nothing at all.
                if self.#field_ident {
                    __values.push(::std::format!("{}{}", __prefix, #name), "true");
                }
            },
            Shape::Optional(_) => {
                let push = push_value(quote!(__value));
                quote! {
                    if let ::core::option::Option::Some(__value) = &self.#field_ident {
                        #push
                    }
                }
            }
            Shape::Many(_) => {
                let push = push_value(quote!(__value));
                quote! {
                    for __value in &self.#field_ident {
                        #push
                    }
                }
            }
            Shape::Scalar(_) => push_value(quote!(&self.#field_ident)),
            Shape::Skip | Shape::Flatten => unreachable!("handled above"),
        });
    }

    let form_id = opt_str(&form.id);
    let form_name = opt_str(&form.name);
    let form_action = opt_str(&form.action);
    let form_class = opt_str(&form.class);
    let form_submit = opt_text(&form.submit);
    let form_method = method_tokens(form.method.as_ref())?;
    let form_enctype = enctype_tokens(form.enctype.as_ref())?;
    let novalidate = form.novalidate;
    let form_attrs = form.custom.iter().map(CustomAttr::tokens);

    let form_validate = match &form.validate {
        Some(path) => quote! {
            ::web_form::ParseCtx::check_form(__ctx, &__form, #path);
        },
        None => quote!(),
    };

    // A form with nothing to generate leaves the trait's own empty body in
    // place, and says so in the const the renderer branches on.
    let generates_defaults = if generates_here {
        quote!(true)
    } else {
        quote!(false #(|| #generates)*)
    };
    let generate_defaults = if default_steps.is_empty() {
        TokenStream::new()
    } else {
        quote! {
            fn generate_defaults(
                __values: &mut ::web_form::Values,
                __prefix: &str,
                __context: &<Self as ::web_form::WebForm>::Context,
            ) {
                #(#default_steps)*
            }
        }
    };

    if !bounds.is_empty() {
        generics.make_where_clause().predicates.extend(bounds);
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::web_form::WebForm for #ident #ty_generics #where_clause {
            type Context = #context;

            const GENERATES_DEFAULTS: bool = #generates_defaults;

            // The whole description is one const-evaluated value: the reference
            // is to memory the compiler laid out, so nothing here runs, or
            // allocates, at render time.
            const SPEC: &'static ::web_form::FormSpec = &::web_form::FormSpec {
                id: #form_id,
                name: #form_name,
                action: #form_action,
                method: #form_method,
                enctype: #form_enctype,
                novalidate: #novalidate,
                class: #form_class,
                submit_label: #form_submit,
                attrs: &[#(#form_attrs),*],
                entries: &[#(#entries),*],
            };

            fn parse_in(
                __ctx: &mut ::web_form::ParseCtx<'_, #context>,
            ) -> ::core::option::Option<Self> {
                let __spec = <Self as ::web_form::WebForm>::SPEC;
                // Every field is read before anything is returned, so one pass
                // collects every error.
                #(#parse_steps)*
                let __form = Self { #(#construct),* };
                #form_validate
                ::core::option::Option::Some(__form)
            }

            fn fill_in(&self, __values: &mut ::web_form::Values, __prefix: &str) {
                #(#fill_steps)*
            }

            #generate_defaults
        }
    })
}

/// Work out how a field participates in the form, from its attributes and the
/// shape of its type.
fn shape_of<'a>(ident: &Ident, ty: &'a Type, attrs: &FieldAttrs) -> Result<Shape<'a>> {
    if attrs.skip {
        return Ok(Shape::Skip);
    }
    if attrs.flatten {
        return Ok(Shape::Flatten);
    }
    if attrs.prefix.is_some() {
        return Err(Error::new(
            ident.span(),
            "`prefix` only applies to `#[field(flatten)]`",
        ));
    }
    if let Some(inner) = generic_arg(ty, "Option") {
        return Ok(Shape::Optional(inner));
    }
    if let Some(inner) = generic_arg(ty, "Vec") {
        return Ok(Shape::Many(inner));
    }
    if is_ident(ty, "bool") {
        return Ok(Shape::Flag(ty));
    }
    Ok(Shape::Scalar(ty))
}

/// The `FieldSpec` const for one field.
fn field_spec(ident: &Ident, attrs: &FieldAttrs, shape: &Shape<'_>) -> Result<TokenStream> {
    let name = attrs.name.clone().unwrap_or_else(|| ident.to_string());
    let inner = shape.value_type();

    let required = attrs.required.unwrap_or(matches!(shape, Shape::Scalar(_)));
    let label = label_tokens(attrs, ident);
    let default = opt_str(&attrs.default);
    let placeholder = opt_text(&attrs.placeholder);
    let help = opt_text(&attrs.help);
    let autocomplete = opt_str(&attrs.autocomplete);
    let id = opt_str(&attrs.id);
    let class = opt_str(&attrs.class);
    let (disabled, readonly, autofocus) = (attrs.disabled, attrs.readonly, attrs.autofocus);
    let control = control_tokens(attrs, inner, shape)?;
    let custom = attrs.custom.iter().map(CustomAttr::tokens);

    Ok(quote! {
        ::web_form::Entry::Field(::web_form::FieldSpec {
            name: #name,
            label: #label,
            control: #control,
            required: #required,
            default: #default,
            placeholder: #placeholder,
            help: #help,
            autocomplete: #autocomplete,
            id: #id,
            class: #class,
            disabled: #disabled,
            readonly: #readonly,
            autofocus: #autofocus,
            attrs: &[#(#custom),*],
        })
    })
}

/// The field's [`Control`], assembled at const-eval time.
///
/// The macro cannot see what `<T as FormValue>::CONTROL` is, so it hands the
/// three ingredients — the type's control, the one `type = "..."` names and the
/// declared options — to `__private::control` along with every attribute that
/// belongs *inside* a control. That function is where an attribute finds its
/// place, or fails to.
fn control_tokens(
    attrs: &FieldAttrs,
    inner: Option<&Type>,
    shape: &Shape<'_>,
) -> Result<TokenStream> {
    let implied = match inner {
        Some(ty) => quote!(<#ty as ::web_form::FormValue>::CONTROL),
        None => quote!(::web_form::Control::TEXT),
    };

    let explicit = match &attrs.kind {
        Some((name, span)) => {
            let skeleton = control_skeleton(name, *span)?;
            quote!(::core::option::Option::Some(#skeleton))
        }
        None => quote!(::core::option::Option::None),
    };

    let choices = choices_tokens(attrs)?;

    // A `Vec<T>` field submits repeatedly by nature; whether that renders as a
    // `multiple` attribute depends on the control, which only `__private`
    // knows.
    let multiple = attrs.multiple.unwrap_or(matches!(shape, Shape::Many(_)));

    let pattern = opt_str(&attrs.pattern);
    let minlength = opt_usize(&attrs.minlength);
    let maxlength = opt_usize(&attrs.maxlength);
    let min = opt_str(&attrs.min);
    let max = opt_str(&attrs.max);
    let step = opt_str(&attrs.step);
    let accept = opt_str(&attrs.accept);
    let rows = opt_u32(&attrs.rows);
    let cols = opt_u32(&attrs.cols);

    Ok(quote! {
        ::web_form::__private::control(
            #implied,
            #explicit,
            #choices,
            ::web_form::__private::Overrides {
                pattern: #pattern,
                minlength: #minlength,
                maxlength: #maxlength,
                min: #min,
                max: #max,
                step: #step,
                accept: #accept,
                rows: #rows,
                cols: #cols,
                multiple: #multiple,
            },
        )
    })
}

/// An explicit label, or one derived from the field name.
///
/// `label = ""` means "render no label", which is what a hidden field wants.
fn label_tokens(attrs: &FieldAttrs, ident: &Ident) -> TokenStream {
    match &attrs.label {
        Some(label) if label.is_blank() => quote!(::core::option::Option::None),
        Some(label) => {
            let label = label.tokens();
            quote!(::core::option::Option::Some(#label))
        }
        None => {
            if matches!(attrs.kind.as_ref().map(|(k, _)| k.as_str()), Some("hidden")) {
                return quote!(::core::option::Option::None);
            }
            // A label nobody wrote is derived from the field name, so it is
            // literal text — there is no key to guess.
            let text = humanize(&ident.to_string());
            quote!(::core::option::Option::Some(::web_form::Text::literal(#text)))
        }
    }
}

/// `first_name` → `First name`.
fn humanize(ident: &str) -> String {
    let spaced = ident.trim_matches('_').replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => spaced,
    }
}

/// The empty control that `type = "..."` names, with no attribute placed in it
/// yet.
fn control_skeleton(name: &str, span: Span) -> Result<TokenStream> {
    let text = |format: &str| {
        let format = Ident::new(format, span);
        quote!(::web_form::Control::Text(::web_form::TextControl {
            format: ::web_form::TextFormat::#format,
            ..::web_form::TextControl::DEFAULT
        }))
    };
    let number = |format: &str| {
        let format = Ident::new(format, span);
        quote!(::web_form::Control::Number(::web_form::NumberControl {
            format: ::web_form::NumberFormat::#format,
            ..::web_form::NumberControl::DEFAULT
        }))
    };
    let temporal = |format: &str| {
        let format = Ident::new(format, span);
        quote!(::web_form::Control::Temporal(::web_form::TemporalControl {
            format: ::web_form::TemporalFormat::#format,
            ..::web_form::TemporalControl::DEFAULT
        }))
    };
    let choose = |style: &str| {
        let style = Ident::new(style, span);
        quote!(::web_form::Control::Choose(::web_form::ChooseControl {
            style: ::web_form::ChoiceStyle::#style,
            ..::web_form::ChooseControl::DEFAULT
        }))
    };

    Ok(match name {
        "text" => text("Text"),
        "password" => text("Password"),
        "tel" => text("Tel"),
        "search" => text("Search"),
        "url" => text("Url"),
        "email" => quote!(::web_form::Control::Text(::web_form::TextControl {
            format: ::web_form::TextFormat::Email { multiple: false },
            ..::web_form::TextControl::DEFAULT
        })),
        "number" => number("Number"),
        "range" => number("Range"),
        "date" => temporal("Date"),
        "time" => temporal("Time"),
        "datetime-local" | "datetime_local" => temporal("DatetimeLocal"),
        "month" => temporal("Month"),
        "week" => temporal("Week"),
        "select" => choose("Select"),
        "radio" => choose("Radio"),
        "textarea" => quote!(::web_form::Control::Textarea(
            ::web_form::TextareaControl::DEFAULT
        )),
        "file" => quote!(::web_form::Control::File(::web_form::FileControl::DEFAULT)),
        "checkbox" => quote!(::web_form::Control::Checkbox),
        "color" => quote!(::web_form::Control::Color),
        "hidden" => quote!(::web_form::Control::Hidden),
        other => {
            return Err(Error::new(
                span,
                format!(
                    "unknown field type `{other}`; expected one of: {}",
                    KIND_NAMES.join(", ")
                ),
            ));
        }
    })
}

const KIND_NAMES: &[&str] = &[
    "text",
    "password",
    "email",
    "url",
    "tel",
    "search",
    "number",
    "range",
    "checkbox",
    "radio",
    "date",
    "time",
    "datetime-local",
    "month",
    "week",
    "color",
    "file",
    "hidden",
    "textarea",
    "select",
];

/// `Option<&'static [Choice]>` from `#[option(...)]` entries or `choices = ...`.
fn choices_tokens(attrs: &FieldAttrs) -> Result<TokenStream> {
    if !attrs.options.is_empty() {
        if attrs.choices.is_some() {
            return Err(Error::new(
                Span::call_site(),
                "use either `#[option(...)]` entries or `choices = ...`, not both",
            ));
        }
        let items = attrs.options.iter().map(|option| {
            let value = &option.value;
            // An option with no label of its own is labelled by its value,
            // which is text, never a key.
            let label = match &option.label {
                Some(label) => label.tokens(),
                None => {
                    let text = &option.value;
                    quote!(::web_form::Text {
                        content: ::std::borrow::Cow::Borrowed(#text),
                        is_key: false,
                    })
                }
            };
            let disabled = option.disabled;
            let group = opt_text(&option.group);
            quote! {
                ::web_form::Choice {
                    value: ::std::borrow::Cow::Borrowed(#value),
                    label: #label,
                    disabled: #disabled,
                    group: #group,
                }
            }
        });
        return Ok(quote!(::core::option::Option::Some(&[#(#items),*])));
    }
    Ok(match &attrs.choices {
        Some(path) => quote!(::core::option::Option::Some(#path)),
        None => quote!(::core::option::Option::None),
    })
}

fn method_tokens(method: Option<&(String, Span)>) -> Result<TokenStream> {
    let Some((name, span)) = method else {
        return Ok(quote!(::core::option::Option::None));
    };
    let variant = match name.to_ascii_lowercase().as_str() {
        "get" => "Get",
        "post" => "Post",
        "dialog" => "Dialog",
        other => {
            return Err(Error::new(
                *span,
                format!("unknown form method `{other}`; expected `get`, `post` or `dialog`"),
            ));
        }
    };
    let variant = Ident::new(variant, *span);
    Ok(quote!(::core::option::Option::Some(
        ::web_form::FormMethod::#variant
    )))
}

fn enctype_tokens(enctype: Option<&(String, Span)>) -> Result<TokenStream> {
    let Some((name, span)) = enctype else {
        return Ok(quote!(::core::option::Option::None));
    };
    let variant = match name.to_ascii_lowercase().as_str() {
        "application/x-www-form-urlencoded" | "urlencoded" => "UrlEncoded",
        "multipart/form-data" | "multipart" => "MultipartFormData",
        "text/plain" | "text" => "TextPlain",
        other => {
            return Err(Error::new(
                *span,
                format!(
                    "unknown enctype `{other}`; expected `application/x-www-form-urlencoded`, \
                     `multipart/form-data` or `text/plain`"
                ),
            ));
        }
    };
    let variant = Ident::new(variant, *span);
    Ok(quote!(::core::option::Option::Some(
        ::web_form::FormEncType::#variant
    )))
}

// ─── Type inspection ──────────────────────────────────────────────────────────

/// The `T` of a `Wrapper<T>`, matched on the last path segment so that
/// `std::option::Option<T>` works too.
fn generic_arg<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else { return None };
    if path.qself.is_some() {
        return None;
    }
    let segment = path.path.segments.last()?;
    if segment.ident != wrapper {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    })
}

/// Whether a type mentions one of the struct's own type parameters, at any
/// depth — `T`, `Option<T>` and `Vec<Wrapper<T>>` all do.
fn mentions(ty: &Type, params: &[Ident]) -> bool {
    fn walk(tokens: TokenStream, params: &[Ident]) -> bool {
        tokens.into_iter().any(|tree| match tree {
            proc_macro2::TokenTree::Ident(ident) => params.contains(&ident),
            proc_macro2::TokenTree::Group(group) => walk(group.stream(), params),
            _ => false,
        })
    }
    !params.is_empty() && walk(quote!(#ty), params)
}

fn is_ident(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(path) if path.qself.is_none() && path.path.is_ident(name))
}
