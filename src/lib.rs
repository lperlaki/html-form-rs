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
//! use html_form::{Form, Outcome};
//!
//! #[derive(Form)]
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
//! use html_form::{Form, FormErrors};
//!
//! #[derive(Form, Debug)]
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
//! # A type that carries its own rules
//!
//! A check the whole application makes of a value does not belong to one field
//! of one form. `#[derive(FormValue)]` puts it on the type instead: the wrapped
//! type does the converting, and `#[value(...)]` says what the wrapper adds —
//! the control, the constraints, the default, and the check no markup could
//! make. A form that uses the type then says only where it goes.
//!
//! ```
//! use html_form::{Form, Text};
//!
//! #[derive(html_form::FormValue, Debug)]
//! #[value(type = "email", maxlength = 254, validate = is_company_address)]
//! struct WorkEmail(String);
//!
//! fn is_company_address(email: &WorkEmail) -> Result<(), Text> {
//!     match email.0.ends_with("@example.com") {
//!         true => Ok(()),
//!         false => Err(Text::key("invite.email.outside")),
//!     }
//! }
//!
//! #[derive(Form, Debug)]
//! struct Invite {
//!     #[field(label = "Who should we invite?")]
//!     colleague: WorkEmail,
//! }
//!
//! // The control, and everything it constrains, came from the type.
//! let view = Invite::render();
//! let field = view.field("colleague").unwrap();
//! assert_eq!(field.kind, html_form::FieldKind::Email);
//! assert_eq!(field.maxlength, Some(254));
//!
//! // So did the check, which runs on every form that uses the type.
//! let errors = Invite::from_urlencoded("colleague=ada@example.org").unwrap_err();
//! assert_eq!(
//!     errors.field("colleague").next().unwrap().code(),
//!     Some("invite.email.outside")
//! );
//! ```
//!
//! What a field writes wins over what the type said, exactly as an attribute
//! wins over the control a Rust type implies. What the type will not carry is
//! anything describing the *field* — a label, a placeholder — since the same
//! type is a "Work email" on one form and a "Recipient" on the next. See
//! [`FormValue`] for the trait, and the derive for every `#[value(...)]` key.
//!
//! # A type from a crate that has never heard of this one
//!
//! `#[field(from_str)]` converts a field with the type's own [`FromStr`] and
//! [`Display`], so a `Uuid`, a `NaiveDate` or a `Decimal` is a field with no
//! impl written and no newtype wrapped around it.
//!
//! ```
//! use html_form::Form;
//! # use std::fmt;
//! # use std::str::FromStr;
//! # #[derive(Debug, PartialEq)]
//! # struct Date(String);
//! # impl FromStr for Date {
//! #     type Err = &'static str;
//! #     fn from_str(raw: &str) -> Result<Self, Self::Err> { Ok(Date(raw.to_owned())) }
//! # }
//! # impl fmt::Display for Date {
//! #     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
//! # }
//!
//! #[derive(Form, Debug)]
//! struct Booking {
//!     #[field(from_str, type = "date", min = "2026-01-01")]
//!     day: Date,
//! }
//!
//! let booking = Booking::from_urlencoded("day=2026-08-06").unwrap();
//! assert_eq!(booking.day, Date("2026-08-06".to_owned()));
//!
//! // Everything the spec could check is checked first, and says more than a
//! // conversion could: this is the date control's own format check.
//! let errors = Booking::from_urlencoded("day=whenever").unwrap_err();
//! assert_eq!(
//!     errors.field("day").next().unwrap().message.as_str(),
//!     "Enter a date as YYYY-MM-DD."
//! );
//! ```
//!
//! It applies to the field's own type, `Option<T>` and `Vec<T>` included, and
//! changes nothing else: a `validate` function still sees the type the field
//! was written as, and `Display` writes the value back out for an edit form.
//! What it cannot do is imply a control — a foreign type has no `CONTROL` to be
//! asked for one — so the field renders as text until `type = "..."` says
//! otherwise, and a value that will not parse is reported as the generic "Enter
//! a valid value.", a `FromStr` error being written for whoever wrote the call
//! rather than for whoever filled in the form.
//!
//! For a type you own, `#[derive(FormValue)]` with `#[value(from_str)]` says it
//! once, on the type, instead of at every field that mentions it — and, since
//! converting itself asks nothing of its shape, that is also how a several-field
//! struct or an enum becomes one value.
//!
//! [`FromStr`]: std::str::FromStr
//! [`Display`]: std::fmt::Display
//!
//! # Localisation
//!
//! Any string a person reads — a label, help text, a placeholder, a legend, an
//! option's label, an error message — can be written as `t("key")` instead of
//! as text. The crate resolves nothing itself; hand [`FormView::localize`] a
//! lookup, or read the `…_key` companion field and translate in the template.
//!
//! ```
//! use html_form::Form;
//!
//! #[derive(Form)]
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
//! use html_form::{Form, Outcome, Text};
//!
//! #[derive(Form)]
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
//! use html_form::Form;
//!
//! #[derive(Form)]
//! struct Address {
//!     #[field(label = "Street")]
//!     street: String,
//!     #[field(label = "Postcode", pattern = r"\d{4,5}")]
//!     postcode: String,
//! }
//!
//! #[derive(Form)]
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
//! A form may be generic, which is what makes a *wrapper* possible: [`SPEC`](Form::SPEC) is
//! an associated constant, so `<T as Form>::SPEC` is resolved once per
//! instantiation, and the bounds each field implies are added by the derive.
//! What a flatten splices in is the sub-form's fields — its `action`, `method`
//! and submit label describe its own `<form>` element, so a wrapper declares the
//! ones it wants.
//!
//! ```
//! use html_form::Form;
//!
//! #[derive(Form)]
//! #[form(method = "post")]
//! struct WithCsrf<T> {
//!     #[field(type = "hidden", default = fresh_token)]
//!     csrf_token: String,
//!
//!     #[field(flatten)]
//!     inner: T,
//! }
//!
//! #[derive(Form)]
//! struct Signup {
//!     #[field(type = "email")]
//!     email: String,
//! }
//!
//! fn fresh_token() -> String {
//!     // A real one comes from a CSPRNG, and is remembered in the session.
//!     "3f9c…".to_owned()
//! }
//!
//! let view = WithCsrf::<Signup>::render();
//! let names: Vec<&str> = view.fields.iter().map(|f| f.name.as_ref()).collect();
//! assert_eq!(names, ["csrf_token", "email"]);
//! assert_eq!(view.field("csrf_token").unwrap().value.as_deref(), Some("3f9c…"));
//! ```
//!
//! # Defaults the form produces itself
//!
//! `default = "…"` is a value written into the spec. `default = some_fn` is a
//! function called once per render, for the defaults a constant cannot hold: a
//! CSRF token, a nonce, today's date. It may return any string type.
//!
//! A generated default belongs to *rendering* alone. It is never a fallback
//! while parsing — if it were, a submission that left the CSRF token out would
//! arrive carrying a freshly minted, valid one.
//!
//! On a blank form it is the value, as any default is. Once there are values to
//! show — a submission being re-rendered, a record being edited — only a
//! **hidden** field is minted again: nobody typed it, so there is nothing of
//! the caller's to preserve, and echoing a rejected token back would leave the
//! retry failing exactly as the first attempt did. Every other control shows
//! what it was given, empty included. A form that filled a visible field in
//! would be putting a value the caller never had in front of the user, for them
//! to send back without noticing.
//!
//! # What the form's own functions are handed
//!
//! A token that has to match the session, a list of options only the database
//! knows, a check that depends on who is logged in: none of that fits in a
//! `const`. `#[form(context = …)]` declares a type the caller passes in at the
//! moment it renders or parses, and every function the form names is handed it.
//!
//! Declaring a context changes the *names* of the calls, not their meaning:
//! [`render`](Form::render) becomes
//! [`render_with_context`](Form::render_with_context),
//! [`from_values`](Form::from_values) becomes
//! [`from_values_with_context`](Form::from_values_with_context), and so on
//! through the pairs. Both halves are on [`Form`] itself; the short one asks
//! for `Context: EmptyContext`, which `()` — what a form that declares no
//! context gets — is.
//!
//! ```
//! use html_form::{Form, Text};
//!
//! /// Whatever a handler already has: the session, a connection, the clock.
//! struct Session {
//!     csrf: String,
//! }
//!
//! #[derive(Form, Debug)]
//! #[form(method = "post", context = Session)]
//! struct Comment {
//!     #[field(type = "hidden", default = issued_token, validate = is_our_token)]
//!     csrf_token: String,
//!
//!     #[field(type = "textarea", label = "Comment", maxlength = 2000)]
//!     body: String,
//! }
//!
//! /// A default may take the context, and is called once per render.
//! fn issued_token(session: &Session) -> String {
//!     session.csrf.clone()
//! }
//!
//! /// So may a validator, after the value it is checking.
//! fn is_our_token(submitted: &String, session: &Session) -> Result<(), Text> {
//!     match *submitted == session.csrf {
//!         true => Ok(()),
//!         false => Err(Text::key("form.csrf.rejected")),
//!     }
//! }
//!
//! let session = Session { csrf: "3f9c…".to_owned() };
//!
//! // The hidden field is filled in from the session that will check it.
//! let view = Comment::render_with_context(&session);
//! assert_eq!(view.field("csrf_token").unwrap().value.as_deref(), Some("3f9c…"));
//!
//! let comment = Comment::from_urlencoded_with_context(
//!     "csrf_token=3f9c…&body=Nice+post",
//!     &session,
//! )
//! .unwrap();
//! assert_eq!(comment.body, "Nice post");
//!
//! // Somebody else's token is rejected by the check the markup could not make.
//! let errors =
//!     Comment::from_urlencoded_with_context("csrf_token=forged&body=x", &session).unwrap_err();
//! assert_eq!(
//!     errors.field("csrf_token").next().unwrap().code(),
//!     Some("form.csrf.rejected")
//! );
//! ```
//!
//! Either arity will do wherever a function is named: `fn() -> String` and
//! `fn(&Session) -> String`, `fn(&T) -> bool` and `fn(&T, &Session) -> bool`.
//! Which one was written is read off the function itself, so a form that gains
//! a context does not have to rewrite the checks that never needed one. See
//! [`DefaultSource`], [`FieldValidator`] and [`FormValidator`].
//!
//! A flattened sub-form is parsed and rendered with the enclosing form's
//! context. [`Provides`] is what lets the two differ — most usefully, what lets
//! a form written without a context be reused inside one that has a context.
//! `examples/csrf.rs` puts the lot together.
//!
//! # Attributes the crate has no opinion about
//!
//! `attr(...)` carries anything else the markup needs — `data-*`, `hx-*` — onto
//! the `<form>` or onto one control. It never takes part in validation.
//!
//! ```
//! use html_form::Form;
//!
//! #[derive(Form)]
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
//! Nor is it tied to a submission being a form. [`Values`] is `Serialize` and
//! `Deserialize`, so a JSON body is a submission too — the same struct, the
//! same checks, the same errors, which already serialise for a client that
//! sent JSON in the first place.
//!
//! ```
//! # use html_form::{Form, Values};
//! # #[derive(Form)]
//! # struct Signup {
//! #     #[field(type = "email")]
//! #     email: String,
//! #     age: Option<u32>,
//! # }
//! let values: Values = serde_json::from_str(r#"{"email": "ada@example.com", "age": 36}"#)?;
//! let signup = Signup::from_values(&values)?;
//! assert_eq!(signup.age, Some(36));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! A number or a boolean is read as the string a form would have submitted, a
//! list is a name submitted repeatedly, and `null` is a name not submitted at
//! all.
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
//! A spec is `const`: [`Form::SPEC`] is one const-evaluated value, flattened
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
//!
//! A context costs a reference passed down the walk, and nothing else. The
//! defaults a form generates are the one thing that has to be produced before
//! the view is built, and [`Form::GENERATES_DEFAULTS`] — const-evaluated
//! through the whole flatten tree — is what keeps a form that declares none from
//! paying for the mechanism at all.

