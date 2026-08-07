#![cfg_attr(docsrs, feature(doc_cfg))]

//! Declarative HTML forms.
//!
//! <div class="warning">
//!
//! **This crate is AI generated.** A language model wrote the code, the tests
//! and this documentation. A person reviewed the result, but no such review
//! catches everything. Read the source before you depend on it, and treat every
//! claim here as something to verify rather than something to trust.
//!
//! </div>
//!
//! One struct describes a form. From it you get:
//!
//! * a **render format**. [`FormView`] is a flat, serializable description of
//!   the form, ready for MiniJinja, Askama or the built-in HTML renderer.
//! * a **parser**. The crate builds the same struct again from an
//!   `application/x-www-form-urlencoded` submission, and checks every attribute
//!   again on the server.
//! * an **error render**. When validation fails, you get the same
//!   [`FormView`]. It carries the values the user typed and the messages for
//!   each field.
//!
//! Validation does not stop at the first problem. The crate tries every field
//! and collects every failure into one [`FormErrors`].
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
//!         // A bad address, a short password, an age below the minimum.
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
//! `validate = ...` names a function. The crate runs it after every check the
//! spec could make: per field with `#[field(...)]`, or once over the whole
//! struct with `#[form(...)]`. A predicate is enough when the built-in message
//! will do. Return a `Result` to say more. See [`FieldValidation`] and
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
//!         // Attach it to the field the user can correct, not to the form.
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
//! A check that the whole application makes of a value does not belong to one
//! field of one form. `#[derive(FormValue)]` puts the check on the type
//! instead. The wrapped type does the conversion. `#[value(...)]` says what the
//! wrapper adds: the control, the constraints, the default, and the check no
//! markup could make. A form that uses the type then says only where it goes.
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
//! // The control, and every constraint on it, came from the type.
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
//! anything that describes the *field*, such as a label or a placeholder. The
//! same type is a "Work email" on one form and a "Recipient" on the next. See
//! [`FormValue`] for the trait, and the derive for every `#[value(...)]` key.
//!
//! # A type from a crate that has never heard of this one
//!
//! `#[field(from_str)]` converts a field with the type's own [`FromStr`] and
//! [`Display`]. A `Uuid`, a `NaiveDate` or a `Decimal` is therefore a field
//! with no impl written and no newtype wrapped around it.
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
//! // The crate makes every check the spec could make first. Those checks say
//! // more than a conversion could: this is the date control's format check.
//! let errors = Booking::from_urlencoded("day=whenever").unwrap_err();
//! assert_eq!(
//!     errors.field("day").next().unwrap().message.as_str(),
//!     "Enter a date as YYYY-MM-DD."
//! );
//! ```
//!
//! It applies to the field's own type, `Option<T>` and `Vec<T>` included, and
//! changes nothing else. A `validate` function still sees the type the field
//! was written as, and `Display` writes the value out again for an edit form.
//! What it cannot do is imply a control, because a foreign type has no
//! `CONTROL` to give one. The field therefore renders as text until
//! `type = "..."` says otherwise. And a value that will not parse gets the
//! general message "Enter a valid value." A `FromStr` error speaks to whoever
//! wrote the call, not to whoever filled in the form.
//!
//! For a type you own, `#[derive(FormValue)]` with `#[value(from_str)]` says
//! this once, on the type, in place of at every field that names it.
//! Self-conversion asks nothing of a type's shape, so that is also how a
//! several-field struct or an enum becomes one value.
//!
//! [`FromStr`]: std::str::FromStr
//! [`Display`]: std::fmt::Display
//!
//! # Localization
//!
//! You can write any string a person reads as `t("key")` in place of the text.
//! That covers a label, help text, a placeholder, a legend, an option's label
//! and an error message. The crate resolves no key itself. Give
//! [`FormView::localize`] a lookup, or read the `…_key` companion field and
//! translate in the template.
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
//! The crate leaves a key that nothing knows in place. The view then shows the
//! key itself, which is a visible bug rather than a silently blank label.
//!
//! A `validate = ...` function localizes the same way. Return a [`Text::key`],
//! and the crate resolves the message with everything else. The key is also the
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
//! The messages of the *built-in* checks are English, and carry no key on
//! purpose. Every [`ErrorKind`] carries the constraint the value broke, so a
//! caller that needs a translation matches on the kind and writes its own text.
//! There is no key to guess at, and no message table to keep in step.
//!
//! # Reuse: flattening one form into another
//!
//! `#[field(flatten)]` puts one form inside another. Give the flatten a
//! `prefix` to use the same sub-form more than once.
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
//! A form may be generic, which is what makes a *wrapper* possible.
//! [`SPEC`](Form::SPEC) is an associated constant, so the compiler resolves
//! `<T as Form>::SPEC` once per instantiation, and the derive adds the bounds
//! each field implies. A flatten brings in the sub-form's fields alone. Its
//! `action`, `method` and submit label describe its own `<form>` element, so a
//! wrapper declares the ones it wants.
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
//!     // A real one comes from a CSPRNG, and the session remembers it.
//!     "3f9c…".to_owned()
//! }
//!
//! let view = WithCsrf::<Signup>::render();
//! let names: Vec<&str> = view.fields.iter().map(|f| f.name.as_ref()).collect();
//! assert_eq!(names, ["csrf_token", "email"]);
//! assert_eq!(view.field("csrf_token").unwrap().value.as_deref(), Some("3f9c…"));
//! ```
//!
//! # Defaults
//!
//! A default is what a **render** starts a field with, and that is the whole of
//! what it is. Parsing never reaches for one. A submission that left a field
//! out left it out, and a form that filled the gap in would be answering its
//! own question: a request carrying no CSRF token at all would arrive holding a
//! valid one.
//!
//! There are three ways to write one, and one slot in the spec holds any of
//! them:
//!
//! * `default = "…"` is a value written into the spec.
//! * `default`, with nothing after it, is the field type's own
//!   `Default::default()`.
//! * `default = some_fn` is a function the crate calls once per render, for the
//!   defaults a constant cannot hold: a CSRF token, a nonce, today's date.
//!
//! The last two hand back the field's own type, or anything that converts into
//! it, and the crate writes the value out the way the field writes out every
//! other value it holds.
//!
//! ```
//! use html_form::Form;
//!
//! #[derive(Form, Debug)]
//! struct Booking {
//!     #[field(label = "Seats", default = usual_party)]
//!     seats: u32,
//!     #[field(label = "Reference", default = "walk-in")]
//!     reference: String,
//!     #[field(label = "Notes", default)]
//!     notes: String,
//! }
//!
//! fn usual_party() -> u32 {
//!     2
//! }
//!
//! let view = Booking::render();
//! assert_eq!(view.field("seats").unwrap().value.as_deref(), Some("2"));
//! assert_eq!(view.field("reference").unwrap().value.as_deref(), Some("walk-in"));
//! assert_eq!(view.field("notes").unwrap().value.as_deref(), Some(""));
//!
//! // And a submission is read as it arrived, defaults or no defaults.
//! assert!(Booking::from_urlencoded("reference=r-1").is_err());
//! ```
//!
//! On a blank form the default is the value, whichever kind it is. Once there
//! are values to show, such as a submission to re-render or a record to edit,
//! the crate mints only a **hidden** field whose default it *generates*. Nobody
//! typed that field, so there is nothing of the caller's to keep, and a rejected
//! token sent back would make the retry fail exactly as the first attempt did.
//! Every other control shows what it received, an empty value included. A form
//! that filled a visible field in would put a value the caller never had in
//! front of the user. The user could then send it back without noticing.
//!
//! Where a render shows what it received, the crate does not call the generator
//! at all. A function that hands out a token the server also records therefore
//! runs once for each render that uses its value, and not once more.
//!
//! # Fields that reset
//!
//! `#[field(reset)]` says that of any field. The field shows its default on
//! every render — a blank form, a submission to re-render, a record to edit —
//! and never what came in. Declare it on a field the form owns rather than the
//! user: a token in a control the user sees, a password box that must not come
//! back filled in, a literal the retry has to start over from.
//!
//! A **hidden** field whose default the form *generates* resets without being
//! told to, because it can hold nothing a caller would miss. That is the one
//! case the crate decides for you, it decides it where the form is declared,
//! and `#[field(reset = false)]` turns it off. Everything the renderer needs is
//! then in one place: [`FieldSpec::reset`].
//!
//! ```
//! use html_form::Form;
//!
//! #[derive(Form)]
//! struct ChangePassword {
//!     #[field(type = "password", reset, minlength = 12)]
//!     new_password: String,
//! }
//!
//! // The submission was rejected, and the box comes back empty all the same.
//! let outcome = ChangePassword::submit_urlencoded("new_password=short");
//! let view = outcome.view().unwrap();
//! assert_eq!(view.field("new_password").unwrap().value, None);
//! assert!(view.field("new_password").unwrap().has_errors);
//! ```
//!
//! The field still *parses* as any other does. Resetting is about what the next
//! render shows, so a validator sees the value the user sent, and the error it
//! reports stays on the field.
//!
//! # What the form's own functions receive
//!
//! A `const` cannot hold a token that has to match the session. Nor a list of
//! options only the database knows, nor a check that depends on who is logged
//! in. `#[form(context = …)]` declares a type. The caller passes it in at the
//! moment the form renders or parses, and every function the form names
//! receives it.
//!
//! A context changes the *names* of the calls, not their meaning.
//! [`render`](Form::render) becomes
//! [`render_with_context`](Form::render_with_context), and
//! [`from_values`](Form::from_values) becomes
//! [`from_values_with_context`](Form::from_values_with_context). The other
//! pairs work the same way. [`Form`] itself holds both halves. The short one
//! asks for `Context: EmptyContext`, which `()` satisfies. A form that declares
//! no context gets `()`.
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
//! /// A default may take the context. The crate calls it once per render.
//! fn issued_token(session: &Session) -> String {
//!     session.csrf.clone()
//! }
//!
//! /// So may a validator, after the value it checks.
//! fn is_our_token(submitted: &String, session: &Session) -> Result<(), Text> {
//!     match *submitted == session.csrf {
//!         true => Ok(()),
//!         false => Err(Text::key("form.csrf.rejected")),
//!     }
//! }
//!
//! let session = Session { csrf: "3f9c…".to_owned() };
//!
//! // The session that will check the hidden field also fills it in.
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
//! // The check the markup could not make rejects somebody else's token.
//! let errors =
//!     Comment::from_urlencoded_with_context("csrf_token=forged&body=x", &session).unwrap_err();
//! assert_eq!(
//!     errors.field("csrf_token").next().unwrap().code(),
//!     Some("form.csrf.rejected")
//! );
//! ```
//!
//! Either arity works wherever you name a function: `fn() -> String` and
//! `fn(&Session) -> String`, `fn(&T) -> bool` and `fn(&T, &Session) -> bool`.
//! The crate reads which one you wrote off the function itself. A form that
//! gains a context therefore need not rewrite the checks that never took one.
//! See [`DefaultSource`], [`FieldValidator`] and [`FormValidator`].
//!
//! The crate parses and renders a flattened sub-form with the enclosing form's
//! context. [`Provides`] lets the two differ. Most usefully, it lets you reuse
//! a form written without a context inside one that has a context.
//! `examples/csrf.rs` puts all of this together.
//!
//! # Attributes the crate has no opinion about
//!
//! `attr(...)` carries anything else the markup needs, such as `data-*` and
//! `hx-*`, onto the `<form>` or onto one control. It never takes part in
//! validation.
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
//! Write a dashed name as a string literal. The crate takes a bare word as
//! written, and a bare word with no value renders a boolean attribute. Naming
//! something the crate renders itself is a compile error, so
//! `attr("class" = "x")` points you at `#[field(class = "x")]` in place of
//! emitting a second `class`.
//!
//! # Rendering with a template engine
//!
//! [`FormView`] is `serde::Serialize`, so it goes straight into a MiniJinja
//! context. Its fields are public, so an Askama template can walk it directly.
//! See `examples/minijinja_render.rs`.
//!
//! The built-in renderer is the `html` feature, on by default. It covers
//! [`FormView::to_html`], [`FieldView::to_html`], the `Display` impls and
//! [`escape`]. A crate that renders through a template engine can turn the
//! feature off. Nothing else changes, [`FormView`] included.
//!
//! # Framework integration
//!
//! Nothing here depends on an HTTP stack. [`Values::from_pairs`] takes whatever
//! a framework's body parser produced. The `axum` feature adds two axum 0.8
//! extractors, and they differ in who answers a failed validation:
//! [`Outcome<T>`] hands it to the handler, and
//! [`axum::Form<T, R>`](axum::Form) rejects with the form again, rendered by
//! `R`. See the [`axum`] module and `examples/axum_signup.rs`.
//!
//! A submission also does not have to be a form. [`Values`] is `Serialize` and
//! `Deserialize`, so a JSON body is a submission too. You get the same struct,
//! the same checks and the same errors, which already serialize for a client
//! that sent JSON in the first place.
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
//! The crate reads a number or a boolean as the string a form would have
//! submitted. A list is a name submitted more than once, and `null` is a name
//! the client did not submit at all.
//!
//! # The two descriptions of a form
//!
//! [`FormSpec`] is the static one: what the derive emits, and what both the
//! renderer and the parser read. Each of its fields names a [`Control`], which
//! carries the attributes that control accepts and no others. There is no way
//! to give a date a `minlength` or a `<select>` an `accept`.
//!
//! [`FormView`] is the runtime one: flat, serializable, and carrying the things
//! a spec cannot know. That is what the user typed, what was wrong with it, and
//! the options that come from a database.
//!
//! [`Text`] is what every person-facing string in the spec is: literal text, or
//! an i18n key. Both reach the view, the key next to the string.
//!
//! # What rendering costs
//!
//! A spec is `const`. [`Form::SPEC`] is one const-evaluated value, flattened
//! sub-forms included, so the first render of a form builds nothing.
//!
//! Every string in a [`FormView`] is a `Cow<'static, str>`, and rendering
//! borrows from the spec where it can. A blank form allocates only what the
//! spec does not hold. That is the three ids derived from each field's name,
//! and the `Vec`s that hold the fields and their options. On a re-render it
//! also allocates the values the user submitted, which do not outlive the
//! request they came in on. Names, labels, help text, placeholders, options,
//! constraints and error messages stay borrowed. The owned half of each `Cow`
//! keeps the view an ordinary owned value that a handler can return. It also
//! lets `set_value`, `set_choices` and [`FormView::localize`] put runtime
//! strings in.
//!
//! Parsing works the same way. A field's name reaches [`FormErrors`] borrowed
//! from the spec, and only a `#[field(flatten, prefix = "…")]` makes a name
//! that the crate has to build.
//!
//! A context costs one pointer passed down the walk, and nothing else. A
//! default costs one indirect call, made where the render reads the value and
//! nowhere else. There is no pass over the form to collect defaults first, and
//! no `Values` built to carry them, so a form that declares none pays nothing
//! for the mechanism and a form that declares one pays for that one field. See
//! [`FieldDefault`].

