//! Parsing of `#[form(...)]`, `#[field(...)]`, `#[option(...)]` and
//! `#[choice(...)]`.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Error, Ident, Lit, LitStr, Path, Result, Token, token};

/// A string a person reads: written either as plain text or as `t("key")`,
/// which names an entry in whatever i18n backend the application uses.
///
/// `t("…")` is spelled with parentheses rather than the prefix form `t"…"`
/// because a prefixed string literal is a *lexer* error in Rust 2021 and later:
/// the tokens never reach a proc macro at all.
pub struct TextAttr {
    pub content: String,
    /// Whether `content` is an i18n key rather than the text itself.
    pub is_key: bool,
}

impl TextAttr {
    /// The `Text` const this becomes.
    ///
    /// Written as a struct literal rather than as `Text::literal(…)`: a choice
    /// list is an `&[Choice]` rvalue that has to be promoted to a `'static`,
    /// and a call — even to a `const fn` — is not something the compiler will
    /// promote.
    pub fn tokens(&self) -> TokenStream {
        let content = &self.content;
        let is_key = self.is_key;
        quote!(::web_form::Text {
            content: ::std::borrow::Cow::Borrowed(#content),
            is_key: #is_key,
        })
    }

    /// True for the empty *literal*, which is how "render nothing here" is
    /// written. An empty key is rejected at parse time, so it never gets here.
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

/// Anything usable as an attribute value, flattened to the string that ends up
/// in the generated HTML. `min = 18` and `min = "18"` mean the same thing.
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

/// One entry of an `attr(...)` list: something the crate itself has no opinion
/// about, written onto the element exactly as given.
pub struct CustomAttr {
    pub name: String,
    /// `None` for a bare boolean attribute.
    pub value: Option<String>,
    span: Span,
}

impl Parse for CustomAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        // A dashed name is not an ident, so `"hx-post" = "/x"` is the general
        // form; a bare word is accepted for the names that happen to be idents.
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
            Some(value) => quote!(::web_form::Attr::new(#name, #value)),
            None => quote!(::web_form::Attr::flag(#name)),
        }
    }
}

/// A name the built-in renderer already writes, and the key that sets it —
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

/// Parse one `attr(...)` group, appending to what earlier groups collected.
///
/// `owner` is the attribute the group was written in, so a rejected name can
/// point at the dedicated key that does the same job.
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

/// Reject a name that would not survive being written into markup, and one the
/// crate already writes itself.
fn check_attr_name(attr: &CustomAttr, owner: &str, reserved: &[Reserved]) -> Result<()> {
    if attr.name.is_empty() {
        return Err(Error::new(attr.span, "an attribute name cannot be empty"));
    }
    if let Some(bad) = attr
        .name
        .chars()
        .find(|ch| ch.is_whitespace() || ch.is_control() || "\"'>/=".contains(*ch))
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
    /// `context = Type`: what this form's own functions are handed. `None`
    /// means `()`.
    pub context: Option<syn::Type>,
    /// `attr(...)` entries, in the order they were written.
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
                    "attr" => parse_custom_attrs(&meta, "form", FORM_RESERVED, &mut out.custom)?,
                    other => {
                        return Err(meta.error(format!(
                            "unknown `form` attribute `{other}`; expected one of: id, name, \
                             action, method, enctype, class, submit, novalidate, validate, \
                             context, attr"
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
            // The label may be given positionally, as `#[option("de", "Germany")]`.
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

/// `#[field(...)]` plus any `#[option(...)]` entries on one struct field.
#[derive(Default)]
pub struct FieldAttrs {
    pub name: Option<String>,
    pub label: Option<TextAttr>,
    pub kind: Option<(String, proc_macro2::Span)>,
    pub required: Option<bool>,
    pub multiple: Option<bool>,
    pub default: Option<String>,
    /// `default = path`: a function called once per render, rather than a value
    /// written into the spec.
    pub default_fn: Option<Path>,
    pub placeholder: Option<TextAttr>,
    pub help: Option<TextAttr>,
    pub autocomplete: Option<String>,
    pub id: Option<String>,
    pub class: Option<String>,
    pub disabled: bool,
    pub readonly: bool,
    pub autofocus: bool,
    pub rows: Option<u32>,
    pub cols: Option<u32>,
    pub pattern: Option<String>,
    pub minlength: Option<usize>,
    pub maxlength: Option<usize>,
    pub min: Option<String>,
    pub max: Option<String>,
    pub step: Option<String>,
    pub accept: Option<String>,
    pub choices: Option<Path>,
    pub validate: Option<Path>,
    pub flatten: bool,
    pub prefix: Option<String>,
    pub legend: Option<TextAttr>,
    pub skip: bool,
    pub options: Vec<OptionAttr>,
    /// `attr(...)` entries, in the order they were written.
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
                // `type` is a keyword, which `parse_nested_meta` accepts in a
                // path but `get_ident` still reports by name.
                let key = meta
                    .path
                    .get_ident()
                    .map(Ident::to_string)
                    .unwrap_or_default();
                match key.as_str() {
                    "name" => out.name = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "label" => out.label = Some(meta.value()?.parse()?),
                    "type" | "kind" => {
                        let lit: LitStr = meta.value()?.parse()?;
                        out.kind = Some((lit.value(), lit.span()));
                    }
                    "required" => out.required = Some(parse_flag(&meta)?),
                    "optional" => out.required = Some(!parse_flag(&meta)?),
                    "multiple" => out.multiple = Some(parse_flag(&meta)?),
                    // A literal is the value itself; anything else names a
                    // function that produces one at render time.
                    "default" => {
                        let value = meta.value()?;
                        if value.peek(Lit) {
                            out.default = Some(lit_to_string(&value.parse()?)?);
                        } else {
                            out.default_fn = Some(value.parse().map_err(|e| {
                                Error::new(
                                    e.span(),
                                    "expected a literal default or the path of a function \
                                     returning one",
                                )
                            })?);
                        }
                        if out.default.is_some() && out.default_fn.is_some() {
                            return Err(meta.error(
                                "`default` is either a literal or a function that produces one, \
                                 not both",
                            ));
                        }
                    }
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
                    "rows" => {
                        out.rows = Some(meta.value()?.parse::<syn::LitInt>()?.base10_parse()?)
                    }
                    "cols" => {
                        out.cols = Some(meta.value()?.parse::<syn::LitInt>()?.base10_parse()?)
                    }
                    "pattern" => out.pattern = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "minlength" => {
                        out.minlength = Some(meta.value()?.parse::<syn::LitInt>()?.base10_parse()?)
                    }
                    "maxlength" => {
                        out.maxlength = Some(meta.value()?.parse::<syn::LitInt>()?.base10_parse()?)
                    }
                    "min" => out.min = Some(lit_to_string(&meta.value()?.parse()?)?),
                    "max" => out.max = Some(lit_to_string(&meta.value()?.parse()?)?),
                    "step" => out.step = Some(lit_to_string(&meta.value()?.parse()?)?),
                    "accept" => out.accept = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "choices" => out.choices = Some(meta.value()?.parse()?),
                    "validate" => out.validate = Some(meta.value()?.parse()?),
                    "flatten" => out.flatten = parse_flag(&meta)?,
                    "prefix" => out.prefix = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "legend" => out.legend = Some(meta.value()?.parse()?),
                    "skip" => out.skip = parse_flag(&meta)?,
                    "attr" => parse_custom_attrs(&meta, "field", FIELD_RESERVED, &mut out.custom)?,
                    other => {
                        return Err(meta.error(format!(
                            "unknown `field` attribute `{other}`; expected one of: name, label, \
                             type, required, optional, multiple, default, placeholder, help, \
                             autocomplete, id, class, disabled, readonly, autofocus, rows, cols, \
                             pattern, minlength, maxlength, min, max, step, accept, choices, \
                             validate, flatten, prefix, legend, skip, attr"
                        )));
                    }
                }
                Ok(())
            })?;
        }

        Ok(out)
    }
}

/// A bare word (`required`) or an explicit `required = false`.
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

/// `Option<&'static str>` — `Copy`, so it can be merged in a `const fn`.
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
