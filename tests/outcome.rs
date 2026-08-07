#![allow(dead_code)]

//! [`Outcome`], and the `Form` trait as something to implement by hand.
//!
//! The derive writes the three members without a body and the trait provides
//! everything else. A hand-written impl writes the same three, against the same
//! [`ParseCtx`] the derive uses, and gets the same methods for free.

use html_form::{
    Choice, ChoiceStyle, ChooseControl, Control, Entry, FieldSpec, Form, FormErrors, FormSpec,
    FormView, Outcome, ParseCtx, Values,
};

#[derive(Form, Debug, PartialEq)]
struct Signup {
    #[field(type = "email", label = "Email address")]
    email: String,
    #[field(min = 18)]
    age: Option<u32>,
}

fn valid() -> Outcome<Signup> {
    Signup::submit_urlencoded("email=ada@example.com&age=36")
}

fn invalid() -> Outcome<Signup> {
    Signup::submit_urlencoded("email=nope&age=7")
}

// ─── Outcome ──────────────────────────────────────────────────────────────────

#[test]
fn an_outcome_says_which_of_the_two_it_is() {
    assert!(valid().is_valid());
    assert!(!invalid().is_valid());
}

/// The value alone, for a handler that has already decided what to do with a
/// rejection.
#[test]
fn the_parsed_value_can_be_taken_without_the_re_render() {
    assert_eq!(
        valid().ok(),
        Some(Signup {
            email: "ada@example.com".to_owned(),
            age: Some(36),
        })
    );
    assert_eq!(invalid().ok(), None);
}

/// And the re-render alone, for one that has already decided what to do with
/// a success.
#[test]
fn the_re_render_can_be_taken_without_the_value() {
    assert!(valid().view().is_none());

    let view = invalid().view().expect("the submission was rejected");
    assert_eq!(view.field("email").unwrap().value.as_deref(), Some("nope"));
    assert!(view.field("age").unwrap().has_errors);
}

/// As a `Result`, so a handler can use `?` and let the error be the form to
/// render again.
#[test]
fn an_outcome_converts_into_a_result_whose_error_is_the_form() {
    fn handle(body: &str) -> Result<String, Box<FormView>> {
        let signup = Signup::submit_urlencoded(body).into_result()?;
        Ok(signup.email)
    }

    assert_eq!(
        handle("email=ada@example.com&age=36").unwrap(),
        "ada@example.com"
    );
    let view = handle("email=nope").unwrap_err();
    assert!(view.has_errors);
}

/// The invalid case carries both halves, so a handler that wants to log the
/// errors *and* render the form need not parse twice.
#[test]
fn the_invalid_case_carries_the_typed_errors_beside_the_view() {
    let Outcome::Invalid { errors, view } = invalid() else {
        panic!("the submission was supposed to be rejected");
    };
    assert_eq!(errors.len(), 2);
    assert!(errors.has_field("email") && errors.has_field("age"));
    // The same messages reach the view, ready to render.
    assert_eq!(
        view.field("email").unwrap().errors,
        ["Enter a valid email address."]
    );
}

/// An empty submission is not a rejection of the request. It is a form with
/// every field left blank, which is exactly what the user sent.
#[test]
fn an_empty_submission_is_an_invalid_form_and_not_a_failure() {
    let Outcome::Invalid { errors, view } = Signup::submit(&Values::new()) else {
        panic!("`email` is required");
    };
    assert!(errors.has_field("email"));
    assert_eq!(view.field("email").unwrap().value, None);
}

// ─── A form written by hand ───────────────────────────────────────────────────

const CHOICES: &[Choice] = &[Choice::new("free", "Free"), Choice::new("pro", "Pro")];

static LOGIN: FormSpec = FormSpec {
    action: Some("/login"),
    submit_label: Some(html_form::Text::literal("Sign in")),
    entries: &[
        Entry::Field(FieldSpec {
            name: "username",
            label: Some(html_form::Text::literal("Username")),
            required: true,
            ..FieldSpec::DEFAULT
        }),
        Entry::Field(FieldSpec {
            name: "plan",
            control: Control::Choose(ChooseControl {
                style: ChoiceStyle::Select,
                multiple: false,
                choices: CHOICES,
            }),
            ..FieldSpec::DEFAULT
        }),
    ],
    ..FormSpec::DEFAULT
};

#[derive(Debug, PartialEq)]
struct Login {
    username: String,
    plan: String,
}

impl Form for Login {
    type Context = ();
    const SPEC: &'static FormSpec = &LOGIN;

