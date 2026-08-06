#![allow(dead_code)]

//! Forms that are handed something at the moment they are rendered or parsed:
//! what reaches a `default = ...` or `validate = ...` function, and what happens
//! at the seam where a form with a context flattens one without.

use web_form::{FormErrors, Outcome, Provides, Text, WebForm};

/// Whatever a handler already has.
struct Session {
    csrf: String,
    reserved: Vec<&'static str>,
}

impl Session {
    fn new() -> Self {
        Session {
            csrf: "tok-1".to_owned(),
            reserved: vec!["admin"],
        }
    }
}

/// A sub-form written without a context is flattened into one that has a
/// context by saying what that context hands down in its place.
impl Provides<()> for Session {
    fn provide(&self) -> &() {
        &()
    }
}

#[derive(WebForm, Debug)]
#[form(method = "post", context = Session, validate = one_admin_at_a_time)]
struct Signup {
    #[field(type = "hidden", default = issued_token, validate = belongs_to_session)]
    csrf_token: String,

    #[field(label = "Username", validate = is_available)]
    username: String,

    // The context is optional per function: this one never needed it and did
    // not have to grow an argument.
    #[field(label = "Seats", validate = is_even)]
    seats: u32,
}

fn issued_token(session: &Session) -> String {
    session.csrf.clone()
}

#[allow(clippy::ptr_arg)]
fn belongs_to_session(submitted: &String, session: &Session) -> Result<(), Text> {
    match *submitted == session.csrf {
        true => Ok(()),
        false => Err(Text::key("form.csrf.rejected")),
    }
}

#[allow(clippy::ptr_arg)]
fn is_available(name: &String, session: &Session) -> bool {
    !session.reserved.contains(&name.as_str())
}

fn is_even(seats: &u32) -> bool {
    seats.is_multiple_of(2)
}

/// A form-level validator takes the context after the assembled struct.
fn one_admin_at_a_time(form: &Signup, session: &Session) -> Result<(), FormErrors> {
    match session.reserved.contains(&form.username.as_str()) {
        true => Err("That account cannot sign up twice.".into()),
        false => Ok(()),
    }
}

#[test]
fn a_generated_default_is_produced_from_the_context() {
    let session = Session::new();
    let view = Signup::render_with_context(&session);

    assert_eq!(
        view.field("csrf_token").unwrap().value.as_deref(),
        Some("tok-1")
    );
}

#[test]
fn a_validator_sees_the_context_it_was_promised() {
    let session = Session::new();

    let form =
        Signup::from_urlencoded_with_context("csrf_token=tok-1&username=ada&seats=2", &session)
            .unwrap();
    assert_eq!(form.username, "ada");

    let errors =
        Signup::from_urlencoded_with_context("csrf_token=forged&username=admin&seats=3", &session)
            .unwrap_err();

    // The context-taking checks, the context-free one, and the form-level one
    // all ran, and every failure is in the same list.
    assert_eq!(
        errors.field("csrf_token").next().unwrap().code(),
        Some("form.csrf.rejected")
    );
    assert!(errors.has_field("username"));
    assert!(errors.has_field("seats"));
    assert_eq!(errors.form_errors().len(), 1);
}

#[test]
fn a_re_render_mints_the_hidden_default_again_rather_than_echoing_it() {
    let session = Session::new();
    let Outcome::Invalid { view, .. } =
        Signup::submit_urlencoded_with_context("csrf_token=forged&username=ada&seats=2", &session)
    else {
        panic!("a forged token should have been rejected");
    };

    assert_eq!(
        view.field("csrf_token").unwrap().value.as_deref(),
        Some("tok-1")
    );
    // What the user typed is echoed back, as always.
    assert_eq!(
        view.field("username").unwrap().value.as_deref(),
        Some("ada")
    );
}

// ─── Across the seam ──────────────────────────────────────────────────────────

/// Knows nothing of any context, and is not asked to.
#[derive(WebForm, Debug)]
struct Address {
    #[field(label = "Postcode", pattern = r"\d{5}")]
    postcode: String,
}

#[derive(WebForm, Debug)]
#[form(context = Session)]
struct Order {
    #[field(type = "hidden", default = issued_token)]
    csrf_token: String,

    #[field(flatten, prefix = "billing_")]
    billing: Address,
}

