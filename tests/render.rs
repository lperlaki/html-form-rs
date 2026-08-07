#![allow(dead_code)]

//! Rendering a blank form: the spec, the view and the built-in HTML.

use html_form::{FieldKind, Form, Outcome};
use std::borrow::Cow;

#[derive(Form)]
#[form(id = "signup", action = "/signup", method = "post", submit = "Join")]
struct Signup {
    #[field(
        type = "email",
        label = "Email address",
        autocomplete = "email",
        placeholder = "you@example.com"
    )]
    email: String,

    #[field(type = "password", minlength = 12, help = "At least 12 characters.")]
    password: String,

    #[field(label = "Age", min = 18, max = 120)]
    age: Option<u32>,

    #[field(default = true)]
    newsletter: bool,

    #[field(type = "textarea", label = "Bio", rows = 4, maxlength = 500)]
    bio: Option<String>,

    #[field(label = "Plan")]
    #[option("free", "Free")]
    #[option("pro", "Professional")]
    plan: String,

    #[field(type = "hidden", default = "web")]
    source: String,

    #[field(skip)]
    internal_note: String,
}

#[test]
fn fields_keep_declaration_order_and_skip_is_excluded() {
    let names: Vec<Cow<'static, str>> = Signup::spec()
        .fields()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(
        names,
        [
            "email",
            "password",
            "age",
            "newsletter",
            "bio",
            "plan",
            "source"
        ]
    );
}

#[test]
fn kinds_come_from_the_attribute_or_the_rust_type() {
    let view = Signup::render();
    let kind = |name: &str| view.field(name).unwrap().kind;

    assert_eq!(kind("email"), FieldKind::Email);
    assert_eq!(kind("password"), FieldKind::Password);
    // Inferred from `u32`.
    assert_eq!(kind("age"), FieldKind::Number);
    // Inferred from `bool`.
    assert_eq!(kind("newsletter"), FieldKind::Checkbox);
    assert_eq!(kind("bio"), FieldKind::Textarea);
    // Declaring options is enough to mean "select".
    assert_eq!(kind("plan"), FieldKind::Select);
    assert_eq!(kind("source"), FieldKind::Hidden);
}

#[test]
fn required_defaults_to_the_shape_of_the_type() {
    let view = Signup::render();
    let required = |name: &str| view.field(name).unwrap().required;

    assert!(required("email"));
    assert!(required("password"));
    assert!(required("plan"));
    // `Option`, `Vec` and `bool` are optional by default.
    assert!(!required("age"));
    assert!(!required("bio"));
    assert!(!required("newsletter"));
}

#[test]
fn labels_default_to_the_humanised_field_name() {
    let view = Signup::render();
    assert_eq!(
        view.field("email").unwrap().label.as_deref(),
        Some("Email address")
    );
    assert_eq!(
        view.field("password").unwrap().label.as_deref(),
        Some("Password")
    );
    assert_eq!(
        view.field("newsletter").unwrap().label.as_deref(),
        Some("Newsletter")
    );
    // A hidden control has nothing to label.
    assert_eq!(view.field("source").unwrap().label, None);
}

#[test]
fn constraints_reach_the_view() {
    let view = Signup::render();

    let password = view.field("password").unwrap();
    assert_eq!(password.minlength, Some(12));
    assert_eq!(password.help.as_deref(), Some("At least 12 characters."));

    let age = view.field("age").unwrap();
    assert_eq!(age.min.as_deref(), Some("18"));
    assert_eq!(age.max.as_deref(), Some("120"));
    // `u32` implies an integer step even though the attribute never said so.
    assert_eq!(age.step.as_deref(), Some("1"));

    let bio = view.field("bio").unwrap();
    assert_eq!(bio.rows, Some(4));
    assert_eq!(bio.maxlength, Some(500));
}