#[cfg(feature = "axum")]
mod axum;
mod context;
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
pub use context::{DefaultSource, EmptyContext, Provides, WithContext, WithoutContext};
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
pub use validate::{FieldValidation, FieldValidator, FormValidation, FormValidator};
pub use value::FormValue;
pub use values::Values;
pub use view::{AttrView, ChoiceView, FieldView, FormView};

#[doc(hidden)]
pub use runtime::__private;

#[cfg(feature = "derive")]
pub use html_form_derive::{Form, FormChoice, FormValue};

/// Everything needed to declare and use a form.
pub mod prelude {
    // `Form` and `FormValue` each name both the trait and, with the `derive`
    // feature, the macro — one `use` brings whichever of the two is meant.
    #[cfg(feature = "derive")]
    pub use crate::{Choice, FormChoice};
    pub use crate::{FieldKind, Form, FormErrors, FormValue, FormView, Outcome, Values};
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
/// Implemented by `#[derive(Form)]`. The members without a body are what the
/// derive generates; everything else is provided.
///
/// Every method that renders or parses takes the form's [`Context`](Self::Context)
/// and says so in its name. Each one has a twin without the argument or the
/// suffix — `Signup::render()` rather than `Signup::render_with_context(&())` —
/// available where the context is an [`EmptyContext`], which is every form that
/// has not declared one.
pub trait Form: Sized {
    /// What this form's own functions are handed besides the value they are
    /// looking at: the session, a database handle, the request's locale —
    /// whatever `#[field(default = ...)]` and `validate = ...` need to know and
    /// a `const` spec cannot hold.
    ///
    /// `()` for a form that needs nothing, which is what the derive assumes
    /// until `#[form(context = ...)]` says otherwise. [`DefaultSource`],
    /// [`FieldValidator`] and [`FormValidator`] are how a function reaches it;
    /// [`Provides`] is what lets a form with a context flatten one without.
    type Context;

