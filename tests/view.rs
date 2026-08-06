#![allow(dead_code)]

//! The view as data: what a handler may change between building a form and
//! rendering it.
//!
//! Everything here works whether or not the built-in renderer is compiled in.
//! A view is what a template engine reads, and these are the methods that put
//! runtime strings into one.

use html_form::{Choice, FieldKind, Form, FormChoice, FormErrors, Text, Values};

#[derive(FormChoice, Debug, PartialEq)]
enum Plan {
    Free,
    Pro,
}

#[derive(Form, Debug)]
struct Signup {
    #[field(type = "email", label = "Email address", help = "We never share it.")]
    email: String,

    #[field(label = "Plan")]
    plan: Plan,

    #[field(label = "I accept the terms")]
    terms: bool,

    #[field(type = "select", label = "Room")]
    room: String,
}

// ─── Finding and changing a field ─────────────────────────────────────────────

#[test]
fn a_field_is_found_by_the_name_it_is_submitted_under() {
    let view = Signup::render();
    assert_eq!(view.field("email").unwrap().kind, FieldKind::Email);
    assert!(view.field("nothing_here").is_none());

    let mut view = Signup::render();
    assert!(view.field_mut("email").is_some());
    assert!(view.field_mut("nothing_here").is_none());
}

/// The crate has no opinion about where a value comes from, so a handler can
/// put one in after the form is built.
#[test]
fn setting_a_value_replaces_what_the_field_would_have_shown() {
    let mut view = Signup::render();
    view.field_mut("email")
        .unwrap()
        .set_value("ada@example.com");

    let email = view.field("email").unwrap();
    assert_eq!(email.value.as_deref(), Some("ada@example.com"));
    assert_eq!(email.values, ["ada@example.com"]);
}

/// A value that a checkbox reads as "on" ticks the box, whatever kind of field
/// it was set on.
#[test]
fn setting_a_value_a_box_reads_as_on_ticks_it() {
    let mut view = Signup::render();
    for word in ["true", "on", "1", "yes"] {
        view.field_mut("terms").unwrap().set_value(word);
        assert!(view.field("terms").unwrap().checked, "{word}");
    }
    for word in ["false", "off", "0", ""] {
        view.field_mut("terms").unwrap().set_value(word);
        assert!(!view.field("terms").unwrap().checked, "{word}");
    }
}

#[test]
fn setting_a_value_moves_the_selection_to_the_matching_option() {
    let mut view = Signup::render();
    view.field_mut("plan").unwrap().set_value("pro");

    let choices = &view.field("plan").unwrap().choices;
    assert!(!choices[0].selected);
    assert!(choices[1].selected);

    // A value that matches no option selects nothing rather than the first.
    view.field_mut("plan").unwrap().set_value("enterprise");
    assert!(
        view.field("plan")
            .unwrap()
            .choices
            .iter()
            .all(|c| !c.selected)
    );
}

/// Options that only exist at runtime go in the same way, and a selection
/// already made survives them arriving.
#[test]
fn replacing_the_options_keeps_whatever_was_already_selected() {
    let mut view = Signup::render();
    view.field_mut("room").unwrap().set_value("102");
    view.field_mut("room").unwrap().set_choices([
        Choice::owned("101", "Room 101"),
        Choice::owned("102", "Room 102"),
    ]);

    let choices = &view.field("room").unwrap().choices;
    assert_eq!(choices.len(), 2);
    assert!(!choices[0].selected);
    assert!(choices[1].selected);
}

#[test]
fn a_custom_attribute_can_be_set_on_the_form_or_on_one_field() {
    let mut view = Signup::render();
    view.set_attr("hx-post", Some("/signup"));
    view.set_attr("inert", None);
    view.field_mut("email")
        .unwrap()
        .set_attr("data-role", Some("primary"));

    assert_eq!(view.attrs.len(), 2);
    assert_eq!(view.attrs[0].value.as_deref(), Some("/signup"));
    assert_eq!(view.attrs[1].value, None, "a bare boolean attribute");
    assert_eq!(
        view.field("email").unwrap().attrs[0].value.as_deref(),
        Some("primary")
    );

    // Setting the same name again replaces it rather than adding a second.
    view.set_attr("hx-post", Some("/register"));
    assert_eq!(view.attrs.len(), 2);
    assert_eq!(view.attrs[0].value.as_deref(), Some("/register"));
}

