//! `#[derive(Form)]`.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Error, Fields, Ident, Result, Type};

use crate::attrs::{CustomAttr, FieldAttrs, FormAttrs, opt_str, opt_text};
use crate::control::control_tokens;

/// What a struct field becomes in a submission.
enum Shape<'a> {
    /// `#[field(skip)]`: not part of the form.
    Skip,
    /// `#[field(flatten)]`: a whole sub-form put in here.
    Flatten,
    /// A `bool`, which is a checkbox. An absent value means `false`.
    Flag(&'a Type),
    /// `Option<T>`. A blank value and an absent one both mean `None`.
    Optional(&'a Type),
    /// `Vec<T>`: every value submitted under the name.
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
            "`Form` can only be derived for structs with named fields",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new_spanned(
            &data.fields,
            "`Form` can only be derived for structs with named fields",
        ));
    };

    let form = FormAttrs::parse(&input.attrs)?;
    let ident = &input.ident;

    // What this form's own functions receive. The crate calls everything the
    // form declares with it, such as `default = ...` and `validate = ...`. It
    // also parses and renders a flattened sub-form with it.
    let context: Type = form
        .context
        .clone()
        .unwrap_or_else(|| syn::parse_quote!(()));

    // A generic form is one whose `SPEC` names `<T as Form>::SPEC`. That is an
    // associated constant of a parameter, which the compiler resolves at
    // monomorphization like any other. A form *cannot* be generic over anything
    // the spec would have to borrow from, because the spec is `'static`.
    let mut generics = input.generics.clone();
    let params: Vec<Ident> = generics.type_params().map(|p| p.ident.clone()).collect();

    let mut entries = Vec::new();
    let mut parse_steps = Vec::new();
    let mut construct = Vec::new();
    let mut fill_steps = Vec::new();
    // The `default = path` calls, and the flag that says whether a walk is
    // worth making. The flag is `false` unless something here, or in something
    // flattened here, produces a default at render time. A field of this form
    // settles it outright. A flattened field brings a const of the sub-form's
    // own.
    let mut default_steps = Vec::new();
    let mut generates_here = false;
    let mut generates: Vec<TokenStream> = Vec::new();
    // Bounds taken from how each field is used, so `struct WithCsrf<T>` needs
    // no bound written on it. Only a type that names a parameter gets one.
    // Adding `where Address: Form` for a concrete field would move the error
    // away from the field that caused it.
    let mut bounds: Vec<syn::WherePredicate> = Vec::new();

    for field in &fields.named {
        let field_ident = field.ident.as_ref().expect("named field");
        let attrs = FieldAttrs::parse(&field.attrs)?;
        let shape = shape_of(field_ident, &field.ty, &attrs)?;

        if matches!(shape, Shape::Skip) {
            construct.push(quote!(#field_ident: ::core::default::Default::default()));
            continue;
        }

        // The index this field takes in `FormSpec::entries`. The generated
        // parser finds its spec by position.
        let index = entries.len();
        let binding = format_ident!("__field_{}", index);
        let ty = &field.ty;

        if let Some(generic) = shape.value_type().filter(|ty| mentions(ty, &params)) {
            // The conversion the field was told to use decides which trait the
            // field needs of its type.
            bounds.push(match attrs.from_str {
                true => {
                    syn::parse_quote!(#generic: ::core::str::FromStr + ::core::fmt::Display)
                }
                false => syn::parse_quote!(#generic: ::html_form::FormValue),
            });
        }

        if matches!(shape, Shape::Flatten) {
            if mentions(ty, &params) {
                bounds.push(syn::parse_quote!(#ty: ::html_form::Form));
            }
            // The crate parses and renders a sub-form with this form's context,
            // or with whatever that context gives in its place. The bound
            // appears only when one side of it is a parameter. For two concrete
            // types the bound holds or it does not, and stating it here would
            // move the error away from the field that caused it.
            if mentions(ty, &params) || mentions(&context, &params) {
                bounds.push(syn::parse_quote!(
                    #context: ::html_form::Provides<<#ty as ::html_form::Form>::Context>
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
                ::html_form::Entry::Flatten(::html_form::Flattened {
                    prefix: #prefix,
                    legend: #legend,
                    // The sub-form's own `SPEC` constant. The compiler resolves
                    // it while it const-evaluates this one.
                    spec: <#ty as ::html_form::Form>::SPEC,
                })
            });
            parse_steps.push(quote! {
                let #binding = ::html_form::ParseCtx::nested::<#ty>(
                    __ctx,
                    __spec.flatten_at(#index),
                );
            });
            construct.push(quote!(#field_ident: #binding?));
            fill_steps.push(quote! {
                ::html_form::Form::fill_in(
                    &self.#field_ident,
                    __values,
                    &::std::format!("{}{}", __prefix, #prefix),
                );
            });

            generates.push(quote!(<#ty as ::html_form::Form>::GENERATES_DEFAULTS));
            // One call, as on the parse side. The runtime decides which
            // sub-forms are worth a walk, how to join the prefix, and which
            // context the sub-form receives. This macro decides none of it.
            default_steps.push(quote! {
                ::html_form::__private::nested_defaults::<#ty, #context>(
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
        // The crate reads a `from_str` field as the adapter and unwraps it here
        // and now. Everything after this sees the type the caller wrote, such
        // as a `validate` function and the struct the parse builds.
        let value_ty = shape.value_type().map(|ty| value_type(&attrs, ty));
        let read = match (&shape, attrs.from_str) {
            (Shape::Flag(_), _) => quote!(::html_form::ParseCtx::flag(__ctx, #spec_ref)),
            (Shape::Optional(_), false) => {
                quote!(::html_form::ParseCtx::optional::<#value_ty>(__ctx, #spec_ref))
            }
            (Shape::Optional(_), true) => quote! {
                ::html_form::ParseCtx::optional::<#value_ty>(__ctx, #spec_ref)
                    .map(|__outer| __outer.map(|__wrapped| __wrapped.0))
            },
            (Shape::Many(_), false) => {
                quote!(::html_form::ParseCtx::many::<#value_ty>(__ctx, #spec_ref))
            }
            (Shape::Many(_), true) => quote! {
                ::html_form::ParseCtx::many::<#value_ty>(__ctx, #spec_ref).map(|__many| {
                    __many
                        .into_iter()
                        .map(|__wrapped| __wrapped.0)
                        .collect::<::std::vec::Vec<_>>()
                })
            },
            (Shape::Scalar(_), false) => {
                quote!(::html_form::ParseCtx::field::<#value_ty>(__ctx, #spec_ref))
            }
            (Shape::Scalar(_), true) => quote! {
                ::html_form::ParseCtx::field::<#value_ty>(__ctx, #spec_ref)
                    .map(|__wrapped| __wrapped.0)
            },
            (Shape::Skip | Shape::Flatten, _) => unreachable!("handled above"),
        };

        parse_steps.push(quote!(let #binding = #read;));
        if let Some(validate) = &attrs.validate {
            // The validator sees the field's own Rust type, `Option<T>` and
            // `Vec<T>` included. It can therefore also check whether the field
            // is empty and how many values it holds.
            parse_steps.push(quote! {
                if let ::core::option::Option::Some(__value) = &#binding {
                    ::html_form::ParseCtx::check_custom(__ctx, #spec_ref, __value, #validate);
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
            // `DefaultSource` settles which of the two shapes the function has,
            // with the context or without, because a macro cannot see a
            // signature.
            default_steps.push(quote! {
                ::html_form::__private::generate_default(
                    __values,
                    __prefix,
                    #name,
                    #path,
                    __context,
                );
            });
        }

        let push_value = |value: TokenStream| {
            // The other half of the conversion the field uses.
            let written = match attrs.from_str {
                true => quote!(::std::string::ToString::to_string(#value)),
                false => quote!(::html_form::FormValue::to_form_value(#value).into_owned()),
            };
            quote! {
                __values.push(::std::format!("{}{}", __prefix, #name), #written);
            }
        };
        fill_steps.push(match &shape {
            Shape::Flag(_) => quote! {
                // An unchecked box submits nothing.
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
            ::html_form::ParseCtx::check_form(__ctx, &__form, #path);
        },
        None => quote!(),
    };

    // A form with nothing to generate keeps the trait's own empty body, and
    // says so in the const the renderer branches on.
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
                __values: &mut ::html_form::Values,
                __prefix: &str,
                __context: &<Self as ::html_form::Form>::Context,
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
        impl #impl_generics ::html_form::Form for #ident #ty_generics #where_clause {
            type Context = #context;

            const GENERATES_DEFAULTS: bool = #generates_defaults;

            // The whole description is one const-evaluated value. The reference
            // points at memory the compiler laid out, so nothing here runs or
            // allocates at render time.
            const SPEC: &'static ::html_form::FormSpec = &::html_form::FormSpec {
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
                __ctx: &mut ::html_form::ParseCtx<'_, #context>,
            ) -> ::core::option::Option<Self> {
                let __spec = <Self as ::html_form::Form>::SPEC;
                // This reads every field before it returns anything, so one
                // pass collects every error.
                #(#parse_steps)*
                let __form = Self { #(#construct),* };
                #form_validate
                ::core::option::Option::Some(__form)
            }

            fn fill_in(&self, __values: &mut ::html_form::Values, __prefix: &str) {
                #(#fill_steps)*
            }

            #generate_defaults
        }
    })
}

/// Decide what part a field takes in the form, from its attributes and the
/// shape of its type.
fn shape_of<'a>(ident: &Ident, ty: &'a Type, attrs: &FieldAttrs) -> Result<Shape<'a>> {
    if attrs.skip {
        return Ok(Shape::Skip);
    }
    if attrs.flatten {
        if attrs.from_str {
            return Err(Error::new(
                ident.span(),
                "`from_str` converts one value, and `#[field(flatten)]` splices in a whole form; \
                 a sub-form is parsed field by field, each with the conversion it declared",
            ));
        }
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
        if attrs.from_str {
            return Err(Error::new(
                ident.span(),
                "`from_str` has nothing to add to a `bool`: a checkbox submits its own words, \
                 and an unchecked one submits nothing at all, which `FromStr` has no way to read",
            ));
        }
        return Ok(Shape::Flag(ty));
    }
    Ok(Shape::Scalar(ty))
}

/// The type whose `FormValue` impl drives a field. That is the type you wrote,
/// or, for `#[field(from_str)]`, the adapter that converts it with its own
/// `FromStr` and `Display`.
///
/// Everything else the field goes through reads the same either way. That is
/// the point of routing the conversion through a type, and not through a second
/// parse.
fn value_type(attrs: &FieldAttrs, ty: &Type) -> TokenStream {
    match attrs.from_str {
        true => quote!(::html_form::__private::Str<#ty>),
        false => quote!(#ty),
    }
}

/// The `FieldSpec` const for one field.
fn field_spec(ident: &Ident, attrs: &FieldAttrs, shape: &Shape<'_>) -> Result<TokenStream> {
    let name = attrs.name.clone().unwrap_or_else(|| ident.to_string());
    // Where the field's control and default come from: the type you wrote, or
    // the adapter that stands in for it.
    let inner = shape.value_type().map(|ty| value_type(attrs, ty));

    let required = attrs.required.unwrap_or(matches!(shape, Shape::Scalar(_)));
    let label = label_tokens(attrs, ident);
    let placeholder = opt_text(&attrs.placeholder);
    let help = opt_text(&attrs.help);
    let autocomplete = opt_str(&attrs.autocomplete);
    let id = opt_str(&attrs.id);
    let class = opt_str(&attrs.class);
    let (disabled, readonly, autofocus) = (attrs.disabled, attrs.readonly, attrs.autofocus);
    let custom = attrs.custom.iter().map(CustomAttr::tokens);

    // What the field says, then what its type says. The merge is a `const fn`
    // for the same reason the control's merge is. A type's own default is an
    // associated const, which the macro cannot read.
    let written = opt_str(&attrs.default);
    let default = match &inner {
        Some(ty) => quote! {
            ::html_form::__private::or_default(
                #written,
                <#ty as ::html_form::FormValue>::DEFAULT,
            )
        },
        None => written,
    };

    let implied = match &inner {
        Some(ty) => quote!(<#ty as ::html_form::FormValue>::CONTROL),
        None => quote!(::html_form::Control::TEXT),
    };
    // A `Vec<T>` field submits many values by nature. The control decides
    // whether that renders as a `multiple` attribute.
    let control = control_tokens(
        &attrs.constraints,
        &attrs.options,
        implied,
        matches!(shape, Shape::Many(_)),
    )?;

    Ok(quote! {
        ::html_form::Entry::Field(::html_form::FieldSpec {
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

/// A label you wrote, or one derived from the field name.
///
/// `label = ""` means "render no label", which is what a hidden field needs.
fn label_tokens(attrs: &FieldAttrs, ident: &Ident) -> TokenStream {
    match &attrs.label {
        Some(label) if label.is_blank() => quote!(::core::option::Option::None),
        Some(label) => {
            let label = label.tokens();
            quote!(::core::option::Option::Some(#label))
        }
        None => {
            if matches!(
                attrs.constraints.kind.as_ref().map(|(k, _)| k.as_str()),
                Some("hidden")
            ) {
                return quote!(::core::option::Option::None);
            }
            // A label nobody wrote comes from the field name, so it is literal
            // text. There is no key to guess.
            let text = humanize(&ident.to_string());
            quote!(::core::option::Option::Some(::html_form::Text::literal(#text)))
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
        ::html_form::FormMethod::#variant
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
        ::html_form::FormEncType::#variant
    )))
}

// ─── Type inspection ──────────────────────────────────────────────────────────

/// The `T` of a `Wrapper<T>`. This matches the last path segment, so
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

/// Whether a type names one of the struct's own type parameters, at any depth.
/// `T`, `Option<T>` and `Vec<Wrapper<T>>` all do.
pub(crate) fn mentions(ty: &Type, params: &[Ident]) -> bool {
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