    /// The static description of this form.
    ///
    /// A constant rather than a function: the derive builds it entirely at
    /// const-evaluation time, so a flattened sub-form is a reference the
    /// compiler has already resolved rather than a call made at render time,
    /// and `SPEC` can be read from any `const` context.
    const SPEC: &'static FormSpec;

    /// Whether this form, or anything it flattens, has a default it produces at
    /// render time rather than one written into the spec.
    ///
    /// Const-evaluated through the whole flatten tree, so a form that has none
    /// — nearly every form — pays nothing for the mechanism: the walk in
    /// [`generate_defaults`](Self::generate_defaults) is never made.
    const GENERATES_DEFAULTS: bool = false;

    /// Parse the form out of `ctx`, honouring the flatten prefix in scope.
    ///
    /// Returns `None` when a value could not be produced. Errors are pushed
    /// into the context rather than returned, so parsing always visits every
    /// field.
    fn parse_in(ctx: &mut ParseCtx<'_, Self::Context>) -> Option<Self>;

    /// Write this value back out as raw submitted values, so an existing record
    /// can be rendered into the form it came from.
    fn fill_in(&self, values: &mut Values, prefix: &str);

    /// Produce the defaults this form makes afresh for one render — every
    /// `#[field(default = path)]` — under fully-qualified field names.
    ///
    /// Rendering is the whole of it. A generated default never stands in while
    /// *parsing*: if it did, a submission that left the CSRF token out would
    /// arrive carrying a freshly minted, valid one.
    fn generate_defaults(values: &mut Values, prefix: &str, context: &Self::Context) {
        let _ = (values, prefix, context);
    }

    /// [`Form::SPEC`], for call sites that would rather not name the type.
    fn spec() -> &'static FormSpec {
        Self::SPEC
    }