#[test]
fn implied_numeric_bounds_come_from_the_rust_type() {
    #[derive(Form)]
    struct Counts {
        small: u8,
        signed: i32,
        ratio: f64,
    }

    let view = Counts::render();
    // Unsigned types cannot go below zero, so the browser should not offer it.
    assert_eq!(view.field("small").unwrap().min.as_deref(), Some("0"));
    assert_eq!(view.field("signed").unwrap().min, None);
    assert_eq!(view.field("ratio").unwrap().step.as_deref(), Some("any"));
}

#[test]
fn defaults_fill_a_blank_form() {
    let view = Signup::render();
    assert_eq!(view.field("source").unwrap().value.as_deref(), Some("web"));
    assert!(view.field("newsletter").unwrap().checked);
    assert_eq!(view.field("email").unwrap().value, None);
}

#[test]
fn a_generated_default_is_produced_afresh_for_every_render() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static ISSUED: AtomicUsize = AtomicUsize::new(0);

    fn ticket() -> String {
        format!("t{}", ISSUED.fetch_add(1, Ordering::Relaxed))
    }

    #[derive(Form)]
    struct Booking {
        #[field(type = "hidden", default = ticket)]
        ticket: String,
        seat: Option<u32>,
    }

    let value = |view: html_form::FormView| view.field("ticket").unwrap().value.clone().unwrap();
    assert_eq!(value(Booking::render()), "t0");
    assert_eq!(value(Booking::render()), "t1");
    // Once per render, not once per field, and only for the field that asked.
    assert_eq!(ISSUED.load(Ordering::Relaxed), 2);
    assert_eq!(Booking::render().field("seat").unwrap().value, None);
}

/// A generator hands back the field's own type, and the crate writes the value
/// out the way the field writes out every other value it holds. Nothing has to
/// make a string of it first, and nothing can make one of the wrong shape.
#[test]
fn a_generated_default_is_the_type_the_field_holds() {
    use std::fmt;
    use std::str::FromStr;

    #[derive(Debug, PartialEq)]
    struct Day(String);

    impl FromStr for Day {
        type Err = &'static str;
        fn from_str(raw: &str) -> Result<Self, Self::Err> {
            Ok(Day(raw.to_owned()))
        }
    }

    impl fmt::Display for Day {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }

    fn usual_party() -> u32 {
        2
    }
    fn today() -> Day {
        Day("2026-08-07".to_owned())
    }
    // Anything the field's type is made from will do, so a value that already
    // exists costs nothing to hand over.
    fn walk_in() -> &'static str {
        "walk-in"
    }

    #[derive(Form, Debug)]
    struct Booking {
        #[field(label = "Seats", default = usual_party)]
        seats: u32,
        // A field converted by its own `FromStr` takes its own type too, and
        // `Display` writes it out, exactly as it does for a stored record.
        #[field(from_str, type = "date", default = today)]
        day: Day,
        #[field(label = "Reference", default = walk_in)]
        reference: String,
        // An `Option<T>` field asks for one `T`: a default fills in one control.
        #[field(label = "Table", default = usual_party)]
        table: Option<u32>,
    }

    let view = Booking::render();
    let value = |name: &str| view.field(name).unwrap().value.clone();
    assert_eq!(value("seats").as_deref(), Some("2"));
    assert_eq!(value("day").as_deref(), Some("2026-08-07"));
    assert_eq!(value("reference").as_deref(), Some("walk-in"));
    assert_eq!(value("table").as_deref(), Some("2"));
}

