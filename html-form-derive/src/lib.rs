//! Derive macros for [`html-form`](https://docs.rs/html-form).
//!
//! The `html_form` crate documents everything these macros generate. This crate
//! exists only because a procedural macro needs its own compilation unit.
//!
//! Nothing it emits is `unsafe` either. The macro writes calls into
//! `html_form::__private`, and `html_form` itself is `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod attrs;
mod choice;
mod control;
mod form;
mod response;
mod value;

/// Derive `Form` for a struct with named fields.
///
/// # Struct attributes — `#[form(...)]`
///
/// | Attribute | Meaning |
/// |---|---|
/// | `id`, `name`, `action`, `class` | The crate renders these onto the `<form>` element |
/// | `method = "post"` | `get`, `post` or `dialog`. Defaults to `post` |
/// | `enctype = "multipart/form-data"` | The encoding, such as for file uploads |
/// | `novalidate` | Turn off the browser's own validation |
/// | `submit = "Create account"` | The caption of the built-in submit button |
/// | `validate = path::to::fn` | Cross-field check, `fn(&Self) -> bool` or `-> Result<(), E>` |
/// | `context = Session` | What this form's own functions receive besides the value they look at |
/// | `renderer = Page` | The struct becomes an axum extractor and response. The `axum` feature |
/// | `status`, `from_request`, `into_response` | What `renderer` generates. See below |
///
/// # Field attributes — `#[field(...)]`
///
/// | Attribute | Meaning |
/// |---|---|
/// | `type = "email"` | The control to render. The crate infers it from the Rust type otherwise |
/// | `label = "Email"` | The label text. It defaults to the humanized field name, and `""` renders no label |
/// | `label = t("email.label")` | An i18n key in place of text. Also on `help`, `placeholder`, `legend`, and on the labels and groups of `#[form(submit)]`, `#[option(...)]` and `#[choice(...)]` |
/// | `name = "e-mail"` | The submitted name. Defaults to the field name |
/// | `required` / `optional` | Overrides the default, which is required unless the type is `Option`, `Vec` or `bool` |
/// | `default = "…"` | The value a blank form shows |
/// | `default = path::to::fn` | A default the crate produces once per render, such as a CSRF token or a nonce, in place of a fixed one |
/// | `pattern`, `minlength`, `maxlength`, `min`, `max`, `step`, `accept` | Validation. The browser *and* the server enforce these |
/// | `placeholder`, `help`, `autocomplete`, `id`, `class`, `rows`, `cols` | Presentation |
/// | `disabled`, `readonly`, `autofocus`, `multiple` | Flags |
/// | `choices = SOME_CONST` | A `&'static [Choice]` of options |
/// | `validate = path::to::fn` | Per-field check, `fn(&FieldType) -> bool` or `-> Result<(), E>` |
/// | `from_str` | Convert with the type's own `FromStr` and `Display` in place of a `FormValue` impl |
/// | `flatten` (+ `prefix`, `legend`) | Put another form in |
/// | `skip` | Leave the field out of the form. The crate fills it with `Default::default()` |
///
/// # A type from a crate that has never heard of this one
///
/// `from_str` is how a foreign type becomes a field. The crate asks it for
/// nothing but its own `FromStr` and `Display`, so a `Uuid`, a `NaiveDate` or a
/// `Decimal` needs no impl and no newtype.
///
/// ```ignore
/// #[derive(Form)]
/// struct Booking {
///     #[field(from_str, type = "date", min = "2026-01-01")]
///     day: NaiveDate,
///     #[field(from_str)]
///     reference: Uuid,
/// }
/// ```
///
/// It applies to the field's own type, `Option<T>` and `Vec<T>` included.
/// Everything else about the field stays the same. The crate checks the
/// constraints in the spec first. A `validate` function gets the type the field
/// was written as, and `Display` writes the value out again for an edit form.
///
/// There are two things it does not do. It implies no control, because a
/// foreign type has no `CONTROL` to give one, so the field renders as text
/// until `type = "..."` says otherwise. And a value that will not parse gets
/// the general message "Enter a valid value." A `FromStr` error speaks to
/// whoever wrote the call, not to whoever filled in the form. One answer covers
/// both: give the field the `type` or the `pattern` it really has. That check
/// runs first, and it says what it wanted.
///
/// For a type you own, `#[derive(FormValue)] #[value(from_str)]` says the same
/// thing once, on the type, in place of on every field of it.
///
/// You can also list options inline with repeated
/// `#[option("value", "Label")]` attributes, which also accept `group = "…"`
/// and `disabled`.
///
/// # Generic forms
///
/// A struct with type parameters derives like any other. `SPEC` is an
/// associated constant, so the compiler resolves `<T as Form>::SPEC` per
/// instantiation. The derive adds the bounds each field implies: `Form` for a
/// flattened field, and `FormValue` for a value field. A wrapper form therefore
/// needs no bound written on it:
///
/// ```ignore
/// #[derive(Form)]
/// #[form(method = "post")]
/// struct WithCsrf<T> {
///     #[field(type = "hidden", default = session_token, validate = belongs_to_session)]
///     csrf_token: String,
///     #[field(flatten)]
///     inner: T,
/// }
/// ```
///
/// A flatten brings in the sub-form's *fields*. Its `action`, `method` and
/// submit label belong to its own `<form>` element, so a wrapper declares its
/// own. See `examples/csrf.rs`.
///
/// # What a `validate` function may return
///
/// A `bool`, or a `Result` whose error is anything that becomes a message. That
/// is a `&'static str`, a `String`, a `Text`, which carries an i18n key, or a
/// `FieldError`. A `#[form(validate = ...)]` function may also return a
/// `(field, message)` pair or a whole `FormErrors`. A cross-field check can
/// therefore name the field the user has to change. See
/// `html_form::FieldValidation` and `html_form::FormValidation`.
///
/// # A form that is its own axum extractor — `#[form(renderer = ...)]`
///
/// The `axum` feature. Name what answers a failed submission, and the struct
/// itself becomes the argument a handler takes and the response it returns:
///
/// ```ignore
/// #[derive(Form)]
/// #[form(method = "post", renderer = Page)]
/// struct Signup {
///     #[field(type = "email")]
///     email: String,
/// }
///
/// async fn signup(form: Signup) -> Html<String> { … }
/// ```
///
/// It generates a unit struct named after the form, `SignupRenderer`, which is
/// the `Renderer` the named type, function or closure is called from — a
/// function has no type anybody can write down, so the derive makes one that
/// can be. `HasRenderer for Signup` names it, and is what a bound asks for when
/// it means "a form that knows how it is rendered". From there it is
/// `html_form::axum::Form<Signup, SignupRenderer>` with the wrapper taken off:
/// `FromRequest` and `IntoResponse` for the form itself, and the rejection, the
/// statuses and the context extraction that extractor already had.
///
/// | Attribute | Meaning |
/// |---|---|
/// | `renderer = Page` | A unit struct implementing `Renderer`, or a `fn(FormView) -> impl IntoResponse`, which may take `&Context` after the view. A closure works too, and a `fn` pointer is a compile error |
/// | `status = 201` | What the derived `IntoResponse` answers with. A number or an `http::StatusCode`, checked while the compiler evaluates it. `200` otherwise |
/// | `from_request = false` | Leave the extractor out |
/// | `into_response = false` | Leave the response out |
///
/// The response is left out on its own where the form declares a `context`,
/// because a value renders itself with no context to render it under.
/// `into_response` puts it back where that context is an
/// `html_form::EmptyContext`. The status of a *failed* submission is not this
/// attribute's to set: it belongs to the rejection, which is what knows the
/// submission was bad.
///
/// # Attributes that do not apply
///
/// Each control accepts only the attributes HTML gives it. So
/// `#[field(type = "date", minlength = 3)]` and a `pattern` on a `u32` field
/// are compile errors, not attributes that quietly do nothing. The check
/// happens while the compiler const-evaluates the generated spec. The control a
/// field renders as can come from its Rust type, which a macro cannot inspect.
#[proc_macro_derive(Form, attributes(form, field, option))]
pub fn derive_form(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    form::derive(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive `FormValue` for a struct that wraps one value, so the type carries
/// what every field of it would otherwise repeat.
///
/// The wrapped type does the conversion, because it is already a `FormValue`.
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
/// | `type = "email"` | The control every field of this type renders as. The wrapped type's control otherwise |
/// | `pattern`, `minlength`, `maxlength`, `min`, `max`, `step`, `accept`, `rows`, `cols`, `multiple` | Constraints the type carries. The browser *and* the server enforce them |
/// | `choices = SOME_CONST` | A `&'static [Choice]`. The value has to be one of them |
/// | `default = "…"` | What a blank form shows for a field of this type |
/// | `validate = path::to::fn` | The type's own check: `fn(&Self) -> bool`, or `-> Result<(), E>` |
/// | `from_str` | Convert with the type's own `FromStr` and `Display` in place of the value it wraps |
///
/// Everything here is a *default* that a field may still override.
/// `#[field(type = "hidden")]` on a field of the type wins, exactly as an
/// attribute wins over the control a Rust type implies. The check belongs to
/// the type and always runs, next to any `#[field(validate = ...)]`.
///
/// What is missing on purpose is anything that describes the field rather than
/// the value: a label, help text, a placeholder. The same type is a "Work
/// email" on one form and a "Recipient" on the next.
///
/// # Converting through the wrapped value, or by itself
///
/// By default the wrapped type does the conversion. That is why the derive asks
/// for a struct with exactly one field, named or not. A form control submits
/// one string, so there is one value to convert.
///
/// `#[value(from_str)]` says the type converts *itself*, through its own
/// `FromStr` and `Display`. That asks nothing of the type's shape, so a
/// several-field struct or an enum uses it too. It also turns a type that
/// already round-trips through a string into a form field once, in place of at
/// every field that names it:
///
/// ```ignore
/// #[derive(FormValue)]
/// #[value(from_str, pattern = r"\d+\.\d+\.\d+", default = "1.0.0")]
/// struct Version { major: u32, minor: u32, patch: u32 }
/// ```
///
/// It renders as text until `type = "..."` says otherwise, because a type that
/// converts itself says nothing about how it looks. A value it will not parse
/// gets the general message "Enter a valid value." A `FromStr` error speaks to
/// whoever wrote the call. The crate checks a `type` or a `pattern` first, and
/// that check says more.
///
/// A struct with several fields and no `from_str` is a form of its own. Derive
/// `Form` and put it in with `#[field(flatten)]`. For a fieldless enum whose
/// variants are the options of a `<select>`, use `#[derive(FormChoice)]`.
///
/// A validator receives `&Self` and nothing else. A `FormValue` belongs to no
/// form, so there is no context to reach. A check that needs one belongs on the
/// field, as `#[field(validate = ...)]`.
#[proc_macro_derive(FormValue, attributes(value))]
pub fn derive_form_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    value::derive(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive `FormValue` for a fieldless enum, which turns it into a `<select>`
/// whose options are the variants.
///
/// Each variant submits its name in `kebab-case`. Its label is its name split
/// into words. You can override both:
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