    /// What this form generates for one render, ready to be handed to
    /// [`FormView::build`] — `None`, and unvisited, for the forms that declare
    /// no generated default at all.
    fn defaults_with_context(context: &Self::Context) -> Option<Values> {
        Self::GENERATES_DEFAULTS.then(|| {
            let mut values = Values::new();
            Self::generate_defaults(&mut values, "", context);
            values
        })
    }

    /// Parse and validate a submission.
    fn from_values_with_context(
        values: &Values,
        context: &Self::Context,
    ) -> Result<Self, FormErrors> {
        let mut ctx = ParseCtx::new(values, context);
        let parsed = Self::parse_in(&mut ctx);
        let errors = ctx.into_errors();
        match parsed {
            Some(value) if errors.is_empty() => Ok(value),
            _ => Err(errors),
        }
    }

    /// [`from_values_with_context`](Form::from_values_with_context), for a
    /// form with nothing to be told.
    fn from_values(values: &Values) -> Result<Self, FormErrors>
    where
        Self::Context: EmptyContext,
    {
        Self::from_values_with_context(values, empty::<Self>())
    }

    /// Parse and validate an `application/x-www-form-urlencoded` body or query
    /// string.
    fn from_urlencoded_with_context(
        body: &str,
        context: &Self::Context,
    ) -> Result<Self, FormErrors> {
        Self::from_values_with_context(&Values::parse(body), context)
    }

