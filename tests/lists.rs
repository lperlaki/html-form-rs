//! A `Vec<T>` behind a control that holds one value: one input per value, all
//! under one name, in the markup as much as in the parse.

use html_form::{ErrorKind, FieldError, Form, FormChoice, FormErrors, Outcome, Values};

#[derive(Form, Debug)]
struct Allowlist {
    #[field(pattern = "[a-z]+", minlength = 2, placeholder = "name")]
    tags: Vec<String>,
}

fn invalid<T: Form<Context = ()>>(body: &str) -> (FormErrors, Box<html_form::FormView>) {
    match T::submit_urlencoded(body) {
        Outcome::Valid(_) => panic!("unexpectedly accepted {body:?}"),
        Outcome::Invalid { errors, view } => (errors, view),
    }
}

fn rows(errors: &FormErrors, name: &str) -> Vec<Option<usize>> {
    errors.field(name).map(|error| error.index).collect()
}

// ─── What the view says ───────────────────────────────────────────────────────

/// The Rust type cannot tell the two apart, and the spec can.
#[test]
fn a_list_of_inputs_is_repeated_and_a_multiple_control_is_not() {
    #[derive(FormChoice, Debug)]
    enum Color {
        Red,
        Green,
    }
    #[derive(Form, Debug)]
    struct Mixed {
        names: Vec<String>,
        select: Vec<Color>,
        #[field(type = "checkbox")]
        group: Vec<Color>,
        #[field(type = "email")]
        emails: Vec<String>,
        #[field(type = "hidden")]
        hidden: Vec<String>,
    }

    let view = Mixed::render();
    let repeated = |name| view.field(name).unwrap().repeated;
    assert!(repeated("names"));
    assert!(repeated("hidden"));
    assert!(!repeated("select"));
    assert!(!repeated("group"));
    assert!(!repeated("emails"));
    assert!(view.field("select").unwrap().multiple);
    assert!(!view.field("names").unwrap().multiple);

    let _ = (Color::Red, Color::Green);
}

/// A position among the boxes that came back says nothing about the page, so
/// a checkbox group and a select keep their messages on the field.
#[test]
fn only_a_repeated_field_gives_an_error_a_row() {
    #[derive(FormChoice, Debug)]
    enum Color {
        Red,
    }
    #[derive(Form, Debug)]
    struct Group {
        #[field(type = "checkbox")]
        colors: Vec<Color>,
    }
    #[derive(Form, Debug)]
    struct Select {
        colors: Vec<Color>,
    }

    let (errors, _) = invalid::<Group>("colors=&colors=red&colors=nope");
    assert_eq!(rows(&errors, "colors"), [None]);
    let (errors, _) = invalid::<Select>("colors=nope");
    assert_eq!(rows(&errors, "colors"), [None]);

    let _ = Color::Red;
}

// ─── Blank rows ───────────────────────────────────────────────────────────────

/// A blank row is no value, but it is a row the user put there, and the rows
/// after it stay where they were.
#[test]
fn a_blank_row_survives_a_rejected_submission() {
    let (errors, view) = invalid::<Allowlist>("tags=aa&tags=&tags=B&tags=cc");
    assert_eq!(rows(&errors, "tags"), [Some(2), Some(2)]);

    let field = view.field("tags").unwrap();
    assert_eq!(field.values, ["aa", "", "B", "cc"]);

    // Four rows, and a blank one after them to grow the list.
    assert_eq!(view.to_html().matches("name=\"tags\"").count(), 5);
}

/// The parse drops it all the same.
#[test]
fn a_blank_row_is_no_value() {
    let form = Allowlist::from_urlencoded("tags=aa&tags=&tags=+&tags=bb").unwrap();
    assert_eq!(form.tags, ["aa", "bb"]);
}

// ─── Rows in the markup ───────────────────────────────────────────────────────

#[test]
fn every_value_gets_its_own_input_with_every_constraint() {
    let html = Allowlist {
        tags: vec!["aa".into(), "bb".into(), "cc".into()],
    }
    .render_filled()
    .to_html();

    for value in ["aa", "bb", "cc"] {
        assert!(html.contains(&format!("value=\"{value}\"")), "{html}");
    }
    assert_eq!(html.matches("pattern=\"[a-z]+\"").count(), 4, "{html}");
    assert_eq!(html.matches("minlength=\"2\"").count(), 4, "{html}");
    assert_eq!(html.matches("placeholder=\"name\"").count(), 4, "{html}");
}

#[test]
fn each_message_sits_under_its_own_row_and_points_at_it() {
    let (_, view) = invalid::<Allowlist>("tags=aa&tags=B&tags=cc");
    let rows = view.field("tags").unwrap().rows();

    assert_eq!(rows.len(), 4);
    assert!(rows[0].errors.is_empty());
    assert_eq!(rows[1].errors.len(), 2);
    assert!(rows[2].errors.is_empty());
    assert_eq!(rows[1].id, "tags-1");
    assert_eq!(rows[1].described_by().as_deref(), Some("tags-error-1"));

    let html = view.to_html();
    assert!(html.contains("aria-describedby=\"tags-error-1\""), "{html}");
    assert!(html.contains("id=\"tags-error-1\""), "{html}");
    assert_eq!(html.matches("<ul").count(), 1, "{html}");
}

