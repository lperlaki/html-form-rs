//! One form inside another: `WithCsrf<T>` gives any form a hidden token.
//!
//! Four things meet here:
//!
//! * a **generic form**. `WithCsrf<T>` flattens `T`, so the token is the only
//!   field it adds, and the wrapped form's names stay as they were.
//! * a **context**. `#[form(context = Session)]` says what the caller passes in
//!   at the moment it renders or parses. That is how the token reaches the two
//!   functions below with no thread-local in sight.
//! * a **generated default**. `default = issued_token` names a function the
//!   crate runs once per render, not a value written into the spec.
//! * a **custom validation**. `validate = belongs_to_session` checks the token
//!   that comes back. The markup could not ask the browser to do that.
//!
//! Run with: `cargo run --example csrf`

use html_form::{Form, Outcome, Provides, Text};

/// A stand-in for a session, which a real application reaches through a cookie.
/// Whatever a handler already has can be a context: a database handle, the
/// user's locale, or the clock.
struct Session {
    csrf: String,
}

/// Somebody wrote `Signup` below without a context, and it should not have to
/// gain one to go inside a wrapper. `Session` therefore says what it gives a
/// sub-form that asks for nothing: nothing.
impl Provides<()> for Session {
    fn provide(&self) -> &() {
        &()
    }
}

/// Any form, plus the hidden field that says the submission came from a page we
/// served.
///
/// From the flatten, the derive works out that `T` has to be a `Form`, and that
/// a `Session` has to supply whatever context `T` asks for. The struct
/// therefore needs no bound written on it. The flatten brings in the *fields*
/// of `T`. Its `action`, `method` and submit label belong to its own `<form>`
/// element, so declare here anything the wrapper should render.
#[derive(Form, Debug)]
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

/// The form the wrapper protects. It knows nothing about any of this, and it
/// asks for no context of its own.
#[derive(Form, Debug)]
struct Signup {
    #[field(type = "email", label = "Email address")]
    email: String,

    #[field(type = "password", label = "Password", minlength = 12)]
    password: String,
}

/// The token this session shows. `default = issued_token` calls this once per
/// render, with the context the render received.
fn issued_token(session: &Session) -> String {
    session.csrf.clone()
}

/// The check the markup cannot express: the token that came back has to be the
/// one this session issued.
///
/// This returns a [`Text::key`] in place of a message. The page therefore
/// translates the rejection with everything else, and the error carries
/// `form.csrf.rejected` as its code for a caller that would rather match on it.
// A field validator receives the field's own Rust type, so a `String` field
// means `&String` here, however well a `&str` would do. The context comes next.
#[allow(clippy::ptr_arg)]
fn belongs_to_session(submitted: &String, session: &Session) -> Result<(), Text> {
    match constant_time_eq(&session.csrf, submitted) {
        true => Ok(()),
        false => Err(Text::key("form.csrf.rejected")),
    }
}

/// A stand-in for a real generator. Draw from a CSPRNG, such as `getrandom` or
/// your session library's own token, and not from the clock.
fn mint_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after 1970")
        .as_nanos();
    format!("{nanos:032x}")
}

/// A comparison that does not reveal where two tokens first differ.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ─── What that adds up to ─────────────────────────────────────────────────────

fn main() {
    // What a handler already has, and passes to every call below.
    let session = Session { csrf: mint_token() };

    // The render: one hidden input, then the wrapped form's own fields.
    let view = WithCsrf::<Signup>::render_with_context(&session);
    println!("{}\n", view.to_html());

    let token = view
        .field("csrf_token")
        .and_then(|field| field.value.clone())
        .expect("the generated default fills the hidden field in");
    assert_eq!(token, session.csrf);

    let credentials = "email=ada@example.com&password=correct-horse-battery";

    // A submission that carries the token back passes, and `T` comes out of it
    // as the form it always was.
    let Outcome::Valid(form) = WithCsrf::<Signup>::submit_urlencoded_with_context(
        &format!("csrf_token={token}&{credentials}"),
        &session,
    ) else {
        panic!("the session's own token should have been accepted");
    };
    assert_eq!(form.inner.email, "ada@example.com");

    // The validator rejects somebody else's token, and the message is the key
    // it returned.
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
    // The re-render carries a token that works. It does not send back the one
    // that just failed, which would make the user's retry fail the same way.
    assert_eq!(
        view.field("csrf_token").unwrap().value.as_deref(),
        Some(&*token)
    );
    // What the user typed is still there, as after any other rejection.
    assert_eq!(
        view.field("email").unwrap().value.as_deref(),
        Some("ada@example.com")
    );

    // Leaving the field out fails too. A generated default belongs to render
    // time alone. If it stood in while parsing, a submission with no token
    // would arrive with a fresh and valid one.
    let errors =
        WithCsrf::<Signup>::from_urlencoded_with_context(credentials, &session).unwrap_err();
    assert!(errors.has_field("csrf_token"));

    // `FormErrors` prints as `field: message`, which suits a log line.
    println!("{errors}");
}
