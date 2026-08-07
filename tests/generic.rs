#![allow(dead_code)]

//! Forms with type parameters: a wrapper that adds a field to whatever it is
//! given, and the bounds the derive works out for itself.

use html_form::{Entry, Form, Outcome};

#[derive(Form, Debug)]
struct Signup {
    #[field(type = "email", label = "Email address")]
    email: String,

    #[field(label = "Age", min = 18)]
    age: Option<u32>,
}

#[derive(Form, Debug)]
struct Feedback {
    #[field(type = "textarea", maxlength = 500)]
    message: String,
}

/// The wrapper under test. No bound is written on `T`: the flatten is what says
/// it has to be a form, and the derive adds that itself.
#[derive(Form, Debug)]
#[form(method = "post")]
struct WithToken<T> {
    #[field(type = "hidden", label = "")]
    token: String,

    #[field(flatten)]
    inner: T,
}

#[test]
fn a_generic_form_has_a_spec_per_instantiation() {
    let signup = WithToken::<Signup>::SPEC;
    let feedback = WithToken::<Feedback>::SPEC;

    // Two entries either way — the token, and the whole sub-form behind it.
    assert_eq!(signup.entries.len(), 2);
    assert!(matches!(signup.entries[1], Entry::Flatten(_)));

    let names = |spec: &'static html_form::FormSpec| -> Vec<String> {
        spec.fields()
            .iter()
            .map(|field| field.name.to_string())
            .collect()
    };
    assert_eq!(names(signup), ["token", "email", "age"]);
    assert_eq!(names(feedback), ["token", "message"]);

    // The sub-form's own spec is reached through the flatten, not copied into
    // it: the wrapper's field list is the sub-form's, one entry longer.
    assert_eq!(
        signup.flatten_at(1).spec as *const _,
        Signup::SPEC as *const _
    );
}

#[test]
fn the_wrapper_renders_its_own_field_and_then_the_wrapped_ones() {
    let html = WithToken::<Signup>::render().to_html();
    assert!(html.contains(r#"<input type="hidden" name="token" id="token">"#));
    assert!(html.contains(r#"<input type="email" name="email" id="email" required>"#));
    // A hidden field has no label of its own to render.
    assert!(!html.contains(r#"for="token""#));
}

#[test]
fn a_submission_is_parsed_into_both_halves() {
    let form =
        WithToken::<Signup>::from_urlencoded("token=abc&email=ada@example.com&age=36").unwrap();
    assert_eq!(form.token, "abc");
    assert_eq!(form.inner.email, "ada@example.com");
    assert_eq!(form.inner.age, Some(36));
}

#[test]
fn errors_from_either_half_name_the_field_that_caused_them() {
    let Outcome::Invalid { errors, view } =
        WithToken::<Signup>::submit_urlencoded("email=nope&age=7")
    else {
        panic!("a missing token and two bad fields should not have parsed");
    };

    assert!(errors.has_field("token"));
    assert!(errors.has_field("email"));
    assert!(errors.has_field("age"));
    // And the re-render is the wrapper's, carrying the sub-form's messages.
    assert!(view.field("age").unwrap().errors[0].contains("18"));
}

#[test]
fn a_prefix_applies_to_the_whole_wrapped_form() {
    #[derive(Form, Debug)]
    struct Prefixed<T> {
        token: String,

        #[field(flatten, prefix = "signup_", legend = "Your details")]
        inner: T,
    }

    let view = Prefixed::<Signup>::render();
    let names: Vec<&str> = view.fields.iter().map(|f| f.name.as_ref()).collect();
    assert_eq!(names, ["token", "signup_email", "signup_age"]);

    let form =
        Prefixed::<Signup>::from_urlencoded("token=abc&signup_email=ada@example.com").unwrap();
    assert_eq!(form.inner.email, "ada@example.com");
}

#[test]
fn prefixes_compose_through_a_generic_wrapper() {
    #[derive(Form, Debug)]
    struct Address {
        #[field(label = "Postcode", pattern = r"\d{5}")]
        postcode: String,
    }

    #[derive(Form, Debug)]
    struct Order {
        #[field(flatten, prefix = "billing_")]
        billing: Address,
    }

    #[derive(Form, Debug)]
    struct Outer<T> {
        #[field(flatten, prefix = "order_")]
        inner: T,
    }

    let view = Outer::<Order>::render();
    let names: Vec<&str> = view.fields.iter().map(|f| f.name.as_ref()).collect();
    assert_eq!(names, ["order_billing_postcode"]);

    let errors = Outer::<Order>::from_urlencoded("order_billing_postcode=nope").unwrap_err();
    assert!(errors.has_field("order_billing_postcode"));
}

#[test]
fn a_parameter_used_as_a_value_is_bounded_by_form_value() {
    // `T` is a field's own type here rather than a sub-form, so what the derive
    // infers is `FormValue`.
    #[derive(Form, Debug)]
    struct Pair<T> {
        #[field(label = "From")]
        from: T,
        #[field(label = "To")]
        to: Option<T>,
    }

    let words = Pair::<String>::from_urlencoded("from=alpha&to=omega").unwrap();
    assert_eq!(words.to.as_deref(), Some("omega"));

    // The control follows the parameter, so the same struct is a number input
    // for one instantiation and a text input for the other.
    let numbers = Pair::<u32>::render();
    assert_eq!(numbers.field("from").unwrap().input_type, Some("number"));
    assert_eq!(
        Pair::<String>::render().field("from").unwrap().input_type,
        Some("text")
    );

    let errors = Pair::<u32>::from_urlencoded("from=twelve").unwrap_err();
    assert!(errors.has_field("from"));
}

#[test]
fn a_wrapper_fills_itself_in_from_an_existing_value() {
    let form = WithToken {
        token: "abc".to_owned(),
        inner: Signup {
            email: "ada@example.com".to_owned(),
            age: Some(36),
        },
    };

    let view = form.render_filled();
    assert_eq!(view.field("token").unwrap().value.as_deref(), Some("abc"));
    assert_eq!(
        view.field("email").unwrap().value.as_deref(),
        Some("ada@example.com")
    );
}

// ─── A parameter that is the context ──────────────────────────────────────────

/// A form may be generic over its own *context*, not only over the values it
/// holds. `Form::Context` is `'static` — a render hands it to the spec's glue
/// erased, and the glue names the type back — and the derive states that bound
/// for a context that names a parameter, so `Dated<C>` needs nothing written on
/// it beyond the trait its own default calls through.
trait Clock {
    fn today(&self) -> String;
}

fn stamp<C: Clock>(clock: &C) -> String {
    clock.today()
}

#[derive(Form, Debug)]
#[form(context = C)]
struct Dated<C: Clock> {
    #[field(type = "hidden", label = "", default = stamp)]
    on: String,

    #[field(skip)]
    clock: std::marker::PhantomData<C>,
}

#[derive(Debug)]
struct Wall;

impl Clock for Wall {
    fn today(&self) -> String {
        "2026-08-07".to_owned()
    }
}

#[test]
fn a_form_generic_over_its_context_reaches_that_context_from_its_default() {
    let view = Dated::<Wall>::render_with_context(&Wall);
    assert_eq!(
        view.field("on").unwrap().value.as_deref(),
        Some("2026-08-07")
    );
}
