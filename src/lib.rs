//! Declarative HTML forms.
//!
//! One struct describes a form. From it you get:
//!
//! * a **render format** — [`FormView`], a flat serialisable description of the
//!   form, ready for MiniJinja, Askama or the built-in HTML renderer;
//! * a **parser** — the same struct is reconstructed from an
//!   `application/x-www-form-urlencoded` submission, with every attribute
//!   re-checked server-side;
//! * an **error render** — when validation fails you get the same
//!   [`FormView`], carrying the values the user already typed and the messages
//!   attached to each field.
//!
//! Validation never stops at the first problem. Every field is attempted and
//! every failure is collected into one [`FormErrors`].
//!
//! # Example
//!
//! ```
//! use web_form::{Outcome, WebForm};
//!
//! #[derive(WebForm)]
//! #[form(action = "/signup", method = "post", submit = "Create account")]
//! struct Signup {
//!     #[field(type = "email", label = "Email address", autocomplete = "email")]
//!     email: String,
//!
//!     #[field(type = "password", label = "Password", minlength = 12,
//!             help = "At least 12 characters.")]
//!     password: String,
//!
//!     #[field(label = "Age", min = 18, max = 120)]
//!     age: Option<u32>,
//!
//!     #[field(label = "Subscribe to the newsletter", default = true)]
//!     newsletter: bool,
//! }
//!
//! // Rendering a blank form.
//! let html = Signup::render().to_html();
//! assert!(html.contains(r#"<input type="email" name="email""#));
//!
//! // A bad submission: every problem is reported at once.
//! let body = "email=nope&password=short&age=7";
//! match Signup::submit_urlencoded(body) {
//!     Outcome::Valid(_) => unreachable!(),
//!     Outcome::Invalid { errors, view } => {
//!         // Bad address, short password, age below the minimum.
//!         assert_eq!(errors.len(), 3);
//!         // The re-render keeps what the user typed…
//!         assert_eq!(view.field("email").unwrap().value.as_deref(), Some("nope"));
//!         // …and carries the messages.
//!         assert!(view.field("age").unwrap().errors[0].contains("18"));
//!     }
//! }
//!
//! // A good one.
//! let signup = Signup::from_urlencoded(
//!     "email=a@example.com&password=correct-horse-battery&age=30&newsletter=on",
//! )
//! .unwrap();
//! assert_eq!(signup.age, Some(30));
//! assert!(signup.newsletter);
//! ```
//!
//! # Checks the markup cannot express
//!
//! `validate = ...` names a function run after everything the spec could check
//! — per field with `#[field(...)]`, or once over the assembled struct with
//! `#[form(...)]`. A predicate is enough when the built-in message will do;
//! return a `Result` to say more. See [`FieldValidation`] and
//! [`FormValidation`] for every shape either one may return.
//!
//! ```
//! use web_form::{FormErrors, WebForm};
//!
//! #[derive(WebForm, Debug)]
//! #[form(validate = passwords_match)]
//! struct Signup {
//!     #[field(validate = is_available)]
//!     username: String,
//!     #[field(type = "password")]
//!     password: String,
//!     #[field(type = "password")]
//!     confirm: String,
//! }
//!
//! fn is_available(name: &String) -> bool {
//!     name != "admin"
//! }
//!
//! fn passwords_match(form: &Signup) -> Result<(), FormErrors> {
//!     if form.password == form.confirm {
//!         Ok(())
//!     } else {
//!         // Attached to the field that can be corrected, not to the form.
//!         Err(("confirm", "The two passwords do not match.").into())
//!     }
//! }
//!
//! let errors =
//!     Signup::from_urlencoded("username=admin&password=a&confirm=b").unwrap_err();
//! assert!(errors.has_field("username") && errors.has_field("confirm"));
//! ```
//!
//! # Localisation
//!
//! Any string a person reads — a label, help text, a placeholder, a legend, an
//! option's label, an error message — can be written as `t("key")` instead of
//! as text. The crate resolves nothing itself; hand [`FormView::localize`] a
//! lookup, or read the `…_key` companion field and translate in the template.
//!
//! ```
//! use web_form::WebForm;
//!
//! #[derive(WebForm)]
//! #[form(submit = t("signup.submit"))]
//! struct Signup {
//!     #[field(type = "email", label = t("signup.email"), help = "Never shared.")]
//!     email: String,
//! }
//!
//! let view = Signup::render_localized(|key| match key {
//!     "signup.email" => Some("E-Mail-Adresse"),
//!     "signup.submit" => Some("Konto erstellen"),
//!     _ => None,
//! });
//!
//! assert_eq!(view.field("email").unwrap().label.as_deref(), Some("E-Mail-Adresse"));
//! assert_eq!(view.submit_label, "Konto erstellen");
//! // A literal is a literal, whatever language the rest is in.
//! assert_eq!(view.field("email").unwrap().help.as_deref(), Some("Never shared."));
//! ```
//!
//! A key nothing recognises is left in place — the view shows the key itself,
//! which is a visible bug rather than a silently blank label.
//!
//! A `validate = ...` function localises the same way: return a [`Text::key`]
//! and the message is resolved with everything else. The key doubles as the
//! error's [code](ErrorKind::Custom), so a caller that would rather build its
//! own message can match on that instead.
//!
//! ```
//! use web_form::{Outcome, Text, WebForm};
//!
//! #[derive(WebForm)]
//! struct Signup {
//!     #[field(label = "Username", validate = is_available)]
//!     username: String,
//! }
//!
//! fn is_available(name: &String) -> Result<(), Text> {
//!     match name.as_str() {
//!         "admin" => Err(Text::key("signup.username.taken")),
//!         _ => Ok(()),
//!     }
//! }
//!
//! let Outcome::Invalid { errors, view } = Signup::submit_urlencoded("username=admin") else {
//!     panic!("expected the reserved name to be rejected");
//! };
//! assert_eq!(errors.field("username").next().unwrap().code(), Some("signup.username.taken"));
//!
//! let view = view.localized(|key| (key == "signup.username.taken").then_some("Schon vergeben."));
//! assert_eq!(view.field("username").unwrap().errors[0], "Schon vergeben.");
//! ```
//!
//! The messages of the *built-in* checks are English, and deliberately not
//! keyed: every [`ErrorKind`] carries the constraint it was violating, so a
//! caller that needs them translated matches on the kind and writes its own —
//! there is no key to guess at, and no message table to keep in step.
//!
//! # Reuse: flattening one form into another
//!
//! A form can be spliced into another with `#[field(flatten)]`. Give the
//! flatten a `prefix` to embed the same sub-form more than once.
//!
//! ```
//! use web_form::WebForm;
//!
//! #[derive(WebForm)]
//! struct Address {
//!     #[field(label = "Street")]
//!     street: String,
//!     #[field(label = "Postcode", pattern = r"\d{4,5}")]
//!     postcode: String,
//! }
//!
//! #[derive(WebForm)]
//! struct Order {
//!     #[field(label = "Customer")]
//!     customer: String,
//!
//!     #[field(flatten, prefix = "billing_", legend = "Billing address")]
//!     billing: Address,
//!
//!     #[field(flatten, prefix = "shipping_", legend = "Shipping address")]
//!     shipping: Address,
//! }
//!
//! let view = Order::render();
//! let names: Vec<&str> = view.fields.iter().map(|f| f.name.as_ref()).collect();
//! assert_eq!(
//!     names,
//!     [
//!         "customer",
//!         "billing_street",
//!         "billing_postcode",
//!         "shipping_street",
//!         "shipping_postcode"
//!     ]
//! );
//!
//! let order = Order::from_urlencoded(
//!     "customer=Ada&billing_street=Main+1&billing_postcode=12345\
//!      &shipping_street=Side+2&shipping_postcode=54321",
//! )
//! .unwrap();
//! assert_eq!(order.shipping.postcode, "54321");
//! ```
//!
//! # Attributes the crate has no opinion about
//!
//! `attr(...)` carries anything else the markup needs — `data-*`, `hx-*` — onto
//! the `<form>` or onto one control. It never takes part in validation.
//!
//! ```
//! use web_form::WebForm;
//!
//! #[derive(WebForm)]
//! #[form(attr("hx-post" = "/search", "hx-target" = "#results"))]
//! struct Search {
//!     #[field(attr("hx-trigger" = "keyup changed delay:300ms", autocorrect = "off"))]
//!     query: String,
//! }
//!
//! let html = Search::render().to_html();
//! assert!(html.contains(r#"hx-post="/search""#));
//! assert!(html.contains(r#"autocorrect="off""#));
//! ```
//!
//! A dashed name has to be written as a string literal; a bare word is taken as
//! written, and one with no value renders a boolean attribute. Naming something
//! the crate renders itself is a compile error, so `attr("class" = "x")` points
//! you at `#[field(class = "x")]` rather than emitting a second `class`.
//!
//! # Rendering with a template engine
//!
//! [`FormView`] is `serde::Serialize`, so it drops straight into a MiniJinja
//! context, and its fields are public, so an Askama template can walk it
//! directly. See `examples/minijinja_render.rs`.
//!
//! The built-in renderer — [`FormView::to_html`], [`FieldView::to_html`], the
//! `Display` impls and [`escape`] — is the `html` feature, on by default. A
//! crate that renders through a template engine can turn it off; everything
//! else, [`FormView`] included, is unaffected.
//!
//! # Framework integration
//!
//! Nothing here is tied to an HTTP stack: [`Values::from_pairs`] takes whatever
//! a framework's body parser produced. With the `axum` feature, [`Outcome<T>`]
//! is additionally an axum 0.8 extractor — see `FormRejection` and
//! `examples/axum_signup.rs`.
//!
//! # The two descriptions of a form
//!
//! [`FormSpec`] is the static one: what the derive emits, and what both the
//! renderer and the parser read. Each of its fields names a [`Control`], which
//! carries the attributes that control accepts and no others — there is no way
//! to give a date a `minlength` or a `<select>` an `accept`.
//!
//! [`FormView`] is the runtime one: flat, serialisable, and carrying the things
//! a spec cannot know — what the user typed, what was wrong with it, and options
//! loaded from a database.
//!
//! [`Text`] is what every person-facing string in the spec is: literal text, or
//! an i18n key. Both end up in the view, the key alongside the string.
//!
//! # What rendering costs
//!
//! A spec is `const`: [`WebForm::SPEC`] is one const-evaluated value, flattened
//! sub-forms and all, so nothing is built the first time a form is rendered.
//!
//! Every string in a [`FormView`] is a `Cow<'static, str>`, and rendering
//! borrows from the spec wherever it can. What a blank form still allocates is
//! what the spec does not contain: the three ids derived from each field's name,
//! the `Vec`s holding the fields and their options, and — on a re-render — the
//! values the user submitted, which do not outlive the request they came in on.
//! Names, labels, help text, placeholders, options, constraints and error
//! messages are borrowed. The owned half of each `Cow` is what keeps the view an
//! ordinary owned value that a handler can return, and what lets `set_value`,
//! `set_choices` and [`FormView::localize`] put runtime strings in.
//!
//! The same holds while parsing: a field's name reaches [`FormErrors`] borrowed
//! from the spec, and only a `#[field(flatten, prefix = "…")]` makes a name that
//! has to be built.