    /// [`from_urlencoded_with_context`](Form::from_urlencoded_with_context),
    /// for a form with nothing to be told.
    fn from_urlencoded(body: &str) -> Result<Self, FormErrors>
    where
        Self::Context: EmptyContext,
    {
        Self::from_urlencoded_with_context(body, empty::<Self>())
    }

    /// Parse a submission, and on failure build the form to show again.
    fn submit_with_context(values: &Values, context: &Self::Context) -> Outcome<Self> {
        match Self::from_values_with_context(values, context) {
            Ok(value) => Outcome::Valid(value),
            Err(errors) => {
                let view = Box::new(Self::render_submitted_with_context(
                    values, &errors, context,
                ));
                Outcome::Invalid { errors, view }
            }
        }
    }

    /// [`submit_with_context`](Form::submit_with_context), for a form with
    /// nothing to be told.
    fn submit(values: &Values) -> Outcome<Self>
    where
        Self::Context: EmptyContext,
    {
        Self::submit_with_context(values, empty::<Self>())
    }

    /// [`Form::submit_with_context`] straight from a request body.
    fn submit_urlencoded_with_context(body: &str, context: &Self::Context) -> Outcome<Self> {
        Self::submit_with_context(&Values::parse(body), context)
    }

    /// [`Form::submit`] straight from a request body.
    fn submit_urlencoded(body: &str) -> Outcome<Self>
    where
        Self::Context: EmptyContext,
    {
        Self::submit_urlencoded_with_context(body, empty::<Self>())
    }

    /// A blank form, with each field showing its declared default.
    fn render_with_context(context: &Self::Context) -> FormView {
        FormView::build(
            Self::SPEC,
            None,
            Self::defaults_with_context(context).as_ref(),
            &FormErrors::new(),
        )
    }

    /// A blank form, for a form with nothing to be told.
    fn render() -> FormView
    where
        Self::Context: EmptyContext,
    {
        Self::render_with_context(empty::<Self>())
    }

    /// A blank form with every i18n key already resolved.
    ///
    /// The general form is [`FormView::localized`], which does the same to a
    /// view from any of the other constructors:
    /// `article.render_filled().localized(&translate)`.
    fn render_localized_with_context<S, F>(translate: F, context: &Self::Context) -> FormView
    where
        F: Fn(&str) -> Option<S>,
        S: Into<std::borrow::Cow<'static, str>>,
    {
        Self::render_with_context(context).localized(translate)
    }

    /// [`render_localized_with_context`](Form::render_localized_with_context),
    /// for a form with nothing to be told.
    fn render_localized<S, F>(translate: F) -> FormView
    where
        F: Fn(&str) -> Option<S>,
        S: Into<std::borrow::Cow<'static, str>>,
        Self::Context: EmptyContext,
    {
        Self::render_localized_with_context(translate, empty::<Self>())
    }

    /// The form as it should be shown after a submission: the submitted values
    /// with the errors attached to their fields.
    fn render_submitted_with_context(
        values: &Values,
        errors: &FormErrors,
        context: &Self::Context,
    ) -> FormView {
        FormView::build(
            Self::SPEC,
            Some(values),
            Self::defaults_with_context(context).as_ref(),
            errors,
        )
    }

    /// [`render_submitted_with_context`](Form::render_submitted_with_context),
    /// for a form with nothing to be told.
    fn render_submitted(values: &Values, errors: &FormErrors) -> FormView
    where
        Self::Context: EmptyContext,
    {
        Self::render_submitted_with_context(values, errors, empty::<Self>())
    }

    /// The form filled in from an existing value — an edit form.
    fn render_filled_with_context(&self, context: &Self::Context) -> FormView {
        FormView::build(
            Self::SPEC,
            Some(&self.to_values()),
            Self::defaults_with_context(context).as_ref(),
            &FormErrors::new(),
        )
    }

    /// [`render_filled_with_context`](Form::render_filled_with_context), for
    /// a form with nothing to be told.
    fn render_filled(&self) -> FormView
    where
        Self::Context: EmptyContext,
    {
        self.render_filled_with_context(empty::<Self>())
    }

    /// This value as raw submitted values.
    fn to_values(&self) -> Values {
        let mut values = Values::new();
        self.fill_in(&mut values, "");
        values
    }
}

/// The context of a form that has nothing to be told, named once so that each
/// short method above is its `…_with_context` counterpart and nothing else.
fn empty<T: Form>() -> &'static T::Context
where
    T::Context: EmptyContext,
{
    <T::Context as EmptyContext>::EMPTY
}
