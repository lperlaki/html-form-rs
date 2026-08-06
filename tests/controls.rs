#![allow(dead_code)]

//! The control model: which attributes a control accepts, and what happens to
//! the ones the Rust type and the attribute both have an opinion about.

use html_form::{Control, FieldKind, Form, FormChoice, NumberFormat, TemporalFormat, TextFormat};

#[derive(FormChoice, Debug)]
enum Plan {
    Free,
    Pro,
}

#[derive(Form, Debug)]
struct Everything {
    // `multiple` is meaningless on a text input, so a `Vec<String>` must not
    // render one — it is still submitted repeatedly.
    tags: Vec<String>,

    // `type = "radio"` restyles the enum's control without discarding the
    // variants it is choosing between.
    #[field(type = "radio")]
    plan: Plan,

    // `type = "range"` keeps the bounds the integer type implies.
    #[field(type = "range", max = 10)]
    level: u32,

    // `multiple` on an email input means a comma-separated list, which is the
    // one text format that has a meaning for it.
    #[field(type = "email", multiple)]
    cc: Vec<String>,

    #[field(type = "date", min = "2026-01-01")]
    starts: Option<String>,

    #[field(type = "textarea", rows = 4, maxlength = 500)]
    notes: Option<String>,

    #[field(type = "file", accept = "image/*")]
    avatar: Option<String>,
}

#[test]
fn each_attribute_lands_in_the_control_that_accepts_it() {
    let spec = Everything::spec();
    let control = |name: &str| spec.field(name).unwrap().spec.control;

    assert!(matches!(
        control("tags"),
        Control::Text(t) if t.format == TextFormat::Text
    ));
    assert!(matches!(
        control("cc"),
        Control::Text(t) if t.format == TextFormat::Email { multiple: true }
    ));
    assert!(matches!(
        control("notes"),
        Control::Textarea(a) if a.rows == Some(4) && a.maxlength == Some(500)
    ));
    assert!(matches!(
        control("avatar"),
        Control::File(f) if f.accept == Some("image/*")
    ));
}

#[test]
fn a_vec_of_text_is_submitted_repeatedly_but_renders_no_multiple() {
    let view = Everything::render();
    let tags = view.field("tags").unwrap();

    assert_eq!(tags.kind, FieldKind::Text);
    assert!(!tags.multiple);
    assert!(
        view.to_html()
            .contains(r#"<input type="text" name="tags" id="tags">"#)
    );

    let parsed = Everything::from_urlencoded("tags=a&tags=b&plan=free&level=2&cc=x@y.z").unwrap();
    assert_eq!(parsed.tags, ["a", "b"]);
}

#[test]
fn restyling_a_choice_control_keeps_its_options() {
    let Control::Choose(choose) = Everything::spec().field("plan").unwrap().spec.control else {
        panic!("`plan` should be a chooser");
    };
    assert_eq!(choose.choices.len(), 2);

    let plan = Everything::render();
    let plan = plan.field("plan").unwrap();
    assert_eq!(plan.kind, FieldKind::Radio);
    assert_eq!(plan.choices.len(), 2);
    // A radio group is single-valued whatever the field is typed as.
    assert!(!plan.multiple);
}

#[test]
fn an_explicit_numeric_format_keeps_the_bounds_the_rust_type_implies() {
    let Control::Number(number) = Everything::spec().field("level").unwrap().spec.control else {
        panic!("`level` should be numeric");
    };
    assert_eq!(number.format, NumberFormat::Range);
    // `min` and `step` come from `u32`, `max` from the attribute.
    assert_eq!(number.bounds.min, Some("0"));
    assert_eq!(number.bounds.step, Some("1"));
    assert_eq!(number.bounds.max, Some("10"));
}

#[test]
fn a_date_bound_is_a_string_and_compares_chronologically() {
    let Control::Temporal(temporal) = Everything::spec().field("starts").unwrap().spec.control
    else {
        panic!("`starts` should be temporal");
    };
    assert_eq!(temporal.format, TemporalFormat::Date);
    assert_eq!(temporal.bounds.min, Some("2026-01-01"));

    let errors = Everything::from_urlencoded("tags=a&plan=free&level=2&cc=x@y.z&starts=2025-12-31")
        .unwrap_err();
    assert_eq!(errors.field("starts").count(), 1);
}

/// A `Control` is `Copy` and `const`-constructible, which is what lets the
/// derive assemble one in a `static` and hand out a `&'static FormSpec`.
#[test]
fn a_control_can_be_written_by_hand_in_const_position() {
    use html_form::{Bounds, NumberControl};

    const PERCENT: Control = Control::Number(NumberControl {
        format: NumberFormat::Range,
        bounds: Bounds {
            min: Some("0"),
            max: Some("100"),
            step: Some("5"),
        },
    });

    assert_eq!(PERCENT.kind(), FieldKind::Range);
    // Nothing else is reachable on a numeric control.
    assert_eq!(PERCENT.pattern(), None);
    assert_eq!(PERCENT.rows(), None);
    assert!(PERCENT.choices().is_empty());
}