/// `default` with nothing after it is the field type's own `Default`. A `const`
/// cannot call that either, so it is glue in the spec like every other default.
#[test]
fn a_bare_default_is_the_field_types_own() {
    #[derive(Default, Debug, PartialEq)]
    struct Size(String);

    impl html_form::FormValue for Size {
        const CONTROL: html_form::Control = html_form::Control::TEXT;

        fn parse_form_value(raw: &str) -> Result<Self, html_form::ValueError> {
            Ok(Size(raw.to_owned()))
        }

        fn to_form_value(&self) -> Cow<'static, str> {
            Cow::Owned(self.0.clone())
        }
    }

    impl Default for Order {
        fn default() -> Self {
            Order {
                seats: 1,
                size: Size("medium".to_owned()),
                note: String::new(),
                extras: Vec::new(),
            }
        }
    }

    #[derive(Form, Debug)]
    struct Order {
        #[field(label = "Seats", default)]
        seats: u32,
        // Whatever the type says its default is, whether or not the crate
        // wrote the type.
        #[field(label = "Size", default)]
        size: Size,
        #[field(label = "Note", default)]
        note: String,
        // One `T`, as any default is: it fills in one control.
        #[field(label = "Extras", default)]
        extras: Vec<u32>,
    }

    let view = Order::render();
    let value = |name: &str| view.field(name).unwrap().value.clone();
    assert_eq!(value("seats").as_deref(), Some("0"));
    assert_eq!(value("size").as_deref(), Some(""));
    assert_eq!(value("note").as_deref(), Some(""));
    assert_eq!(value("extras").as_deref(), Some("0"));

    // It is the *field type's* default, not the form's: `Order::default()` says
    // one seat and a medium, and neither reaches the render.
    assert_eq!(Order::default().seats, 1);

    // And it fills a render only, as every default does.
    assert!(Order::from_urlencoded("size=large&note=&extras=").is_err());
}

/// A generator may hand out something the server records as it goes, so the
/// crate calls it where the render uses the value and nowhere else.
#[test]
fn a_generator_is_not_run_for_a_value_the_render_will_not_show() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static ISSUED: AtomicUsize = AtomicUsize::new(0);

    fn ticket() -> String {
        format!("t{}", ISSUED.fetch_add(1, Ordering::Relaxed))
    }

    #[derive(Form)]
    struct Booking {
        #[field(label = "Reference", default = ticket)]
        reference: String,
    }

    // A blank form shows the value, so it is produced.
    assert_eq!(
        Booking::render()
            .field("reference")
            .unwrap()
            .value
            .as_deref(),
        Some("t0")
    );
    assert_eq!(ISSUED.load(Ordering::Relaxed), 1);

    // A re-render shows what the user typed, so there is nothing to produce.
    let view = Booking::render_submitted(
        &html_form::Values::parse("reference=mine"),
        &html_form::FormErrors::new(),
    );
    assert_eq!(
        view.field("reference").unwrap().value.as_deref(),
        Some("mine")
    );
    assert_eq!(ISSUED.load(Ordering::Relaxed), 1, "no second ticket");
}

#[test]
fn a_generated_default_is_the_forms_own_only_where_it_is_hidden() {
    // A generator may hand back anything the field's type is made from, so a
    // value that was already around costs nothing to return.
    fn fresh() -> &'static str {
        "fresh"
    }

    #[derive(Form)]
    struct Renewal {
        #[field(type = "hidden", default = fresh)]
        token: String,
        #[field(default = fresh)]
        reference: String,
    }

    let render = |body: &str| {
        Renewal::render_submitted(
            &html_form::Values::parse(body),
            &html_form::FormErrors::new(),
        )
    };

    // A blank form is where a default is a default: both fields show one.
    let blank = Renewal::render();
    assert_eq!(
        blank.field("token").unwrap().value.as_deref(),
        Some("fresh")
    );
    assert_eq!(
        blank.field("reference").unwrap().value.as_deref(),
        Some("fresh")
    );

    let view = render("token=stale&reference=mine");
    // Nobody typed the hidden field, so there is nothing of the user's to
    // preserve: it is the form's own value, minted again rather than echoed.
    assert_eq!(view.field("token").unwrap().value.as_deref(), Some("fresh"));
    // A field the user can see keeps what they put in it.
    assert_eq!(
        view.field("reference").unwrap().value.as_deref(),
        Some("mine")
    );

    // Once there are values to show, a visible field empty is a visible field
    // empty — whether the name came back blank or did not come back at all.
    // Filling it in would put a value in front of the user that the submission
    // never carried, for them to send back without noticing.
    let view = render("reference=");
    assert_eq!(view.field("reference").unwrap().value.as_deref(), Some(""));
    let view = render("");
    assert_eq!(view.field("reference").unwrap().value, None);
    // The token is still the form's to supply, however little else came back.
    assert_eq!(view.field("token").unwrap().value.as_deref(), Some("fresh"));
}