#[cfg(feature = "axum")]
mod axum;
mod error;
#[cfg(feature = "html")]
mod html;
mod kind;
mod runtime;
mod spec;
mod validate;
mod value;
mod values;
mod view;

#[cfg(feature = "axum")]
pub use axum::FormRejection;
pub use error::{ErrorKind, FieldError, FormErrors, ValueError};
#[cfg(feature = "html")]
pub use html::escape;
pub use kind::FieldKind;
pub use runtime::ParseCtx;
pub use spec::{
    Attr, Bounds, Choice, ChoiceStyle, ChooseControl, Control, Entry, FieldSpec, FileControl,
    Flattened, FormEncType, FormMethod, FormSpec, NumberControl, NumberFormat, ResolvedField,
    TemporalControl, TemporalFormat, Text, TextControl, TextFormat, TextareaControl,
};
pub use validate::{FieldValidation, FormValidation};
pub use value::FormValue;
pub use values::Values;
pub use view::{AttrView, ChoiceView, FieldView, FormView};

#[doc(hidden)]
pub use runtime::__private;

#[cfg(feature = "derive")]
pub use web_form_derive::{FormChoice, WebForm};

/// Everything needed to declare and use a form.
pub mod prelude {
    // `WebForm` names both the trait and, with the `derive` feature, the macro.
    #[cfg(feature = "derive")]
    pub use crate::{Choice, FormChoice};
    pub use crate::{FieldKind, FormErrors, FormValue, FormView, Outcome, Values, WebForm};
}