#[cfg(feature = "axum")]
pub mod axum;
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
    Attr, Bounds, Choice, ChoiceStyle, ChooseControl, Control, Entry, FieldDefault, FieldSpec,
    FileControl, Flattened, FormEncType, FormMethod, FormSpec, Generate, NumberControl,
    NumberFormat, Provider, ResolvedField, TemporalControl, TemporalFormat, Text, TextControl,
    TextFormat, TextareaControl,
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
    // `Form` and `FormValue` each name the trait and, with the `derive`
    // feature, the macro. One `use` brings in whichever of the two you mean.
    #[cfg(feature = "derive")]
    pub use crate::{Choice, FormChoice};
    pub use crate::{FieldKind, Form, FormErrors, FormValue, FormView, Outcome, Values};
}

/// The result of handling a submission.
///
/// The invalid case carries the typed errors and a [`FormView`] ready to render
/// back to the user, with the values they entered.
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

    /// The parsed value, without the re-render.
    pub fn ok(self) -> Option<T> {
        match self {
            Outcome::Valid(value) => Some(value),
            Outcome::Invalid { .. } => None,
        }
    }

    /// The re-render, without the parsed value.
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
/// `#[derive(Form)]` implements it. The derive generates the members without a
/// body. The trait provides everything else.
///
/// Every method that renders or parses takes the form's
/// [`Context`](Self::Context) and says so in its name. Each one has a twin
/// without the argument and without the suffix, such as `Signup::render()` in
/// place of `Signup::render_with_context(&())`. The twin is available where the
/// context is an [`EmptyContext`], which covers every form that declares none.
pub trait Form: Sized {
    /// What this form's own functions receive besides the value they look at:
    /// the session, a database handle, the request's locale. It holds whatever
    /// `#[field(default = ...)]` and `validate = ...` need to know and a
    /// `const` spec cannot hold.
    ///
    /// It is `()` for a form that needs nothing, which is what the derive
    /// assumes until `#[form(context = ...)]` says otherwise.
    /// [`DefaultSource`], [`FieldValidator`] and [`FormValidator`] are how a
    /// function reaches it. [`Provides`] lets a form with a context flatten one
    /// without a context.
    type Context;