// ─── Errors added after the fact ──────────────────────────────────────────────

/// Some errors only the database can find, and it finds them after the parse.
#[test]
fn an_error_can_be_attached_to_a_field_once_the_form_is_built() {
    let mut view = Signup::render();
    assert!(!view.has_errors);

    assert!(view.add_field_error("email", "That address is already registered."));
    assert!(view.has_errors);

    let email = view.field("email").unwrap();
    assert!(email.has_errors);
    assert_eq!(email.errors, ["That address is already registered."]);
}

/// A name no field answers to is not an error to swallow silently.
#[test]
fn attaching_an_error_to_a_field_that_is_not_there_says_so() {
    let mut view = Signup::render();
    assert!(!view.add_field_error("nothing_here", "Anything."));
    assert!(!view.has_errors);
}

#[test]
fn a_form_level_error_belongs_to_no_field() {
    let mut view = Signup::render();
    view.add_error("The card was declined.");

    assert!(view.has_errors);
    assert_eq!(view.errors, ["The card was declined."]);
    assert!(view.fields.iter().all(|f| !f.has_errors));
}

// ─── What the control points at ───────────────────────────────────────────────

/// A control describes itself with whichever of the two it has, and with both
/// when it has both. A control with neither points at nothing.
#[test]
fn a_field_names_the_help_text_and_the_errors_it_has() {
    let mut view = Signup::render();
    assert_eq!(
        view.field("email").unwrap().described_by().as_deref(),
        Some("email-help")
    );
    assert_eq!(view.field("plan").unwrap().described_by(), None);

    view.add_field_error("plan", "Pick one.");
    assert_eq!(
        view.field("plan").unwrap().described_by().as_deref(),
        Some("plan-error")
    );

    view.add_field_error("email", "That address is already registered.");
    assert_eq!(
        view.field("email").unwrap().described_by().as_deref(),
        Some("email-help email-error")
    );
}

// ─── Translation ──────────────────────────────────────────────────────────────

fn german(key: &str) -> Option<&'static str> {
    match key {
        "signup.email" => Some("E-Mail-Adresse"),
        "signup.email.help" => Some("Wir geben sie nie weiter."),
        "signup.email.placeholder" => Some("ada@example.com"),
        "plan.free" => Some("Kostenlos"),
        "plan.paid" => Some("Bezahlt"),
        "signup.declined" => Some("Die Karte wurde abgelehnt."),
        "signup.submit" => Some("Konto anlegen"),
        _ => None,
    }
}

#[derive(FormChoice, Debug)]
enum Keyed {
    #[choice(value = "free", label = t("plan.free"), group = t("plan.paid"))]
    Free,
}

#[derive(Form, Debug)]
#[form(submit_label = t("signup.submit"))]
struct Localised {
    #[field(
        label = t("signup.email"),
        help = t("signup.email.help"),
        placeholder = t("signup.email.placeholder"),
    )]
    email: String,
    plan: Keyed,
}

/// One field translates on its own, for a handler that resolves keys as it
/// walks the view rather than in one pass.
#[test]
fn one_field_can_be_translated_without_the_rest_of_the_form() {
    let mut view = Localised::render();
    view.field_mut("email").unwrap().localize(german);

    let email = view.field("email").unwrap();
    assert_eq!(email.label.as_deref(), Some("E-Mail-Adresse"));
    assert_eq!(email.help.as_deref(), Some("Wir geben sie nie weiter."));
    assert_eq!(email.placeholder.as_deref(), Some("ada@example.com"));
    // The key is gone once something has resolved it.
    assert_eq!(email.label_key, None);

    // And the rest of the form is untouched.
    assert_eq!(view.submit_label, "signup.submit");
    assert_eq!(view.field("plan").unwrap().choices[0].label, "plan.free");
}