/// The result of handling a submission.
///
/// The invalid case carries both the typed errors and a [`FormView`] that is
/// ready to render back to the user, complete with the values they already
/// entered.
#[derive(Debug)]
pub enum Outcome<T> {
    Valid(T),
    Invalid {
        errors: FormErrors,
        view: Box<FormView>,
    },
}

impl<T> Outcome<T> {
    pub fn is_valid(&self) -> bool {
        matches!(self, Outcome::Valid(_))
    }

    /// The parsed value, discarding the re-render.
    pub fn ok(self) -> Option<T> {
        match self {
            Outcome::Valid(value) => Some(value),
            Outcome::Invalid { .. } => None,
        }
    }

    /// The re-render, discarding the parsed value.
    pub fn view(self) -> Option<Box<FormView>> {
        match self {
            Outcome::Valid(_) => None,
            Outcome::Invalid { view, .. } => Some(view),
        }
    }

    /// Turn into a `Result` whose error is the form to render again.
    pub fn into_result(self) -> Result<T, Box<FormView>> {
        match self {
            Outcome::Valid(value) => Ok(value),
            Outcome::Invalid { view, .. } => Err(view),
        }
    }
}

/// A struct that describes an HTML form.
///
/// Implemented by `#[derive(WebForm)]`. The three required members are what the
/// derive generates; everything else is provided.
pub trait WebForm: Sized {
    /// The static description of this form.
    ///
    /// A constant rather than a function: the derive builds it entirely at
    /// const-evaluation time, so a flattened sub-form is a reference the
    /// compiler has already resolved rather than a call made at render time,
    /// and `SPEC` can be read from any `const` context.
    const SPEC: &'static FormSpec;