#[test]
fn a_context_free_sub_form_is_flattened_into_one_that_has_a_context() {
    let session = Session::new();

    let view = Order::render_with_context(&session);
    let names: Vec<&str> = view.fields.iter().map(|f| f.name.as_ref()).collect();
    assert_eq!(names, ["csrf_token", "billing_postcode"]);

    let order =
        Order::from_urlencoded_with_context("csrf_token=x&billing_postcode=12345", &session)
            .unwrap();
    assert_eq!(order.billing.postcode, "12345");

    // And the sub-form's own checks still run, under the prefixed name.
    let errors =
        Order::from_urlencoded_with_context("billing_postcode=nope", &session).unwrap_err();
    assert!(errors.has_field("billing_postcode"));
}

/// A wrapper is generic over what it wraps; the derive works out that a
/// `Session` has to be able to supply whatever context the wrapped form asks
/// for.
#[derive(WebForm, Debug)]
#[form(context = Session)]
struct WithToken<T> {
    #[field(type = "hidden", default = issued_token)]
    csrf_token: String,

    #[field(flatten)]
    inner: T,
}

#[test]
fn a_wrapper_renders_the_token_for_a_sub_form_of_either_kind() {
    let session = Session::new();

    let view = WithToken::<Address>::render_with_context(&session);
    let names: Vec<&str> = view.fields.iter().map(|f| f.name.as_ref()).collect();
    assert_eq!(names, ["csrf_token", "postcode"]);
    assert_eq!(
        view.field("csrf_token").unwrap().value.as_deref(),
        Some("tok-1")
    );

    // A sub-form that asks for the *same* context needs nothing extra either.
    // It generates a default of its own, which lands under the flatten's
    // prefix — as its name does, and as it has to: two fields submitted under
    // one name would reach the parse as one.
    #[derive(WebForm, Debug)]
    #[form(context = Session)]
    struct Checkout<T> {
        #[field(type = "hidden", default = issued_token)]
        csrf_token: String,

        #[field(flatten, prefix = "order_")]
        inner: T,
    }

    let view = Checkout::<Order>::render_with_context(&session);
    let names: Vec<&str> = view.fields.iter().map(|f| f.name.as_ref()).collect();
    assert_eq!(
        names,
        ["csrf_token", "order_csrf_token", "order_billing_postcode"]
    );
    assert_eq!(
        view.field("order_csrf_token").unwrap().value.as_deref(),
        Some("tok-1")
    );
}

// ─── The forms that ask for nothing ───────────────────────────────────────────

#[derive(WebForm, Debug)]
struct Search {
    #[field(type = "hidden", default = fixed_token)]
    token: String,

    #[field(label = "Query")]
    query: String,
}

/// A generated default that takes no context, in a form that has none.
fn fixed_token() -> &'static str {
    "fixed"
}

#[test]
fn a_form_without_a_context_keeps_the_shorter_names() {
    let view = Search::render();
    assert_eq!(view.field("token").unwrap().value.as_deref(), Some("fixed"));

    assert!(Search::from_urlencoded("token=fixed&query=rust").is_ok());
    assert!(Search::submit_urlencoded("token=fixed&query=rust").is_valid());
    // A generated default is still a render-time thing only, context or no
    // context: it never stands in for a value the submission failed to carry.
    assert!(Search::from_urlencoded("query=rust").is_err());

    // And they are the context-taking ones with `()` filled in.
    assert_eq!(
        Search::render_with_context(&())
            .field("token")
            .unwrap()
            .value,
        view.field("token").unwrap().value
    );
}

#[test]
fn a_form_that_generates_nothing_says_so_at_compile_time() {
    const { assert!(!Address::GENERATES_DEFAULTS) };
    const { assert!(Search::GENERATES_DEFAULTS) };
    // And the answer is the whole flatten tree's, not just the outer form's.
    const { assert!(<WithToken<Address> as WebForm>::GENERATES_DEFAULTS) };

    #[derive(WebForm, Debug)]
    struct Plain {
        #[field(flatten)]
        inner: Address,
    }
    const { assert!(!Plain::GENERATES_DEFAULTS) };

    #[derive(WebForm, Debug)]
    struct Wrapping {
        #[field(flatten, prefix = "s_")]
        inner: Search,
    }
    const { assert!(Wrapping::GENERATES_DEFAULTS) };
    assert_eq!(
        Wrapping::render()
            .field("s_token")
            .unwrap()
            .value
            .as_deref(),
        Some("fixed")
    );
}
