//! One form wrapping another: `WithCsrf<T>` gives any form a hidden token.
//!
//! Four things meet here:
//!
//! * a **generic form** — `WithCsrf<T>` flattens `T`, so the token is the only
//!   field it adds and the wrapped form's names are untouched;
//! * a **context** — `#[form(context = Session)]` says what the caller hands in
//!   at the moment it renders or parses, which is how the token reaches the two
//!   functions below without a thread-local in sight;
//! * a **generated default** — `default = issued_token` names a function, run
//!   once per render, rather than a value written into the spec;
//! * a **custom validation** — `validate = belongs_to_session` re-checks the
//!   token that comes back, which is not something the markup could have asked
//!   the browser to do.
//!
//! Run with: `cargo run --example csrf`

use web_form::{Outcome, Provides, Text, WebForm};

/// Stand-in for a session, which a real application reaches through a cookie.
/// Whatever a handler already has can be a context: a database handle, the
/// user's locale, the clock.
struct Session {
    csrf: String,
}

/// `Signup` below was written without a context and should not have to gain one
/// to be wrapped, so `Session` says what it hands a sub-form that asks for
/// nothing: nothing.
impl Provides<()> for Session {
    fn provide(&self) -> &() {
        &()
    }
}

/// Any form, plus the hidden field that says the submission came from a page we
/// served.
///
/// The derive works out that `T` has to be a `WebForm` from the flatten, and
/// that a `Session` has to be able to supply whatever context `T` asks for, so
/// the struct needs no bound written on it. What the flatten splices in is `T`'s
/// *fields* — its `action`, `method` and submit label belong to its own `<form>`
/// element, so anything the wrapper should render has to be declared here.
#[derive(WebForm, Debug)]
#[form(method = "post", context = Session)]
struct WithCsrf<T> {
    #[field(
        type = "hidden",
        default = issued_token,
        validate = belongs_to_session
    )]
    csrf_token: String,

    #[field(flatten)]
    inner: T,
}

/// The form being protected — it knows nothing about any of this, and asks for
/// no context of its own.
#[derive(WebForm, Debug)]
struct Signup {
    #[field(type = "email", label = "Email address")]
    email: String,

    #[field(type = "password", label = "Password", minlength = 12)]
    password: String,
}

/// The token this session is showing. `default = issued_token` calls this once
/// per render, with the context the render was given.
fn issued_token(session: &Session) -> String {
    session.csrf.clone()
}

/// The check the markup cannot express: the token that came back has to be the
/// one this session was issued.
///
/// Returning a [`Text::key`] rather than a message means the rejection is
/// translated with everything else on the page, and that the error carries
/// `form.csrf.rejected` as its code for a caller that would rather match on it.
// A field validator is handed the field's own Rust type, so a `String` field
// means `&String` here however much a `&str` would do. The context follows it.
#[allow(clippy::ptr_arg)]
fn belongs_to_session(submitted: &String, session: &Session) -> Result<(), Text> {
    match constant_time_eq(&session.csrf, submitted) {
        true => Ok(()),
        false => Err(Text::key("form.csrf.rejected")),
    }
}

/// Stands in for a real generator: draw from a CSPRNG (`getrandom`, or your
/// session library's own token) rather than from the clock.
fn mint_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after 1970")
        .as_nanos();
    format!("{nanos:032x}")
}

/// Comparison that does not give away where two tokens first differ.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ─── What that adds up to ─────────────────────────────────────────────────────

fn main() {
    // What a handler would have already, and hands to every call below.
    let session = Session { csrf: mint_token() };

    // Rendering: one hidden input, then the wrapped form's own fields.
    let view = WithCsrf::<Signup>::render_with_context(&session);
    println!("{}\n", view.to_html());

    let token = view
        .field("csrf_token")
        .and_then(|field| field.value.clone())
        .expect("the generated default fills the hidden field in");
    assert_eq!(token, session.csrf);

    let credentials = "email=ada@example.com&password=correct-horse-battery";

    // A submission carrying the token back is accepted, and `T` comes out of it
    // as the form it always was.
    let Outcome::Valid(form) = WithCsrf::<Signup>::submit_urlencoded_with_context(
        &format!("csrf_token={token}&{credentials}"),
        &session,
    ) else {
        panic!("the session's own token should have been accepted");
    };
    assert_eq!(form.inner.email, "ada@example.com");

    // Somebody else's token is rejected by the validator, and the message is
    // the key it returned.
    let Outcome::Invalid { errors, view } = WithCsrf::<Signup>::submit_urlencoded_with_context(
        &format!("csrf_token=deadbeef&{credentials}"),
        &session,
    ) else {
        panic!("a forged token should have been rejected");
    };
    assert_eq!(
        errors.field("csrf_token").next().unwrap().code(),
        Some("form.csrf.rejected")
    );
    // The re-render carries a token that works, rather than echoing the one
    // that just failed — otherwise the user's retry would fail identically.
    assert_eq!(
        view.field("csrf_token").unwrap().value.as_deref(),
        Some(&*token)
    );
    // What the user typed is still there, as it is after any other rejection.
    assert_eq!(
        view.field("email").unwrap().value.as_deref(),
        Some("ada@example.com")
    );

    // Leaving the field out entirely fails as well. A generated default is a
    // render-time thing only: if it stood in while parsing, a submission with no
    // token at all would arrive carrying a freshly minted, valid one.
    let errors =
        WithCsrf::<Signup>::from_urlencoded_with_context(credentials, &session).unwrap_err();
    assert!(errors.has_field("csrf_token"));

    // `FormErrors` prints as `field: message`, which is enough for a log line.
    println!("{errors}");
}
