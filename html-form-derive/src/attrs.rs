//! How the crate parses `#[form(...)]`, `#[field(...)]`, `#[option(...)]` and
//! `#[choice(...)]`.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Attribute, Error, Ident, Lit, LitStr, Path, Result, Token, token};

/// A string a person reads. You write it either as plain text or as `t("key")`,
/// which names an entry in whatever i18n backend the application uses.
///
/// `t("…")` uses parentheses, not the prefix form `t"…"`. A prefixed string
/// literal is a *lexer* error in Rust 2021 and later, so the tokens never reach
/// a proc macro.
pub struct TextAttr {
    pub content: String,
    /// Whether `content` is an i18n key and not the text itself.
    pub is_key: bool,
}

impl TextAttr {
    /// The `Text` const this becomes.
    ///
    /// This is a struct literal, not a `Text::literal(…)` call. A choice list
    /// is an `&[Choice]` rvalue that the compiler has to promote to `'static`.
    /// The compiler will not promote a call, not even a call to a `const fn`.
    pub fn tokens(&self) -> TokenStream {
        let content = &self.content;
        let is_key = self.is_key;
        quote!(::html_form::Text {
            content: ::std::borrow::Cow::Borrowed(#content),
            is_key: #is_key,
        })
    }

    /// True for the empty *literal*, which is how you write "render nothing
    /// here". The parser rejects an empty key, so one never reaches this.
    pub fn is_blank(&self) -> bool {
        !self.is_key && self.content.is_empty()
    }
}

impl Parse for TextAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        if !(input.peek(Ident) && input.peek2(token::Paren)) {
            let lit: LitStr = input.parse().map_err(|e| {
                Error::new(e.span(), "expected a string literal or `t(\"i18n-key\")`")
            })?;
            return Ok(TextAttr {
                content: lit.value(),
                is_key: false,
            });
        }

        let marker: Ident = input.parse()?;
        if marker != "t" {
            return Err(Error::new(
                marker.span(),
                format!("expected a string literal or `t(\"i18n-key\")`, not `{marker}(...)`"),
            ));
        }
        let inner;
        syn::parenthesized!(inner in input);
        let lit: LitStr = inner.parse()?;
        if !inner.is_empty() {
            return Err(inner.error("`t(...)` takes exactly one string literal: the i18n key"));
        }
        if lit.value().is_empty() {
            return Err(Error::new(lit.span(), "an i18n key cannot be empty"));
        }
        Ok(TextAttr {
            content: lit.value(),
            is_key: true,
        })
    }
}

/// Anything you can use as an attribute value, flattened to the string that
/// reaches the generated HTML. `min = 18` and `min = "18"` mean the same thing.
pub fn lit_to_string(lit: &Lit) -> Result<String> {
    Ok(match lit {
        Lit::Str(v) => v.value(),
        Lit::Int(v) => v.base10_digits().to_owned(),
        Lit::Float(v) => v.base10_digits().to_owned(),
        Lit::Bool(v) => v.value().to_string(),
        Lit::Char(v) => v.value().to_string(),
        other => {
            return Err(Error::new_spanned(
                other,
                "expected a string, number or boolean",
            ));
        }
    })
}

/// One entry of an `attr(...)` list: something the crate has no opinion about.
/// The crate writes it onto the element exactly as you gave it.
pub struct CustomAttr {
    pub name: String,
    /// `None` for a bare boolean attribute.
    pub value: Option<String>,
    span: Span,
}

impl Parse for CustomAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        // A dashed name is not an ident, so `"hx-post" = "/x"` is the general
        // form. A bare word works for a name that is already an ident.
        let (name, span) = if input.peek(LitStr) {
            let lit: LitStr = input.parse()?;
            (lit.value(), lit.span())
        } else {
            let ident = Ident::parse_any(input)?;
            (ident.to_string(), ident.span())
        };
        let value = if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            Some(lit_to_string(&input.parse()?)?)
        } else {
            None
        };
        Ok(CustomAttr { name, value, span })
    }
}

