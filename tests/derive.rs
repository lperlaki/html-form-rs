#![allow(dead_code)]

//! The ways an attribute may be written.
//!
//! Everywhere the derive accepts more than one spelling of the same thing —
//! a flag with or without a value, an option's label by position or by name,
//! a number written as an integer or as a fraction — both spellings mean the
//! same, and these tests say so.

use html_form::{ErrorKind, FieldKind, Form, FormValue};

// ─── Literals ─────────────────────────────────────────────────────────────────

/// The crate keeps `min`, `max`, `step` and `default` as strings, so a literal
/// of any kind reaches the spec as what it was written as. An integer bound on
/// an `i128` therefore survives exactly, and a fractional step is not rounded
/// into one.
#[test]
fn a_constraint_keeps_whichever_kind_of_literal_it_was_written_as() {
    #[derive(Form, Debug)]
    struct Measurements {
        #[field(min = 0.5, max = 99.5, step = 0.25)]
        fraction: f64,

        #[field(min = -170141183460469231731687303715884105728)]
        wide: i128,

        #[field(default = 'x')]
        initial: char,

        #[field(default = true)]
        agreed: bool,

        #[field(default = "web")]
        source: String,
    }

    let view = Measurements::render();

    let fraction = view.field("fraction").unwrap();
    assert_eq!(fraction.min.as_deref(), Some("0.5"));
    assert_eq!(fraction.max.as_deref(), Some("99.5"));
    assert_eq!(fraction.step.as_deref(), Some("0.25"));

    assert_eq!(
        view.field("wide").unwrap().min.as_deref(),
        Some("-170141183460469231731687303715884105728"),
        "which no `f64` bound could have held"
    );

    assert_eq!(view.field("initial").unwrap().value.as_deref(), Some("x"));
    assert!(view.field("agreed").unwrap().checked);
    assert_eq!(view.field("source").unwrap().value.as_deref(), Some("web"));

    // And each one is enforced with the value it was written as.
    let errors = Measurements::from_urlencoded("fraction=0.6&wide=0").unwrap_err();
    assert!(matches!(
        errors.field("fraction").next().unwrap().kind,
        ErrorKind::Step { .. }
    ));
}

// ─── Flags ────────────────────────────────────────────────────────────────────

/// A flag is a bare word, or the same word with the answer spelled out. The
/// second is what lets an attribute turn something *off*.
#[test]
fn a_flag_may_be_a_bare_word_or_carry_the_answer() {
    #[derive(Form, Debug)]
    #[form(novalidate = true)]
    struct Explicit {
        #[field(required = false)]
        name: String,

        #[field(optional = false)]
        nickname: Option<String>,

        #[field(disabled = true, readonly = true, autofocus = true)]
        locked: String,

        #[field(skip = true)]
        internal: String,
    }

    #[derive(Form, Debug)]
    #[form(novalidate)]
    struct Bare {
        #[field(disabled, readonly, autofocus)]
        locked: String,
    }

    let view = Explicit::render();
    assert!(view.novalidate);
    assert!(
        !view.field("name").unwrap().required,
        "a scalar is required until something says otherwise"
    );
    assert!(
        view.field("nickname").unwrap().required,
        "and an `Option` is not, until something says otherwise"
    );
    assert!(view.field("internal").is_none(), "skipped either way");

    let locked = view.field("locked").unwrap();
    assert!(locked.disabled && locked.readonly && locked.autofocus);

    // The bare spelling means the same as `= true`.
    let bare = Bare::render();
    assert!(bare.novalidate);
    let locked = bare.field("locked").unwrap();
    assert!(locked.disabled && locked.readonly && locked.autofocus);

    // A field the attribute took the requirement off is not enforced as one.
    let parsed = Explicit::from_urlencoded("locked=x&nickname=ada").unwrap();
    assert_eq!(parsed.name, "", "`name` was left out and that is allowed");
    assert_eq!(parsed.nickname.as_deref(), Some("ada"));

    // And the one it put the requirement on is.
    assert!(
        Explicit::from_urlencoded("locked=x")
            .unwrap_err()
            .has_field("nickname")
    );
}

// ─── Options ──────────────────────────────────────────────────────────────────

/// An option's label may come by position or by name, and an option that
/// declares none uses its own value.
#[test]
fn an_option_may_be_written_four_ways_and_mean_the_same() {
    #[derive(Form, Debug)]
    struct Pick {
        // The value alone: the label is the value.
        #[option("de")]
        // The label by position.
        #[option("ch", "Switzerland")]
        // The label by name, which is what a group needs alongside it.
        #[option("at", label = "Austria", group = "EU")]
        // A trailing comma, so a long list edits cleanly.
        #[option("fr", "France")]
        // And one nobody may pick, which still shows what it would have been.
        #[option("su", "Soviet Union", disabled)]
        country: String,
    }

    let view = Pick::render();
    let choices = &view.field("country").unwrap().choices;

    assert_eq!(choices.len(), 5);
    assert_eq!(choices[0].label, "de", "its own value stands in");
    assert_eq!(choices[0].label_key, None, "and it is text, never a key");
    assert_eq!(choices[1].label, "Switzerland");
    assert_eq!(choices[2].label, "Austria");
    assert_eq!(choices[2].group.as_deref(), Some("EU"));
    assert_eq!(choices[3].label, "France");
    assert!(choices[4].disabled);
    assert!(!choices[0].disabled);

    // A disabled option is still one of the declared values, because the
    // server checks what arrived and not what the markup offered.
    assert!(Pick::from_urlencoded("country=de").is_ok());
    assert_eq!(
        Pick::from_urlencoded("country=uk")
            .unwrap_err()
            .field("country")
            .next()
            .unwrap()
            .kind,
        ErrorKind::NotAChoice
    );
}