    /// Parse the form out of `ctx`, honouring the flatten prefix in scope.
    ///
    /// Returns `None` when a value could not be produced. Errors are pushed
    /// into the context rather than returned, so parsing always visits every
    /// field.
    fn parse_in(ctx: &mut ParseCtx<'_>) -> Option<Self>;

    /// Write this value back out as raw submitted values, so an existing record
    /// can be rendered into the form it came from.
    fn fill_in(&self, values: &mut Values, prefix: &str);

    /// [`WebForm::SPEC`], for call sites that would rather not name the type.
    fn spec() -> &'static FormSpec {
        Self::SPEC
    }

    /// Parse and validate a submission.
    fn from_values(values: &Values) -> Result<Self, FormErrors> {
        let mut ctx = ParseCtx::new(values);
        let parsed = Self::parse_in(&mut ctx);
        let errors = ctx.into_errors();
        match parsed {
            Some(value) if errors.is_empty() => Ok(value),
            _ => Err(errors),
        }
    }

    /// Parse and validate an `application/x-www-form-urlencoded` body or query
    /// string.
    fn from_urlencoded(body: &str) -> Result<Self, FormErrors> {
        Self::from_values(&Values::parse(body))
    }

    /// Parse a submission, and on failure build the form to show again.
    fn submit(values: &Values) -> Outcome<Self> {
        match Self::from_values(values) {
            Ok(value) => Outcome::Valid(value),
            Err(errors) => {
                let view = Box::new(Self::render_with(values, &errors));
                Outcome::Invalid { errors, view }
            }
        }
    }

    /// [`WebForm::submit`] straight from a request body.
    fn submit_urlencoded(body: &str) -> Outcome<Self> {
        Self::submit(&Values::parse(body))
    }

    /// A blank form, with each field showing its declared default.
    fn render() -> FormView {
        FormView::build(Self::SPEC, None, &FormErrors::new())
    }

    /// A blank form with every i18n key already resolved.
    ///
    /// The general form is [`FormView::localized`], which does the same to a
    /// view from any of the other constructors:
    /// `Signup::render_filled(&article).localized(&translate)`.
    fn render_localized<S, F>(translate: F) -> FormView
    where
        F: Fn(&str) -> Option<S>,
        S: Into<std::borrow::Cow<'static, str>>,
    {
        Self::render().localized(translate)
    }

    /// The form as it should be shown after a submission: the submitted values
    /// with the errors attached to their fields.
    fn render_with(values: &Values, errors: &FormErrors) -> FormView {
        FormView::build(Self::SPEC, Some(values), errors)
    }

    /// The form filled in from an existing value — an edit form.
    fn render_filled(&self) -> FormView {
        FormView::build(Self::SPEC, Some(&self.to_values()), &FormErrors::new())
    }

    /// This value as raw submitted values.
    fn to_values(&self) -> Values {
        let mut values = Values::new();
        self.fill_in(&mut values, "");
        values
    }
}