/// A row is an ordinary field, so what renders one control renders a row.
#[test]
fn the_label_points_at_the_first_row_and_ids_never_repeat() {
    let view = Allowlist {
        tags: vec!["aa".into(), "bb".into()],
    }
    .render_filled();
    let field = view.field("tags").unwrap();
    let rows = field.rows();

    assert_eq!(rows[0].id, field.id);
    let mut ids: Vec<_> = rows.iter().map(|row| row.id.clone()).collect();
    ids.dedup();
    assert_eq!(ids.len(), rows.len());
    assert!(rows.iter().all(|row| !row.repeated));
    assert!(rows.iter().all(|row| row.name == "tags"));
}

#[test]
fn an_empty_list_shows_one_input_to_fill_in() {
    let view = Allowlist::render();
    assert_eq!(view.field("tags").unwrap().rows().len(), 1);
    assert_eq!(view.to_html().matches("<input").count(), 1);
}

/// Only the first row of a required list has to be filled in.
#[test]
fn a_required_list_requires_one_row_and_not_every_row() {
    #[derive(Form, Debug)]
    struct Required {
        #[field(required)]
        tags: Vec<String>,
    }
    let view = Required {
        tags: vec!["a".into()],
    }
    .render_filled();
    let rows = view.field("tags").unwrap().rows();
    assert!(rows[0].required);
    assert!(!rows[1].required);
}

/// Each row of a list of choices shows its own value as selected.
#[test]
fn a_row_of_a_select_selects_its_own_value() {
    #[derive(FormChoice, Debug, PartialEq)]
    enum Color {
        Red,
        Green,
    }
    #[derive(Form, Debug)]
    struct Order {
        #[field(list(max = 3))]
        colors: Vec<Color>,
    }

    let view = Order {
        colors: vec![Color::Green, Color::Red],
    }
    .render_filled();
    let field = view.field("colors").unwrap();
    assert!(field.repeated);
    let rows = field.rows();
    assert_eq!(rows.len(), 3);
    assert!(rows[0].choices[1].selected);
    assert!(rows[1].choices[0].selected);
    assert!(rows[2].choices.iter().all(|choice| !choice.selected));
}

// ─── The rules of a list ──────────────────────────────────────────────────────

#[derive(Form, Debug)]
struct Limited {
    #[field(pattern = "[a-z]+", list(min = 2, max = 3, distinct))]
    tags: Vec<String>,
}

#[test]
fn a_row_that_breaks_the_items_rule_is_named() {
    let (errors, _) = invalid::<Limited>("tags=a&tags=A");
    assert_eq!(rows(&errors, "tags"), [Some(1)]);
}

#[test]
fn a_repeat_is_an_error_on_the_row_that_repeats() {
    let (errors, _) = invalid::<Limited>("tags=a&tags=b&tags=&tags=a");
    let error = errors.field("tags").next().unwrap();
    assert_eq!(error.kind, ErrorKind::Duplicate);
    assert_eq!(error.index, Some(3));
}

#[test]
fn a_count_outside_the_range_is_an_error_on_the_field() {
    let (errors, _) = invalid::<Limited>("tags=a&tags=b&tags=c&tags=d");
    let error = errors.field("tags").next().unwrap();
    assert_eq!(error.kind, ErrorKind::TooMany { max: 3, count: 4 });
    assert_eq!(error.index, None);

    let (errors, _) = invalid::<Limited>("tags=a&tags=");
    let error = errors.field("tags").next().unwrap();
    assert_eq!(error.kind, ErrorKind::TooFew { min: 2, count: 1 });
}

#[test]
fn blank_rows_do_not_count() {
    let form = Limited::from_urlencoded("tags=a&tags=&tags=&tags=&tags=b").unwrap();
    assert_eq!(form.tags, ["a", "b"]);
}

/// A first entry is a requirement the browser can check.
#[test]
fn a_minimum_makes_the_list_required() {
    let view = Limited::render();
    assert!(view.field("tags").unwrap().required);
    let (errors, _) = invalid::<Limited>("");
    assert_eq!(
        errors.field("tags").next().unwrap().kind,
        ErrorKind::Required
    );

    #[derive(Form, Debug)]
    struct Optional {
        #[field(optional, list(min = 2))]
        tags: Vec<String>,
    }
    assert!(Optional::from_urlencoded("").is_ok());
    assert!(Optional::from_urlencoded("tags=a").is_err());
}

#[test]
fn a_full_list_offers_no_blank_row_and_says_how_full_it_can_get() {
    let view = Limited {
        tags: vec!["a".into(), "b".into(), "c".into()],
    }
    .render_filled();
    let field = view.field("tags").unwrap();
    assert_eq!(field.rows().len(), 3);
    assert_eq!(field.list.unwrap().max, Some(3));
}

// ─── A list typed into one control ────────────────────────────────────────────

#[derive(Form, Debug)]
struct Dns {
    #[field(pattern = "[0-9.]+", list(split))]
    dns: Vec<String>,
}