/// The same spellings work on a `FormChoice` variant, whose value and label
/// both come from the variant name until something else names them.
#[test]
fn a_choice_variant_names_itself_until_something_else_names_it() {
    #[derive(html_form::FormChoice, Debug)]
    enum Plan {
        FreeForever,
        #[choice(value = "pro", label = "Professional", group = "Paid", disabled = true)]
        Pro,
    }

    #[derive(Form, Debug)]
    struct Pick {
        plan: Plan,
    }

    let view = Pick::render();
    let choices = &view.field("plan").unwrap().choices;

    assert_eq!(choices[0].value, "free-forever");
    assert_eq!(choices[0].label, "Free Forever");
    assert_eq!(choices[1].value, "pro");
    assert_eq!(choices[1].label, "Professional");
    assert_eq!(choices[1].group.as_deref(), Some("Paid"));
    assert!(choices[1].disabled);
}

// ─── Attributes the crate has no part in ──────────────────────────────────────

/// A field carries whatever else the struct needs. Doc comments, `serde` and
/// anything else pass by untouched, so a form can be a record type too.
#[test]
fn an_attribute_that_belongs_to_something_else_is_left_alone() {
    #[derive(Form, Debug, serde::Serialize)]
    struct Signup {
        /// A doc comment, which is an attribute like any other.
        #[serde(rename = "email_address")]
        #[field(type = "email")]
        email: String,

        /// A field the crate never sees at all.
        #[allow(unused)]
        #[field(skip)]
        internal: u32,
    }

    let signup = Signup {
        email: "ada@example.com".to_owned(),
        internal: 1,
    };
    assert_eq!(
        serde_json::to_value(&signup).unwrap(),
        serde_json::json!({"email_address": "ada@example.com", "internal": 1})
    );
    assert_eq!(
        Signup::render().field("email").unwrap().kind,
        FieldKind::Email
    );
}

// ─── Names ────────────────────────────────────────────────────────────────────

/// A label nobody wrote comes from the field name, with the underscores turned
/// back into spaces and the first letter raised.
#[test]
fn a_label_is_built_from_the_field_name_where_there_is_none() {
    #[derive(Form, Debug)]
    struct Odd {
        first_name: String,
        // A leading underscore says "unused" in Rust and nothing to a reader,
        // so it is not part of the label.
        _internal_note: String,
        // A name with nothing but underscores has nothing to raise.
        __: String,
        // And a name of one letter has nothing after the first.
        x: String,
    }

    let view = Odd::render();
    assert_eq!(
        view.field("first_name").unwrap().label.as_deref(),
        Some("First name")
    );
    assert_eq!(
        view.field("_internal_note").unwrap().label.as_deref(),
        Some("Internal note")
    );
    assert_eq!(view.field("__").unwrap().label.as_deref(), Some(""));
    assert_eq!(view.field("x").unwrap().label.as_deref(), Some("X"));
}

/// The submitted name is the field name until `name = "…"` says otherwise,
/// which is what lets a form match markup it did not generate.
#[test]
fn a_field_may_be_submitted_under_a_name_that_is_not_its_own() {
    #[derive(Form, Debug)]
    struct Legacy {
        #[field(name = "user[email]", label = "Email")]
        email: String,
    }

    assert_eq!(
        Legacy::from_urlencoded("user%5Bemail%5D=ada@example.com")
            .unwrap()
            .email,
        "ada@example.com"
    );

    // A name that is not a usable id becomes one, and the label points at it.
    let view = Legacy::render();
    let field = view.field("user[email]").unwrap();
    assert_eq!(field.id, "user-email-");
    assert!(
        field
            .to_html()
            .contains(r#"<label class="html-form__label" for="user-email-">"#)
    );
}

// ─── Generic forms ────────────────────────────────────────────────────────────

/// A type parameter may stand anywhere a concrete type may, and the derive
/// bounds it by what the field asks the parameter to be.
#[test]
fn a_type_parameter_may_appear_at_any_depth_of_a_field_type() {
    #[derive(Form, Debug)]
    struct Page<T: FormValue> {
        one: T,
        maybe: Option<T>,
        many: Vec<T>,
        // Not a parameter at all, alongside them.
        title: String,
    }

    let page = Page::<u32>::from_urlencoded("one=1&maybe=2&many=3&many=4&title=Hello").unwrap();
    assert_eq!(page.one, 1);
    assert_eq!(page.maybe, Some(2));
    assert_eq!(page.many, [3, 4]);

    // Each instantiation resolves the control of the type it was given.
    assert_eq!(
        Page::<u32>::render().field("one").unwrap().kind,
        FieldKind::Number
    );
    assert_eq!(
        Page::<String>::render().field("one").unwrap().kind,
        FieldKind::Text
    );
}