#[test]
fn an_edit_form_shows_what_the_record_holds_and_not_a_generated_default() {
    fn today() -> &'static str {
        "2026-08-06"
    }

    #[derive(Form)]
    struct Article {
        #[field(type = "hidden", default = today)]
        edited_on: String,
        #[field(label = "Due", default = today)]
        due: Option<String>,
    }

    let view = Article {
        edited_on: "1999-01-01".to_owned(),
        due: None,
    }
    .render_filled();

    // A record with no date has no date. Offering one here would have the user
    // save a value they never entered.
    assert_eq!(view.field("due").unwrap().value, None);
    // The hidden field is the form's own on every path, so it is stamped afresh
    // rather than carrying whatever the record was last edited with.
    assert_eq!(
        view.field("edited_on").unwrap().value.as_deref(),
        Some("2026-08-06")
    );
}

#[test]
fn a_field_that_resets_shows_its_default_and_not_what_came_in() {
    fn fresh() -> &'static str {
        "fresh"
    }

    #[derive(Form)]
    struct Renewal {
        // Visible, so without `reset` the crate would echo the rejected one.
        #[field(reset, default = fresh)]
        token: String,
        // A literal default resets just as a generated one does.
        #[field(reset, default = "web")]
        source: String,
        // Nothing to fall back on, so the box comes back empty.
        #[field(type = "password", reset)]
        password: String,
    }

    let view = Renewal::render_submitted(
        &html_form::Values::parse("token=stale&source=email&password=hunter2"),
        &html_form::FormErrors::new(),
    );
    assert_eq!(view.field("token").unwrap().value.as_deref(), Some("fresh"));
    assert_eq!(view.field("source").unwrap().value.as_deref(), Some("web"));
    assert_eq!(view.field("password").unwrap().value, None);

    // A record to edit is the same case: none of these fields is the record's.
    let view = Renewal {
        token: "stale".to_owned(),
        source: "email".to_owned(),
        password: "hunter2".to_owned(),
    }
    .render_filled();
    assert_eq!(view.field("token").unwrap().value.as_deref(), Some("fresh"));
    assert_eq!(view.field("source").unwrap().value.as_deref(), Some("web"));
    assert_eq!(view.field("password").unwrap().value, None);
}

/// Resetting is settled where the form is declared, not where it renders. The
/// spec carries the answer, and a field may write over the one the derive
/// worked out for it.
#[test]
fn whether_a_field_resets_is_decided_in_the_spec() {
    fn fresh() -> &'static str {
        "fresh"
    }

    #[derive(Form)]
    struct Renewal {
        // Hidden and generated, so it is the form's own and resets untold.
        #[field(type = "hidden", default = fresh)]
        token: String,
        // The same field, told to keep what it was sent.
        #[field(type = "hidden", default = fresh, reset = false)]
        echoed: String,
        // Hidden, but the value is written into the spec: a literal is as much
        // the record's to carry as any other value.
        #[field(type = "hidden", default = "web")]
        source: String,
        #[field(label = "Reference")]
        reference: String,
    }

    let resets = |name: &str| Renewal::SPEC.field(name).unwrap().spec.reset;
    assert!(resets("token"));
    assert!(!resets("echoed"));
    assert!(!resets("source"));
    assert!(!resets("reference"));

    let view = Renewal::render_submitted(
        &html_form::Values::parse("token=stale&echoed=stale&source=email&reference=mine"),
        &html_form::FormErrors::new(),
    );
    let value = |name: &str| view.field(name).unwrap().value.clone();
    assert_eq!(value("token").as_deref(), Some("fresh"));
    assert_eq!(value("echoed").as_deref(), Some("stale"));
    assert_eq!(value("source").as_deref(), Some("email"));
    assert_eq!(value("reference").as_deref(), Some("mine"));
}

