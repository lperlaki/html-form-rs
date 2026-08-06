#![allow(dead_code)]

//! Rendering a blank form: the spec, the view and the built-in HTML.

use std::borrow::Cow;
use web_form::{FieldKind, WebForm};

#[derive(WebForm)]
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
    #[derive(WebForm)]
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
    assert!(html.contains(r#"<button type="submit" class="web-form__submit">Join</button>"#));

    assert!(html.contains(
        r#"<input type="email" name="email" id="email" required autocomplete="email" placeholder="you@example.com">"#
    ));
    assert!(html.contains(r#"<label class="web-form__label" for="email">Email address"#));
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
            r#"<p class="web-form__help" id="password-help">At least 12 characters.</p>"#
        )
    );
}

#[test]
fn an_optional_select_offers_an_empty_option() {
    #[derive(WebForm)]
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
    #[derive(WebForm)]
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
    use web_form::{Control, TextControl, TextFormat};

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
    let submitted = Signup::render_with(
        &web_form::Values::parse("email=a@b.com"),
        &web_form::FormErrors::new(),
    );
    let email = submitted.field("email").unwrap();
    assert!(matches!(email.value, Some(Cow::Owned(_))));
    assert!(borrowed(&email.name));
}

/// Escaping hands back text that has nothing to escape, rather than copying it.
#[test]
fn escaping_copies_only_what_it_changes() {
    assert!(matches!(web_form::escape("plain text"), Cow::Borrowed(_)));
    assert_eq!(web_form::escape("a<b & c"), "a&lt;b &amp; c");
    assert_eq!(web_form::escape("\"'"), "&quot;&#39;");
    // Non-ASCII is left alone, and not cut in half by the search for entities.
    assert_eq!(web_form::escape("Grüße <hier>"), "Grüße &lt;hier&gt;");
}