    /// The static description of this form.
    ///
    /// It is a constant, not a function. The derive builds all of it at
    /// const-evaluation time. A flattened sub-form is therefore a reference the
    /// compiler has already resolved, not a call made at render time, and any
    /// `const` context can read `SPEC`.
    const SPEC: &'static FormSpec;

    /// Parse the form out of `ctx`, and honor the flatten prefix in scope.
    ///
    /// Returns `None` when it could not produce a value. It pushes errors into
    /// the context in place of returning them, so parsing always visits every
    /// field.
    fn parse_in(ctx: &mut ParseCtx<'_, Self::Context>) -> Option<Self>;

    /// Write this value out again as raw submitted values, so you can render an
    /// existing record into the form it came from.
    fn fill_in(&self, values: &mut Values, prefix: &str);

    /// [`Form::SPEC`], for a call site that would rather not name the type.
    fn spec() -> &'static FormSpec {
        Self::SPEC
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
    /// form that needs no context.
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
    /// for a form that needs no context.
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

    /// [`submit_with_context`](Form::submit_with_context), for a form that
    /// needs no context.
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
        FormView::build::<Self>(None, &FormErrors::new(), context)
    }

    /// A blank form, for a form that needs no context.
    fn render() -> FormView
    where
        Self::Context: EmptyContext,
    {
        Self::render_with_context(empty::<Self>())
    }