#[test]
fn a_box_that_resets_returns_to_the_state_a_blank_form_starts_in() {
    #[derive(Form)]
    struct Preferences {
        #[field(reset, default = "true")]
        newsletter: bool,
        #[field(default = "true")]
        receipts: bool,
    }

    // An unchecked box submits nothing at all. The one that resets is ticked
    // again all the same, and the one beside it stays as the user left it.
    let view =
        Preferences::render_submitted(&html_form::Values::parse(""), &html_form::FormErrors::new());
    assert!(view.field("newsletter").unwrap().checked);
    assert!(!view.field("receipts").unwrap().checked);
}

#[test]
fn resetting_says_what_the_next_render_shows_and_nothing_about_parsing() {
    #[derive(Form, Debug)]
    struct ChangePassword {
        #[field(type = "password", reset, minlength = 12)]
        new_password: String,
    }

    let outcome = ChangePassword::submit_urlencoded("new_password=short");
    let Outcome::Invalid { errors, view } = outcome else {
        panic!("`short` is under the minimum length");
    };

    // The check read what the user sent, and its message is on the field.
    assert!(errors.has_field("new_password"));
    assert!(view.field("new_password").unwrap().has_errors);
    // Only the value is gone.
    assert_eq!(view.field("new_password").unwrap().value, None);

    // And a submission that passes is parsed as any other.
    let ok = ChangePassword::from_urlencoded("new_password=long-enough-here").unwrap();
    assert_eq!(ok.new_password, "long-enough-here");
}

#[test]
fn choices_become_options() {
    let view = Signup::render();
    let plan = view.field("plan").unwrap();
    assert_eq!(plan.choices.len(), 2);
    assert_eq!(plan.choices[1].value, "pro");
    assert_eq!(plan.choices[1].label, "Professional");
    assert!(!plan.choices[0].selected);
}