impl CustomAttr {
    /// The `Attr` const this entry becomes.
    pub fn tokens(&self) -> TokenStream {
        let name = &self.name;
        match &self.value {
            Some(value) => quote!(::html_form::Attr::new(#name, #value)),
            None => quote!(::html_form::Attr::flag(#name)),
        }
    }
}

/// A name the built-in renderer already writes, and the key that sets it. It is
/// `None` where the crate derives the attribute and nothing can name it.
type Reserved = (&'static str, Option<&'static str>);

/// What `<form>` renders on its own.
const FORM_RESERVED: &[Reserved] = &[
    ("id", Some("id")),
    ("name", Some("name")),
    ("action", Some("action")),
    ("method", Some("method")),
    ("enctype", Some("enctype")),
    ("class", Some("class")),
    ("novalidate", Some("novalidate")),
];

/// What a control renders on its own.
const FIELD_RESERVED: &[Reserved] = &[
    ("name", Some("name")),
    ("id", Some("id")),
    ("type", Some("type")),
    ("value", Some("default")),
    ("class", Some("class")),
    ("required", Some("required")),
    ("disabled", Some("disabled")),
    ("readonly", Some("readonly")),
    ("autofocus", Some("autofocus")),
    ("multiple", Some("multiple")),
    ("placeholder", Some("placeholder")),
    ("autocomplete", Some("autocomplete")),
    ("pattern", Some("pattern")),
    ("min", Some("min")),
    ("max", Some("max")),
    ("step", Some("step")),
    ("accept", Some("accept")),
    ("minlength", Some("minlength")),
    ("maxlength", Some("maxlength")),
    ("rows", Some("rows")),
    ("cols", Some("cols")),
    ("checked", None),
    ("selected", None),
    ("aria-invalid", None),
    ("aria-describedby", None),
];

/// Parse one `attr(...)` group and add to what earlier groups collected.
///
/// `owner` is the attribute that holds the group, so a rejected name can point
/// at the dedicated key that does the same job.
fn parse_custom_attrs(
    meta: &syn::meta::ParseNestedMeta<'_>,
    owner: &str,
    reserved: &[Reserved],
    out: &mut Vec<CustomAttr>,
) -> Result<()> {
    let content;
    syn::parenthesized!(content in meta.input);
    for entry in content.parse_terminated(CustomAttr::parse, Token![,])? {
        check_attr_name(&entry, owner, reserved)?;
        if out.iter().any(|a| a.name == entry.name) {
            return Err(Error::new(
                entry.span,
                format!("attribute `{}` is given twice", entry.name),
            ));
        }
        out.push(entry);
    }
    Ok(())
}

/// Reject a name that markup could not carry, and a name the crate already
/// writes itself.
fn check_attr_name(attr: &CustomAttr, owner: &str, reserved: &[Reserved]) -> Result<()> {
    if attr.name.is_empty() {
        return Err(Error::new(attr.span, "an attribute name cannot be empty"));
    }
    // The same set `html_form::is_attr_name` holds a runtime name to. A name
    // that compiles has to be one the renderer will write: anything this let
    // through and that did not would compile clean and then be dropped from the
    // markup, with nothing to say why.
    if let Some(bad) = attr
        .name
        .chars()
        .find(|ch| ch.is_whitespace() || ch.is_control() || "\"'></=&".contains(*ch))
    {
        return Err(Error::new(
            attr.span,
            format!("{bad:?} cannot appear in an attribute name"),
        ));
    }

    let lowered = attr.name.to_ascii_lowercase();
    let Some((_, key)) = reserved.iter().find(|(name, _)| *name == lowered) else {
        return Ok(());
    };
    Err(Error::new(
        attr.span,
        match key {
            Some(key) => format!(
                "`{}` is rendered by the crate itself; set it with `#[{owner}({key} = ...)]`",
                attr.name
            ),
            None => format!(
                "`{}` is rendered by the crate itself, from the state of the field",
                attr.name
            ),
        },
    ))
}

/// `#[form(status = ...)]`: the status the derived `IntoResponse` answers with.
///
/// Both shapes are settled before anything runs. A `StatusCode` is checked by
/// its own type, and a number by the `const fn` that turns it into one.
pub enum StatusAttr {
    /// `status = 422`
    Code(syn::LitInt),
    /// `status = StatusCode::UNPROCESSABLE_ENTITY`, or anything else that is a
    /// `StatusCode` a `const` can hold.
    Value(syn::Expr),
}

impl StatusAttr {
    /// The `StatusCode` const this becomes.
    pub fn tokens(&self) -> TokenStream {
        match self {
            StatusAttr::Code(lit) => quote!(::html_form::axum::__private::status(#lit)),
            StatusAttr::Value(expr) => quote!(#expr),
        }
    }
}

/// `#[form(...)]` on the struct.
#[derive(Default)]
pub struct FormAttrs {
    pub id: Option<String>,
    pub name: Option<String>,
    pub action: Option<String>,
    pub method: Option<(String, proc_macro2::Span)>,
    pub enctype: Option<(String, proc_macro2::Span)>,
    pub class: Option<String>,
    pub submit: Option<TextAttr>,
    pub novalidate: bool,
    pub validate: Option<Path>,
    /// `context = Type`: what this form's own functions receive. `None` means
    /// `()`.
    pub context: Option<syn::Type>,
    /// `renderer = ...`: the type, function or closure that answers a failed
    /// submission, and what makes this struct an extractor and a response of
    /// its own.
    ///
    /// It is an expression, because that is the one position a function and a
    /// marker type can share: a function has no type anybody can write down.
    /// The span is where the key was written, so the errors about the three
    /// keys below point at it.
    pub renderer: Option<(syn::Expr, Span)>,
    /// `status = ...`: what the derived `IntoResponse` answers with. `None` is
    /// `200 OK`.
    pub status: Option<(StatusAttr, Span)>,
    /// `from_request = false`: leave the extractor out.
    pub from_request: Option<(bool, Span)>,
    /// `into_response = false`: leave the response out.
    pub into_response: Option<(bool, Span)>,
    /// `attr(...)` entries, in the order you wrote them.
    pub custom: Vec<CustomAttr>,
}

impl FormAttrs {
    pub fn parse(attrs: &[Attribute]) -> Result<Self> {
        let mut out = FormAttrs::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("form")) {
            attr.parse_nested_meta(|meta| {
                let key = meta
                    .path
                    .get_ident()
                    .map(Ident::to_string)
                    .unwrap_or_default();
                match key.as_str() {
                    "id" => out.id = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "name" => out.name = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "action" => out.action = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "class" => out.class = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "submit" | "submit_label" => out.submit = Some(meta.value()?.parse()?),
                    "method" => {
                        let lit: LitStr = meta.value()?.parse()?;
                        out.method = Some((lit.value(), lit.span()));
                    }
                    "enctype" => {
                        let lit: LitStr = meta.value()?.parse()?;
                        out.enctype = Some((lit.value(), lit.span()));
                    }
                    "novalidate" => out.novalidate = parse_flag(&meta)?,
                    "validate" => out.validate = Some(meta.value()?.parse()?),
                    "context" => {
                        out.context = Some(meta.value()?.parse().map_err(|e| {
                            Error::new(
                                e.span(),
                                "expected the type this form's own functions are handed, e.g. \
                                 `context = Session`",
                            )
                        })?);
                    }
                    "renderer" => {
                        let span = meta.path.span();
                        out.renderer = Some((
                            meta.value()?.parse().map_err(|e| {
                                Error::new(
                                    e.span(),
                                    "expected a `Renderer`, or a function that renders one, e.g. \
                                     `renderer = Page`",
                                )
                            })?,
                            span,
                        ));
                    }
                    "status" => {
                        let span = meta.path.span();
                        let value = meta.value()?;
                        let status = if value.peek(syn::LitInt) {
                            StatusAttr::Code(value.parse()?)
                        } else if value.peek(Lit) {
                            return Err(value.error(
                                "expected a status code, e.g. `422`, or an `http::StatusCode`",
                            ));
                        } else {
                            StatusAttr::Value(value.parse()?)
                        };
                        out.status = Some((status, span));
                    }
                    "from_request" => {
                        out.from_request = Some((parse_flag(&meta)?, meta.path.span()))
                    }
                    "into_response" => {
                        out.into_response = Some((parse_flag(&meta)?, meta.path.span()))
                    }
                    "attr" => parse_custom_attrs(&meta, "form", FORM_RESERVED, &mut out.custom)?,
                    other => {
                        return Err(meta.error(format!(
                            "unknown `form` attribute `{other}`; expected one of: id, name, \
                             action, method, enctype, class, submit, novalidate, validate, \
                             context, renderer, status, from_request, into_response, attr"
                        )));
                    }
                }
                Ok(())
            })?;
        }
        Ok(out)
    }
}

/// A `#[option("value", "Label")]` entry on a field.
pub struct OptionAttr {
    pub value: String,
    pub label: Option<TextAttr>,
    pub disabled: bool,
    pub group: Option<TextAttr>,
}

impl Parse for OptionAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let value: LitStr = input.parse()?;
        let mut out = OptionAttr {
            value: value.value(),
            label: None,
            disabled: false,
            group: None,
        };
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            // The label may come by position, as `#[option("de", "Germany")]`.
            if input.peek(LitStr) || (input.peek(Ident) && input.peek2(token::Paren)) {
                out.label = Some(input.parse()?);
                continue;
            }
            let key: Ident = input.parse()?;
            match key.to_string().as_str() {
                "label" => {
                    input.parse::<Token![=]>()?;
                    out.label = Some(input.parse()?);
                }
                "group" => {
                    input.parse::<Token![=]>()?;
                    out.group = Some(input.parse()?);
                }
                "disabled" => out.disabled = true,
                other => {
                    return Err(Error::new(
                        key.span(),
                        format!(
                            "unknown `option` key `{other}`; expected `label`, `group` or \
                             `disabled`"
                        ),
                    ));
                }
            }
        }
        Ok(out)
    }
}

/// What a control is, and what it accepts. These are the keys that
/// `#[field(...)]` and `#[value(...)]` share.
///
/// They are one struct because they are one question: which control this is,
/// and what it constrains. A field asks that about itself. A type asks it about
/// every field it will ever be. Both pass the answer to the same
/// `__private::control`, where an attribute finds its place or fails to.
#[derive(Default)]
pub struct Constraints {
    /// `type = "email"`, with its span, so the compiler reports an unknown type
    /// where you wrote it.
    pub kind: Option<(String, Span)>,
    pub multiple: Option<bool>,
    pub pattern: Option<String>,
    pub minlength: Option<usize>,
    pub maxlength: Option<usize>,
    pub min: Option<String>,
    pub max: Option<String>,
    pub step: Option<String>,
    pub accept: Option<String>,
    pub rows: Option<u32>,
    pub cols: Option<u32>,
    pub choices: Option<Path>,
}

impl Constraints {
    /// The keys, named once, so the "unknown key" message of each attribute
    /// always matches what that attribute accepts.
    pub const KEYS: &'static str = "type, multiple, pattern, minlength, maxlength, min, max, step, accept, rows, cols, \
         choices";

    /// Take `key` if it is one of these, and say whether it was. The caller
    /// handles the keys that belong to it.
    pub fn parse(&mut self, key: &str, meta: &syn::meta::ParseNestedMeta<'_>) -> Result<bool> {
        match key {
            "type" | "kind" => {
                let lit: LitStr = meta.value()?.parse()?;
                self.kind = Some((lit.value(), lit.span()));
            }
            "multiple" => self.multiple = Some(parse_flag(meta)?),
            "pattern" => self.pattern = Some(meta.value()?.parse::<LitStr>()?.value()),
            "minlength" => {
                self.minlength = Some(meta.value()?.parse::<syn::LitInt>()?.base10_parse()?)
            }
            "maxlength" => {
                self.maxlength = Some(meta.value()?.parse::<syn::LitInt>()?.base10_parse()?)
            }
            "min" => self.min = Some(lit_to_string(&meta.value()?.parse()?)?),
            "max" => self.max = Some(lit_to_string(&meta.value()?.parse()?)?),
            "step" => self.step = Some(lit_to_string(&meta.value()?.parse()?)?),
            "accept" => self.accept = Some(meta.value()?.parse::<LitStr>()?.value()),
            "rows" => self.rows = Some(meta.value()?.parse::<syn::LitInt>()?.base10_parse()?),
            "cols" => self.cols = Some(meta.value()?.parse::<syn::LitInt>()?.base10_parse()?),
            "choices" => self.choices = Some(meta.value()?.parse()?),
            _ => return Ok(false),
        }
        Ok(true)
    }
}

/// Where a field's default comes from. The three spellings of `#[field(default
/// …)]`, which are alternatives, so one field of this type holds the answer.
pub enum DefaultAttr {
    /// `default = "…"`: the value itself, written into the spec.
    Literal(String),
    /// `default` with nothing after it: the field type's own
    /// `Default::default()`. A `const` cannot call that, so it too becomes glue
    /// the spec holds.
    Std,
    /// `default = path`: a function the crate calls once per render, not a
    /// value written into the spec.
    Fn(Path),
}

/// `#[field(...)]` plus any `#[option(...)]` entries on one struct field.
#[derive(Default)]
pub struct FieldAttrs {
    pub name: Option<String>,
    pub label: Option<TextAttr>,
    pub required: Option<bool>,
    /// `default`, in whichever of its three spellings you wrote. One field,
    /// because the three are alternatives: naming two of them is an error, and
    /// a type that says so needs no check to enforce it.
    pub default: Option<DefaultAttr>,
    /// `reset`: show the default on every render, and never what came in.
    ///
    /// Unwritten, so that the spec can fall back on the rule for a field the
    /// form owns: a hidden control whose default the form produces resets
    /// without being told to. `reset = false` turns that off.
    pub reset: Option<bool>,
    pub placeholder: Option<TextAttr>,
    pub help: Option<TextAttr>,
    pub autocomplete: Option<String>,
    pub id: Option<String>,
    pub class: Option<String>,
    pub disabled: bool,
    pub readonly: bool,
    pub autofocus: bool,
    pub validate: Option<Path>,
    /// `from_str`: convert this field with the type's own `FromStr` and
    /// `Display`, because it has no `FormValue` impl.
    pub from_str: bool,
    pub flatten: bool,
    pub prefix: Option<String>,
    pub legend: Option<TextAttr>,
    pub skip: bool,
    pub options: Vec<OptionAttr>,
    /// Which control the field is, and what it accepts.
    pub constraints: Constraints,
    /// `attr(...)` entries, in the order you wrote them.
    pub custom: Vec<CustomAttr>,
}

impl FieldAttrs {
    pub fn parse(attrs: &[Attribute]) -> Result<Self> {
        let mut out = FieldAttrs::default();

        for attr in attrs {
            if attr.path().is_ident("option") {
                out.options.push(attr.parse_args()?);
                continue;
            }
            if attr.path().is_ident("form") {
                return Err(Error::new_spanned(
                    attr,
                    "use `#[field(...)]` on fields; `#[form(...)]` describes the struct",
                ));
            }
            if !attr.path().is_ident("field") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                // `type` is a keyword. `parse_nested_meta` accepts it in a
                // path, and `get_ident` still reports it by name.
                let key = meta
                    .path
                    .get_ident()
                    .map(Ident::to_string)
                    .unwrap_or_default();
                if out.constraints.parse(&key, &meta)? {
                    return Ok(());
                }
                match key.as_str() {
                    "name" => out.name = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "label" => out.label = Some(meta.value()?.parse()?),
                    "required" => out.required = Some(parse_flag(&meta)?),
                    "optional" => out.required = Some(!parse_flag(&meta)?),
                    // Bare, the field type's own `Default::default()`. With a
                    // value, a literal is the value itself, and anything else
                    // names a function that produces one at render time.
                    "default" => {
                        if out.default.is_some() {
                            return Err(meta.error(
                                "`default` names one source: a literal, a function that produces \
                                 one, or nothing at all for the field type's own `Default`",
                            ));
                        }
                        out.default = Some(if !meta.input.peek(Token![=]) {
                            DefaultAttr::Std
                        } else {
                            let value = meta.value()?;
                            if value.peek(Lit) {
                                DefaultAttr::Literal(lit_to_string(&value.parse()?)?)
                            } else {
                                DefaultAttr::Fn(value.parse().map_err(|e| {
                                    Error::new(
                                        e.span(),
                                        "expected a literal default or the path of a function \
                                         returning one",
                                    )
                                })?)
                            }
                        });
                    }
                    "reset" => out.reset = Some(parse_flag(&meta)?),
                    "placeholder" => out.placeholder = Some(meta.value()?.parse()?),
                    "help" => out.help = Some(meta.value()?.parse()?),
                    "autocomplete" => {
                        out.autocomplete = Some(meta.value()?.parse::<LitStr>()?.value())
                    }
                    "id" => out.id = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "class" => out.class = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "disabled" => out.disabled = parse_flag(&meta)?,
                    "readonly" => out.readonly = parse_flag(&meta)?,
                    "autofocus" => out.autofocus = parse_flag(&meta)?,
                    "validate" => out.validate = Some(meta.value()?.parse()?),
                    "from_str" => out.from_str = parse_flag(&meta)?,
                    "flatten" => out.flatten = parse_flag(&meta)?,
                    "prefix" => out.prefix = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "legend" => out.legend = Some(meta.value()?.parse()?),
                    "skip" => out.skip = parse_flag(&meta)?,
                    "attr" => parse_custom_attrs(&meta, "field", FIELD_RESERVED, &mut out.custom)?,
                    other => {
                        return Err(meta.error(format!(
                            "unknown `field` attribute `{other}`; expected one of: name, label, \
                             required, optional, default, reset, placeholder, help, autocomplete, \
                             id, class, disabled, readonly, autofocus, validate, from_str, \
                             flatten, prefix, legend, skip, attr, {}",
                            Constraints::KEYS
                        )));
                    }
                }
                Ok(())
            })?;
        }

        Ok(out)
    }
}

/// `#[value(...)]` on a `#[derive(FormValue)]` type.
///
/// The part of `#[field(...)]` that describes a *value*, not its place in a
/// form. It says which control the value is, what it constrains, what it
/// defaults to, and the check it makes of itself. A label or a placeholder
/// belongs to the field, not to the type. The same type is a "Work email" on
/// one form and a "Recipient" on the next.
#[derive(Default)]
pub struct ValueAttrs {
    pub default: Option<String>,
    pub validate: Option<Path>,
    /// `from_str`: the type converts itself with its own `FromStr` and
    /// `Display`, not through the one value it wraps.
    pub from_str: bool,
    pub constraints: Constraints,
}

impl ValueAttrs {
    pub fn parse(attrs: &[Attribute]) -> Result<Self> {
        let mut out = ValueAttrs::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("value")) {
            attr.parse_nested_meta(|meta| {
                let key = meta
                    .path
                    .get_ident()
                    .map(Ident::to_string)
                    .unwrap_or_default();
                if out.constraints.parse(&key, &meta)? {
                    return Ok(());
                }
                match key.as_str() {
                    // A literal only. A type's default is an associated const,
                    // and a const cannot call a function. A default the crate
                    // has to produce per render belongs to the field, which is
                    // where the context that produces it reaches.
                    "default" => {
                        let value = meta.value()?;
                        if !value.peek(Lit) {
                            return Err(value.error(
                                "a type's default is a `const`, so it has to be a literal; a \
                                 default produced once per render is `#[field(default = path)]` \
                                 on the field that needs one",
                            ));
                        }
                        out.default = Some(lit_to_string(&value.parse()?)?);
                    }
                    "validate" => out.validate = Some(meta.value()?.parse()?),
                    "from_str" => out.from_str = parse_flag(&meta)?,
                    other => {
                        return Err(meta.error(format!(
                            "unknown `value` attribute `{other}`; expected one of: default, \
                             validate, from_str, {}",
                            Constraints::KEYS
                        )));
                    }
                }
                Ok(())
            })?;
        }
        Ok(out)
    }
}