    /// A blank form with every i18n key already resolved.
    ///
    /// [`FormView::localized`] is the general form. It does the same to a view
    /// from any of the other constructors:
    /// `article.render_filled().localized(&translate)`.
    fn render_localized_with_context<S, F>(translate: F, context: &Self::Context) -> FormView
    where
        F: Fn(&str) -> Option<S>,
        S: Into<std::borrow::Cow<'static, str>>,
    {
        Self::render_with_context(context).localized(translate)
    }

    /// [`render_localized_with_context`](Form::render_localized_with_context),
    /// for a form that needs no context.
    fn render_localized<S, F>(translate: F) -> FormView
    where
        F: Fn(&str) -> Option<S>,
        S: Into<std::borrow::Cow<'static, str>>,
        Self::Context: EmptyContext,
    {
        Self::render_localized_with_context(translate, empty::<Self>())
    }

    /// The form to show after a submission: the submitted values, with each
    /// error attached to its field.
    fn render_submitted_with_context(
        values: &Values,
        errors: &FormErrors,
        context: &Self::Context,
    ) -> FormView {
        FormView::build::<Self>(Some(values), errors, context)
    }

    /// [`render_submitted_with_context`](Form::render_submitted_with_context),
    /// for a form that needs no context.
    fn render_submitted(values: &Values, errors: &FormErrors) -> FormView
    where
        Self::Context: EmptyContext,
    {
        Self::render_submitted_with_context(values, errors, empty::<Self>())
    }

    /// The form filled in from an existing value — an edit form.
    fn render_filled_with_context(&self, context: &Self::Context) -> FormView {
        FormView::build::<Self>(Some(&self.to_values()), &FormErrors::new(), context)
    }

    /// [`render_filled_with_context`](Form::render_filled_with_context), for
    /// a form that needs no context.
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

/// The context of a form that needs no context, named once so that each short
/// method above is its `…_with_context` twin and nothing else.
fn empty<T: Form>() -> &'static T::Context
where
    T::Context: EmptyContext,
{
    <T::Context as EmptyContext>::EMPTY
}
