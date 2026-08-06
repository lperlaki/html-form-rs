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
/// | `validate = path::to::fn` | Cross-field check, `fn(&Self) -> bool` or `-> Result<(), E>` |
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
/// | `default = path::to::fn` | A default produced once per render — a CSRF token, a nonce — instead of a fixed one |
/// | `pattern`, `minlength`, `maxlength`, `min`, `max`, `step`, `accept` | Validation, enforced in the browser *and* on the server |
/// | `placeholder`, `help`, `autocomplete`, `id`, `class`, `rows`, `cols` | Presentation |
/// | `disabled`, `readonly`, `autofocus`, `multiple` | Flags |
/// | `choices = SOME_CONST` | A `&'static [Choice]` of options |
/// | `validate = path::to::fn` | Per-field check, `fn(&FieldType) -> bool` or `-> Result<(), E>` |
/// | `flatten` (+ `prefix`, `legend`) | Splice another form in |
/// | `skip` | Leave the field out of the form; filled with `Default::default()` |
///
/// Options can also be listed inline with repeated `#[option("value", "Label")]`
/// attributes, which additionally accept `group = "…"` and `disabled`.
///
/// # Generic forms
///
/// A struct with type parameters derives like any other: `SPEC` is an
/// associated constant, so `<T as WebForm>::SPEC` is resolved per
/// instantiation. The bounds a field implies — `WebForm` for one that is
/// flattened, `FormValue` for one that is a value — are added for you, so a
/// wrapper form needs none written on it:
///
/// ```ignore
/// #[derive(WebForm)]
/// #[form(method = "post")]
/// struct WithCsrf<T> {
///     #[field(type = "hidden", default = session_token, validate = belongs_to_session)]
///     csrf_token: String,
///     #[field(flatten)]
///     inner: T,
/// }
/// ```
///
/// A flatten splices in the sub-form's *fields*; its `action`, `method` and
/// submit label belong to its own `<form>` element, so a wrapper declares its
/// own. See `examples/csrf.rs`.
///
/// # What a `validate` function may return
///
/// A `bool`, or a `Result` whose error is anything that becomes a message: a
/// `&'static str`, a `String`, a `Text` (so an i18n key), or a `FieldError`.
/// A `#[form(validate = ...)]` function may additionally return a
/// `(field, message)` pair or a whole `FormErrors`, so a cross-field check can
/// blame the field the user has to change. See `web_form::FieldValidation` and
/// `web_form::FormValidation`.
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
