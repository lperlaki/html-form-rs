//! Derive macros for [`web-form`](https://docs.rs/web-form).
//!
//! Everything these macros generate is documented on the `web_form` crate; this
//! crate exists only because procedural macros need their own compilation unit.

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod attrs;
mod choice;
mod control;
mod form;
mod value;

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
/// | `from_str` | Convert with the type's own `FromStr`/`Display` instead of a `FormValue` impl |
/// | `flatten` (+ `prefix`, `legend`) | Splice another form in |
/// | `skip` | Leave the field out of the form; filled with `Default::default()` |
///
/// # A type from a crate that has never heard of this one
///
/// `from_str` is how a foreign type becomes a field: nothing but its own
/// `FromStr` and `Display` is asked of it, so a `Uuid`, a `NaiveDate` or a
/// `Decimal` needs no impl and no newtype.
///
/// ```ignore
/// #[derive(WebForm)]
/// struct Booking {
///     #[field(from_str, type = "date", min = "2026-01-01")]
///     day: NaiveDate,
///     #[field(from_str)]
///     reference: Uuid,
/// }
/// ```
///
/// It applies to the field's own type, `Option<T>` and `Vec<T>` included, and
/// everything else about the field is unchanged: the constraints in the spec
/// are checked first, a `validate` function is handed the type the field was
/// written as, and `Display` writes the value back out for an edit form.
///
/// Two things it does not do. It has no control to imply — a foreign type has
/// no `CONTROL` to be asked for one — so the field renders as text until
/// `type = "..."` says otherwise. And the message for a value that will not
/// parse is the generic "Enter a valid value.", because a `FromStr` error is
/// written for whoever wrote the call, not for whoever filled in the form. Both
/// are answered the same way: give the field the `type` or the `pattern` it
/// really has, whose check runs first and describes what it wanted.
///
/// For a type you own, `#[derive(FormValue)] #[value(from_str)]` says the same
/// thing once, on the type, instead of on every field of it.
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

/// Derive `FormValue` for a struct wrapping one value, so that a type carries
/// what every field of it would otherwise repeat.
///
/// The wrapped type does the converting — it is already a `FormValue` — and
/// `#[value(...)]` says what the wrapper adds on top:
///
/// ```ignore
/// #[derive(FormValue)]
/// #[value(type = "email", maxlength = 254, validate = is_company_address)]
/// struct WorkEmail(String);
///
/// fn is_company_address(email: &WorkEmail) -> Result<(), Text> {
///     match email.0.ends_with("@example.com") {
///         true => Ok(()),
///         false => Err(Text::key("invite.email.outside")),
///     }
/// }
/// ```
///
/// # `#[value(...)]`
///
/// | Attribute | Meaning |
/// |---|---|
/// | `type = "email"` | The control every field of this type renders as; the wrapped type's otherwise |
/// | `pattern`, `minlength`, `maxlength`, `min`, `max`, `step`, `accept`, `rows`, `cols`, `multiple` | Constraints the type carries, enforced in the browser *and* on the server |
/// | `choices = SOME_CONST` | A `&'static [Choice]` the value has to be one of |
/// | `default = "…"` | What a blank form shows for a field of this type |
/// | `validate = path::to::fn` | The type's own check: `fn(&Self) -> bool`, or `-> Result<(), E>` |
/// | `from_str` | Convert with the type's own `FromStr`/`Display` rather than through the value it wraps |
///
/// Everything here is a *default* a field may still override — `#[field(type =
/// "hidden")]` on a field of the type wins, exactly as an attribute wins over
/// the control a Rust type implies. The check is the type's own and always
/// runs, alongside any `#[field(validate = ...)]`.
///
/// What is deliberately absent is anything that describes the field rather than
/// the value: a label, help text, a placeholder. The same type is a "Work
/// email" on one form and a "Recipient" on the next.
///
/// # Converting through the wrapped value, or by itself
///
/// By default the wrapped type does the converting, which is why the derive
/// asks for a struct with exactly one field, named or not: a form control
/// submits one string, so there is one value to convert.
///
/// `#[value(from_str)]` says the type converts *itself*, through its own
/// `FromStr` and `Display`. That asks nothing of the type's shape, so it is
/// also what a several-field struct or an enum uses — and what turns a type
/// that already round-trips through a string into a form field once, rather
/// than at every field that mentions it:
///
/// ```ignore
/// #[derive(FormValue)]
/// #[value(from_str, pattern = r"\d+\.\d+\.\d+", default = "1.0.0")]
/// struct Version { major: u32, minor: u32, patch: u32 }
/// ```
///
/// It renders as text until `type = "..."` says otherwise — a type converting
/// itself says nothing about what it looks like — and a value it will not parse
/// is reported as the generic "Enter a valid value.", since a `FromStr` error
/// is written for whoever wrote the call. A `type` or a `pattern` is checked
/// first and says more.
///
/// A struct with several fields and no `from_str` is a form of its own: derive
/// `WebForm` and splice it in with `#[field(flatten)]`. A fieldless enum whose
/// variants are a `<select>`'s options is `#[derive(FormChoice)]`.
///
/// A validator is handed `&Self` and nothing else: a `FormValue` belongs to no
/// form, so there is no context to reach. A check that needs one belongs on the
/// field, as `#[field(validate = ...)]`.
#[proc_macro_derive(FormValue, attributes(value))]
pub fn derive_form_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    value::derive(input)
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