#[test]
fn html_carries_the_form_and_control_attributes() {
    let html = Signup::render().to_html();

    assert!(html.contains(r#"<form id="signup" action="/signup" method="post""#));
    assert!(html.contains(r#"<button type="submit" class="html-form__submit">Join</button>"#));

    assert!(html.contains(
        r#"<input type="email" name="email" id="email" required autocomplete="email" placeholder="you@example.com">"#
    ));
    assert!(html.contains(r#"<label class="html-form__label" for="email">Email address"#));
    assert!(html.contains(r#"minlength="12""#));
    assert!(
        html.contains(r#"<input type="number" name="age" id="age" min="18" max="120" step="1">"#)
    );
    assert!(html.contains(r#"<textarea name="bio" id="bio" rows="4" maxlength="500"></textarea>"#));
    assert!(html.contains(r#"<input type="hidden" name="source" id="source" value="web">"#));
    assert!(html.contains(r#"<option value="pro">Professional</option>"#));
    // The checkbox default is reflected as `checked`.
    assert!(html.contains(r#"name="newsletter" id="newsletter" checked>"#));
}

#[test]
fn help_text_is_wired_up_for_screen_readers() {
    let html = Signup::render().to_html();
    assert!(html.contains(r#"aria-describedby="password-help""#));
    assert!(
        html.contains(
            r#"<p class="html-form__help" id="password-help">At least 12 characters.</p>"#
        )
    );
}

#[test]
fn an_optional_select_offers_an_empty_option() {
    #[derive(Form)]
    struct Filter {
        #[field(type = "select")]
        #[option("new", "New")]
        #[option("old", "Old")]
        state: Option<String>,
    }

    let html = Filter::render().to_html();
    assert!(html.contains(r#"<option value="" selected></option>"#));
}

#[test]
fn values_and_labels_are_escaped() {
    #[derive(Form)]
    struct Risky {
        #[field(label = "Name <script>", default = r#"a" onload="x"#)]
        name: String,
    }

    let html = Risky::render().to_html();
    assert!(!html.contains("<script>"));
    assert!(html.contains("Name &lt;script&gt;"));
    assert!(html.contains(r#"value="a&quot; onload=&quot;x""#));
}

#[test]
fn the_view_serialises_for_template_engines() {
    let view = Signup::render();
    let json = serde_json::to_value(&view).unwrap();

    assert_eq!(json["method"], "post");
    assert_eq!(json["fields"][0]["name"], "email");
    assert_eq!(json["fields"][0]["kind"], "email");
    assert_eq!(json["fields"][5]["choices"][1]["label"], "Professional");
}

#[test]
fn a_control_carries_only_the_attributes_that_control_accepts() {
    use html_form::{Control, TextControl, TextFormat};

    let spec = Signup::spec();
    assert_eq!(spec.action, Some("/signup"));

    // The email field's control names its format and its text constraints, and
    // has nowhere to put a `min` or a `rows`.
    assert_eq!(
        spec.field("email").unwrap().spec.control,
        Control::Text(TextControl {
            format: TextFormat::Email { multiple: false },
            ..TextControl::DEFAULT
        })
    );

    // `min`/`max` from the attribute and `step` from `u32` land in the same
    // `Bounds`, which only a numeric control has.
    let age = spec.field("age").unwrap();
    let Control::Number(number) = age.spec.control else {
        panic!("`age` should be a number");
    };
    assert_eq!(number.bounds.min, Some("18"));
    assert_eq!(number.bounds.max, Some("120"));
    assert_eq!(number.bounds.step, Some("1"));

    assert_eq!(spec.field("bio").unwrap().spec.control.rows(), Some(4));
    assert!(spec.field("plan").unwrap().spec.control.restricts_choices());
}

/// Everything a view copies out of the spec is borrowed from it, so rendering a
/// form allocates only for what the spec cannot know.
#[test]
fn a_view_borrows_what_the_spec_already_holds() {
    // Which half of the `Cow` it is *is* the assertion here, so `&str` — what
    // clippy would rather see — would defeat the test.
    #[allow(clippy::ptr_arg)]
    fn borrowed(value: &Cow<'static, str>) -> bool {
        matches!(value, Cow::Borrowed(_))
    }

    let view = Signup::render();
    assert!(borrowed(&view.submit_label));

    let email = view.field("email").unwrap();
    assert!(borrowed(&email.name));
    assert!(borrowed(email.label.as_ref().unwrap()));
    assert!(borrowed(email.placeholder.as_ref().unwrap()));
    assert!(borrowed(email.autocomplete.as_ref().unwrap()));
    // Nothing in the name needs replacing, so the id is the name itself.
    assert!(borrowed(&email.id));

    let age = view.field("age").unwrap();
    assert!(borrowed(age.min.as_ref().unwrap()));
    assert!(borrowed(age.max.as_ref().unwrap()));

    // A blank form's values are its declared defaults, which are in the spec.
    let source = view.field("source").unwrap();
    assert!(borrowed(source.value.as_ref().unwrap()));

    // As are the options of a `<select>` declared with `#[option(...)]`.
    let plan = view.field("plan").unwrap();
    assert!(borrowed(&plan.choices[1].value));
    assert!(borrowed(&plan.choices[1].label));

    // What the user typed is the exception: it has to be copied out of the
    // submission, which does not outlive the view.
    let submitted = Signup::render_submitted(
        &html_form::Values::parse("email=a@b.com"),
        &html_form::FormErrors::new(),
    );
    let email = submitted.field("email").unwrap();
    assert!(matches!(email.value, Some(Cow::Owned(_))));
    assert!(borrowed(&email.name));
}

/// Escaping hands back text that has nothing to escape, rather than copying it.
#[test]
fn escaping_copies_only_what_it_changes() {
    assert!(matches!(html_form::escape("plain text"), Cow::Borrowed(_)));
    assert_eq!(html_form::escape("a<b & c"), "a&lt;b &amp; c");
    assert_eq!(html_form::escape("\"'"), "&quot;&#39;");
    // Non-ASCII is left alone, and not cut in half by the search for entities.
    assert_eq!(html_form::escape("Grüße <hier>"), "Grüße &lt;hier&gt;");
}