/// A bare word, such as `required`, or an explicit `required = false`.
fn parse_flag(meta: &syn::meta::ParseNestedMeta<'_>) -> Result<bool> {
    if meta.input.peek(Token![=]) {
        Ok(meta.value()?.parse::<syn::LitBool>()?.value())
    } else {
        Ok(true)
    }
}

/// `#[choice(...)]` on a `FormChoice` variant.
#[derive(Default)]
pub struct ChoiceAttrs {
    pub value: Option<String>,
    pub label: Option<TextAttr>,
    pub disabled: bool,
    pub group: Option<TextAttr>,
}

impl ChoiceAttrs {
    pub fn parse(attrs: &[Attribute]) -> Result<Self> {
        let mut out = ChoiceAttrs::default();
        for attr in attrs.iter().filter(|a| a.path().is_ident("choice")) {
            attr.parse_nested_meta(|meta| {
                let key = meta
                    .path
                    .get_ident()
                    .map(Ident::to_string)
                    .unwrap_or_default();
                match key.as_str() {
                    "value" => out.value = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "label" => out.label = Some(meta.value()?.parse()?),
                    "group" => out.group = Some(meta.value()?.parse()?),
                    "disabled" => out.disabled = parse_flag(&meta)?,
                    other => {
                        return Err(meta.error(format!(
                            "unknown `choice` attribute `{other}`; expected one of: value, \
                             label, group, disabled"
                        )));
                    }
                }
                Ok(())
            })?;
        }
        Ok(out)
    }
}

// ─── Token helpers ────────────────────────────────────────────────────────────

/// `Option<&'static str>`. It is `Copy`, so a `const fn` can merge it.
pub fn opt_str(value: &Option<String>) -> TokenStream {
    match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    }
}

/// `Option<Text>`
pub fn opt_text(value: &Option<TextAttr>) -> TokenStream {
    match value {
        Some(text) => {
            let text = text.tokens();
            quote!(::core::option::Option::Some(#text))
        }
        None => quote!(::core::option::Option::None),
    }
}

/// `Option<usize>`
pub fn opt_usize(value: &Option<usize>) -> TokenStream {
    match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    }
}

/// `Option<u32>`
pub fn opt_u32(value: &Option<u32>) -> TokenStream {
    match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    }
}