#[test]
fn one_option_can_be_translated_on_its_own_too() {
    let mut view = Localised::render();
    view.field_mut("plan").unwrap().choices[0].localize(german);

    let choice = &view.field("plan").unwrap().choices[0];
    assert_eq!(choice.label, "Kostenlos");
    assert_eq!(choice.label_key, None);
    assert_eq!(choice.group.as_deref(), Some("Bezahlt"));
    assert_eq!(choice.group_key, None);
}

/// The by-value form chains onto a `render()`, which is the shape a handler
/// that translates every form wants.
#[test]
fn a_whole_form_translates_in_one_pass() {
    let view = Localised::render().localized(german);

    assert_eq!(view.submit_label, "Konto anlegen");
    assert_eq!(view.submit_label_key, None);
    assert_eq!(
        view.field("email").unwrap().label.as_deref(),
        Some("E-Mail-Adresse")
    );
    assert_eq!(view.field("plan").unwrap().choices[0].label, "Kostenlos");
}

#[test]
fn a_form_level_error_translates_like_any_other_string() {
    let mut view = Localised::render();
    view.add_error(Text::key("signup.declined"));
    view.localize(german);

    assert_eq!(view.errors, ["Die Karte wurde abgelehnt."]);
    assert_eq!(view.error_keys, [None]);
}

/// A key no backend knows stays in place. That is a visible bug rather than a
/// silently blank label.
#[test]
fn a_key_nothing_resolves_stays_visible_and_keeps_its_key() {
    let view = Localised::render().localized(|_: &str| None::<&'static str>);

    let email = view.field("email").unwrap();
    assert_eq!(email.label.as_deref(), Some("signup.email"));
    assert_eq!(email.label_key.as_deref(), Some("signup.email"));
}

// ─── What a blank form and a re-render show ───────────────────────────────────

/// On a blank form a box is ticked by what its default says. On a re-render it
/// is ticked by what came back, and an absent name means unchecked.
#[test]
fn a_box_is_ticked_by_its_default_and_then_by_the_submission() {
    #[derive(Form, Debug)]
    struct Prefs {
        #[field(default = "true")]
        newsletter: bool,
        #[field(default = "false")]
        beta: bool,
    }

    let blank = Prefs::render();
    assert!(blank.field("newsletter").unwrap().checked);
    assert!(!blank.field("beta").unwrap().checked);

    // The user unticked the first and ticked the second.
    let submitted = Values::parse("beta=on");
    let view = Prefs::render_submitted(&submitted, &FormErrors::new());
    assert!(!view.field("newsletter").unwrap().checked);
    assert!(view.field("beta").unwrap().checked);
}

/// A lone radio carries its own value, so it is checked when that value came
/// back and not merely when the name did.
#[test]
fn a_lone_radio_is_checked_only_when_its_own_value_came_back() {
    #[derive(Form, Debug)]
    struct Pick {
        #[field(type = "radio", default = "yes")]
        answer: String,
    }

    let checked = Pick::render_submitted(&Values::parse("answer=yes"), &FormErrors::new());
    assert!(checked.field("answer").unwrap().checked);

    let other = Pick::render_submitted(&Values::parse("answer=no"), &FormErrors::new());
    assert!(!other.field("answer").unwrap().checked);

    let absent = Pick::render_submitted(&Values::new(), &FormErrors::new());
    assert!(!absent.field("answer").unwrap().checked);
}

/// An option with no label of its own shows its value instead. That value is
/// literal text, however the other labels were written.
#[test]
fn an_option_with_no_label_shows_its_value() {
    #[derive(Form, Debug)]
    struct Pick {
        #[option("de", "")]
        #[option("ch", "Switzerland")]
        country: String,
    }

    let view = Pick::render();
    let choices = &view.field("country").unwrap().choices;
    assert_eq!(choices[0].label, "de");
    assert_eq!(choices[0].label_key, None);
    assert_eq!(choices[1].label, "Switzerland");
}