#[test]
fn separators_split_a_value_and_repeats_go() {
    let form = Dns::from_urlencoded("dns=1.1.1.1&dns=9.9.9.9,+1.1.1.1;8.8.8.8").unwrap();
    assert_eq!(form.dns, ["1.1.1.1", "9.9.9.9", "8.8.8.8"]);
}

#[test]
fn a_bad_item_errors_on_its_own_index_and_the_view_shows_the_items() {
    let (errors, view) = invalid::<Dns>("dns=1.1.1.1,nope&dns=8.8.8.8");
    assert_eq!(rows(&errors, "dns"), [Some(1)]);
    assert_eq!(
        view.field("dns").unwrap().values,
        ["1.1.1.1", "nope", "8.8.8.8"]
    );
}

// ─── Pinning an error to a row from outside the parse ─────────────────────────

// A validator takes the field's own type, which is a `Vec`.
#[allow(clippy::ptr_arg)]
fn no_ten(tags: &Vec<String>) -> Result<(), FieldError> {
    match tags.iter().position(|tag| tag == "ten") {
        Some(position) => Err(FieldError::custom("Not ten.").at(position)),
        None => Ok(()),
    }
}

#[derive(Form, Debug)]
struct Checked {
    #[field(validate = no_ten)]
    tags: Vec<String>,
}

#[derive(Form, Debug)]
#[form(validate = no_ten_across)]
struct CheckedAcross {
    tags: Vec<String>,
}

fn no_ten_across(form: &CheckedAcross) -> Result<(), FormErrors> {
    no_ten(&form.tags).map_err(|error| {
        let mut errors = FormErrors::new();
        errors.push("tags", error);
        errors
    })
}

/// A validator sees no blank rows, and the page has them.
#[test]
fn a_position_from_a_validator_becomes_the_row_on_the_page() {
    let (errors, view) = invalid::<Checked>("tags=one&tags=&tags=ten");
    assert_eq!(rows(&errors, "tags"), [Some(2)]);
    let field = view.field("tags").unwrap();
    assert_eq!(field.values[field.error_indexes[0].unwrap()], "ten");
}

#[test]
fn a_position_from_a_form_check_becomes_the_row_under_its_prefix() {
    let (errors, _) = invalid::<CheckedAcrossHolder>("a_tags=&a_tags=one&a_tags=&a_tags=ten");
    assert_eq!(rows(&errors, "a_tags"), [Some(3)]);
}

#[derive(Form, Debug)]
struct CheckedAcrossHolder {
    #[field(flatten, prefix = "a_")]
    inner: CheckedAcross,
}

/// The translation, for a handler that finds a problem after the parse.
#[test]
fn a_handler_can_turn_a_position_into_a_row() {
    let values = Values::parse("tags=one&tags=&tags=ten");
    let row = values.row_of("tags", 1).unwrap();
    let mut errors = FormErrors::new();
    errors.push("tags", FieldError::custom("Taken.").at(row));

    let view = Allowlist::render_submitted(&values, &errors);
    assert_eq!(view.field("tags").unwrap().rows()[2].errors, ["Taken."]);
}

/// A field that resets shows no rows, so its messages have nowhere else to go.
#[test]
fn a_message_for_a_row_that_is_not_shown_belongs_to_the_field() {
    #[derive(Form, Debug)]
    struct Reset {
        #[field(reset, minlength = 3)]
        tags: Vec<String>,
    }
    let (errors, view) = invalid::<Reset>("tags=abc&tags=x");
    assert_eq!(rows(&errors, "tags"), [Some(1)]);

    let field = view.field("tags").unwrap();
    assert_eq!(field.error_indexes, [None]);
    assert_eq!(field.field_errors().count(), 1);
    assert!(view.to_html().contains("<li>Enter at least 3"));
}

/// A renderer that pushes messages by hand leaves the lists uneven. A message
/// with no row of its own is a message about the field.
#[test]
fn a_message_pushed_by_hand_is_about_the_field() {
    let mut view = Allowlist::render();
    let field = view.field_mut("tags").unwrap();
    field.errors.push("By hand.".into());
    field.has_errors = true;
    assert_eq!(field.field_errors().count(), 1);
    assert!(view.to_html().contains("<li>By hand.</li>"));
}

// ─── A client that reads JSON ─────────────────────────────────────────────────

#[test]
fn the_json_errors_say_which_row() {
    let (errors, _) = invalid::<Limited>("tags=a&tags=A&tags=A");
    let json = serde_json::to_value(&errors).unwrap();
    assert_eq!(json["indexes"]["tags"], serde_json::json!([1, 2]));
    assert_eq!(json["fields"]["tags"].as_array().unwrap().len(), 2);
}

/// A client that knows nothing of rows sees what it always did.
#[test]
fn json_without_rows_has_no_indexes_key() {
    #[derive(Form, Debug)]
    struct One {
        name: String,
    }
    let (errors, _) = invalid::<One>("");
    assert!(
        serde_json::to_value(&errors)
            .unwrap()
            .get("indexes")
            .is_none()
    );
}
