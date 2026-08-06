//! Derive macros for [`web-form`](https://docs.rs/web-form).
//!
//! Everything these macros generate is documented on the `web_form` crate; this
//! crate exists only because procedural macros need their own compilation unit.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod attrs;
mod choice;
mod form;

/// Derive `WebForm` for a struct with named fields.
///
/// # Struct attributes — `#[form(...)]`
///
/// | Attribute | Meaning |
/// |---|---|
/// | `id`, `name`, `action`, `class` | Rendered onto the `<form>` element |
/// | `method = "post"` | `get`, `post` or `dialog` (default `post`) |
/// | `enctype = "multipart/form-data"` | Encoding, e.g. for file uploads |
/// | `novalidate` | Suppress the browser's own validation |
/// | `submit = "Create account"` | Caption of the built-in submit button |
/// | `validate = path::to::fn` | Cross-field check, `fn(&Self) -> Result<(), E>` |
///
/// # Field attributes — `#[field(...)]`
///
/// | Attribute | Meaning |
/// |---|---|
/// | `type = "email"` | The control to render; inferred from the Rust type otherwise |
/// | `label = "Email"` | Label text; defaults to the humanised field name, `""` renders none |
/// | `label = t("email.label")` | An i18n key instead of text. Also on `help`, `placeholder`, `legend`, and on `#[form(submit)]`, `#[option(...)]` and `#[choice(...)]` labels and groups |
/// | `name = "e-mail"` | Submitted name; defaults to the field name |
/// | `required` / `optional` | Overrides the default (required unless `Option`, `Vec` or `bool`) |
/// | `default = "…"` | Value shown on a blank form |
/// | `pattern`, `minlength`, `maxlength`, `min`, `max`, `step`, `accept` | Validation, enforced in the browser *and* on the server |
/// | `placeholder`, `help`, `autocomplete`, `id`, `class`, `rows`, `cols` | Presentation |
/// | `disabled`, `readonly`, `autofocus`, `multiple` | Flags |
/// | `choices = SOME_CONST` | A `&'static [Choice]` of options |
/// | `validate = path::to::fn` | Per-field check, `fn(&FieldType) -> Result<(), E>` |
/// | `flatten` (+ `prefix`, `legend`) | Splice another form in |
/// | `skip` | Leave the field out of the form; filled with `Default::default()` |
///
/// Options can also be listed inline with repeated `#[option("value", "Label")]`
/// attributes, which additionally accept `group = "…"` and `disabled`.
///
/// # Attributes that do not apply
///
/// Each control accepts only the attributes HTML gives it, so
/// `#[field(type = "date", minlength = 3)]` and a `pattern` on a `u32` field are
/// compile errors rather than attributes that quietly do nothing. The check
/// happens during `const` evaluation of the generated spec, because which
/// control a field renders as can come from its Rust type — which a macro
/// cannot inspect.
#[proc_macro_derive(WebForm, attributes(form, field, option))]
pub fn derive_web_form(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    form::derive(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive `FormValue` for a fieldless enum, turning it into a `<select>` whose
/// options are the variants.
///
/// Each variant submits its name in `kebab-case` and is labelled with its name
/// split into words; both can be overridden:
///
/// ```ignore
/// #[derive(FormChoice)]
/// enum Plan {
///     Free,
///     #[choice(value = "pro", label = "Professional")]
///     Pro,
///     #[choice(disabled, group = "Coming soon")]
///     Enterprise,
/// }
/// ```
#[proc_macro_derive(FormChoice, attributes(choice))]
pub fn derive_form_choice(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    choice::derive(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