    /// The same two calls the derive emits, against the same context. The
    /// accessors are what a hand-written impl reaches for when it wants to
    /// look at the submission itself.
    fn parse_in(ctx: &mut ParseCtx<'_, Self::Context>) -> Option<Self> {
        let username = ctx.field::<String>(LOGIN.field_at(0));
        let plan = ctx.field::<String>(LOGIN.field_at(1));

        // A check that no attribute could express: the two together.
        if ctx.values().contains("plan") && !ctx.values().contains("username") {
            ctx.push_form_error("Say who you are before you pick a plan.".into());
        }

        Some(Login {
            username: username?,
            plan: plan?,
        })
    }

    fn fill_in(&self, values: &mut Values, prefix: &str) {
        values.push(format!("{prefix}username"), self.username.clone());
        values.push(format!("{prefix}plan"), self.plan.clone());
    }
}

#[test]
fn a_hand_written_form_parses_through_the_same_context() {
    assert_eq!(
        Login::from_urlencoded("username=ada&plan=pro").unwrap(),
        Login {
            username: "ada".to_owned(),
            plan: "pro".to_owned(),
        }
    );
}

#[test]
fn a_hand_written_form_gets_every_other_method_for_free() {
    let view = Login::render();
    assert_eq!(view.action.as_deref(), Some("/login"));
    assert_eq!(view.submit_label, "Sign in");
    assert_eq!(view.field("plan").unwrap().choices.len(), 2);

    // Including the spec, for a caller that would rather not name the type.
    assert_eq!(Login::spec().entries.len(), 2);
    assert!(std::ptr::eq(Login::spec(), Login::SPEC));
}

#[test]
fn its_own_checks_reach_the_same_error_list_as_the_built_in_ones() {
    let errors = Login::from_urlencoded("plan=pro").unwrap_err();
    assert!(errors.has_field("username"), "the built-in required check");
    assert_eq!(
        errors.form_errors()[0].message.as_str(),
        "Say who you are before you pick a plan.",
        "and the one the impl wrote"
    );
}

#[test]
fn a_hand_written_form_fills_itself_in_and_renders_what_it_holds() {
    let login = Login {
        username: "ada".to_owned(),
        plan: "pro".to_owned(),
    };

    let values = login.to_values();
    assert_eq!(values.get("username"), Some("ada"));

    let view = login.render_filled();
    assert_eq!(
        view.field("username").unwrap().value.as_deref(),
        Some("ada")
    );
    assert!(view.field("plan").unwrap().choices[1].selected);
}

/// Three members and a spec are the whole of the trait a hand-written form has
/// to write. A default is part of the spec, so a form that has none writes
/// nothing about defaults at all.
#[test]
fn a_hand_written_form_writes_a_spec_and_two_methods_and_nothing_else() {
    struct Empty;

    impl Form for Empty {
        type Context = ();
        const SPEC: &'static FormSpec = &FormSpec::DEFAULT;

        fn parse_in(_ctx: &mut ParseCtx<'_, Self::Context>) -> Option<Self> {
            Some(Empty)
        }

        fn fill_in(&self, _values: &mut Values, _prefix: &str) {}
    }

    assert!(Empty::render().fields.is_empty());
    assert!(Empty::submit(&Values::new()).is_valid());
}

// ─── The pairs of methods ─────────────────────────────────────────────────────

/// Every method that takes a context has a twin without one, available where
/// the context is a unit. The two agree.
#[test]
fn each_method_agrees_with_its_context_taking_twin() {
    let body = "email=ada@example.com&age=36";
    let values = Values::parse(body);
    let errors = FormErrors::new();

    assert_eq!(
        Signup::from_values(&values).unwrap(),
        Signup::from_values_with_context(&values, &()).unwrap()
    );
    assert_eq!(
        Signup::from_urlencoded(body).unwrap(),
        Signup::from_urlencoded_with_context(body, &()).unwrap()
    );
    assert_eq!(
        Signup::render().to_html(),
        Signup::render_with_context(&()).to_html()
    );
    assert_eq!(
        Signup::render_submitted(&values, &errors).to_html(),
        Signup::render_submitted_with_context(&values, &errors, &()).to_html()
    );
    assert_eq!(
        Signup::submit(&values).ok(),
        Signup::submit_with_context(&values, &()).ok()
    );
    assert_eq!(
        Signup::submit_urlencoded(body).ok(),
        Signup::submit_urlencoded_with_context(body, &()).ok()
    );

    let signup = Signup::from_urlencoded(body).unwrap();
    assert_eq!(
        signup.render_filled().to_html(),
        signup.render_filled_with_context(&()).to_html()
    );

    let translate = |key: &str| match key {
        "nothing" => Some("nothing"),
        _ => None,
    };
    assert_eq!(
        Signup::render_localized(translate).to_html(),
        Signup::render_localized_with_context(translate, &()).to_html()
    );
}
