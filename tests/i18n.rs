//! Any string a person reads can be an i18n key instead of literal text.

use html_form::Text;
use html_form::prelude::*;

/// A stand-in for whatever an application's i18n backend is: the crate only
/// ever asks it for a key and takes what it gets.
fn german(key: &str) -> Option<&'static str> {
    match key {
        "signup.email.label" => Some("E-Mail-Adresse"),
        "signup.email.help" => Some("Wir schreiben Ihnen nur bei Bedarf."),
        "signup.email.placeholder" => Some("sie@beispiel.de"),
        "signup.submit" => Some("Konto erstellen"),
        "country.de" => Some("Deutschland"),
        "country.non-eu" => Some("Nicht-EU"),
        "address.legend" => Some("Rechnungsadresse"),
        _ => None,
    }
}

#[derive(Form)]
#[form(submit = t("signup.submit"))]
struct Signup {
    #[field(
        type = "email",
        label = t("signup.email.label"),
        help = t("signup.email.help"),
        placeholder = t("signup.email.placeholder"),
    )]
    email: String,

    // Literal text, mixed into the same form.
    #[field(label = "Age")]
    age: Option<u32>,

    #[field(label = t("signup.country"))]
    #[option("de", t("country.de"))]
    #[option("ch", "Schweiz", group = t("country.non-eu"))]
    country: String,
}

#[test]
fn a_key_reaches_the_spec_marked_as_one() {
    let spec = Signup::spec();
    let email = spec.field("email").unwrap();
    assert_eq!(email.spec.label, Some(Text::key("signup.email.label")));
    assert_eq!(
        email.spec.help.as_ref().unwrap().key_str(),
        Some("signup.email.help")
    );

    // A plain string literal is still plain text.
    let age = spec.field("age").unwrap();
    assert_eq!(age.spec.label, Some(Text::literal("Age")));
    assert_eq!(age.spec.label.as_ref().unwrap().key_str(), None);
}

#[test]
fn an_unresolved_view_shows_the_key_and_carries_it_alongside() {
    let view = Signup::render();
    let email = view.field("email").unwrap();

    // The key doubles as the string, so `{{ field.label }}` renders something.
    assert_eq!(email.label.as_deref(), Some("signup.email.label"));
    assert_eq!(email.label_key.as_deref(), Some("signup.email.label"));
    assert_eq!(email.help_key.as_deref(), Some("signup.email.help"));
    assert_eq!(
        email.placeholder_key.as_deref(),
        Some("signup.email.placeholder")
    );
    assert_eq!(view.submit_label_key.as_deref(), Some("signup.submit"));

    // Literal text never grows a key.
    let age = view.field("age").unwrap();
    assert_eq!(age.label.as_deref(), Some("Age"));
    assert_eq!(age.label_key, None);
}

#[test]
fn localizing_replaces_the_text_and_drops_the_key() {
    let view = Signup::render_localized(german);
    let email = view.field("email").unwrap();

    assert_eq!(email.label.as_deref(), Some("E-Mail-Adresse"));
    assert_eq!(email.label_key, None);
    assert_eq!(
        email.help.as_deref(),
        Some("Wir schreiben Ihnen nur bei Bedarf.")
    );
    assert_eq!(email.placeholder.as_deref(), Some("sie@beispiel.de"));
    assert_eq!(view.submit_label, "Konto erstellen");
    assert_eq!(view.submit_label_key, None);

    // Literal text is left exactly as declared.
    assert_eq!(view.field("age").unwrap().label.as_deref(), Some("Age"));
}

#[test]
fn a_key_no_backend_knows_stays_visible() {
    let view = Signup::render_localized(german);
    let country = view.field("country").unwrap();

    // Better a label reading `signup.country` than a blank one.
    assert_eq!(country.label.as_deref(), Some("signup.country"));
    assert_eq!(country.label_key.as_deref(), Some("signup.country"));
}

#[test]
fn option_labels_and_optgroups_are_localised_too() {
    let view = Signup::render_localized(german);
    let choices = &view.field("country").unwrap().choices;

    assert_eq!(choices[0].label, "Deutschland");
    assert_eq!(choices[0].label_key, None);
    // A literal label among keyed ones is untouched.
    assert_eq!(choices[1].label, "Schweiz");
    assert_eq!(choices[1].group.as_deref(), Some("Nicht-EU"));
}

#[test]
fn the_built_in_html_renders_whatever_the_view_currently_holds() {
    let html = Signup::render_localized(german).to_html();
    assert!(html.contains(">E-Mail-Adresse"));
    assert!(html.contains(r#"placeholder="sie@beispiel.de""#));
    assert!(html.contains(">Konto erstellen</button>"));
    assert!(html.contains(">Deutschland</option>"));
}

#[test]
fn a_flatten_legend_can_be_a_key() {
    #[derive(Form)]
    struct Address {
        #[field(label = "Street")]
        street: String,
    }

    #[derive(Form)]
    struct Order {
        #[field(flatten, prefix = "billing_", legend = t("address.legend"))]
        billing: Address,
    }

    let view = Order::render();
    assert_eq!(
        view.field("billing_street").unwrap().group_key.as_deref(),
        Some("address.legend")
    );

    let view = Order::render_localized(german);
    let street = view.field("billing_street").unwrap();
    assert_eq!(street.group.as_deref(), Some("Rechnungsadresse"));
    assert_eq!(street.group_key, None);
    assert!(view.to_html().contains("<legend>Rechnungsadresse</legend>"));
}

#[test]
fn a_form_choice_variant_can_carry_a_key() {
    #[derive(FormChoice)]
    enum Plan {
        Free,
        #[choice(value = "pro", label = t("plan.pro"), group = t("plan.paid"))]
        Pro,
    }

    #[derive(Form)]
    struct Pick {
        #[field(label = "Plan")]
        plan: Plan,
    }

    let view = Pick::render_localized(|key| match key {
        "plan.pro" => Some("Professionell"),
        "plan.paid" => Some("Bezahlt"),
        _ => None,
    });
    let choices = &view.field("plan").unwrap().choices;

    // The variant name is a label nobody wrote, so it is text, not a key.
    assert_eq!(choices[0].label, "Free");
    assert_eq!(choices[0].label_key, None);
    assert_eq!(choices[1].label, "Professionell");
    assert_eq!(choices[1].group.as_deref(), Some("Bezahlt"));
}

#[test]
fn choices_supplied_at_render_time_can_be_keyed_as_well() {
    #[derive(Form)]
    struct Booking {
        #[field(type = "select", label = "Room")]
        room: String,
    }

    let mut view = Booking::render();
    view.field_mut("room").unwrap().set_choices([
        Choice::owned("1", "Boardroom"),
        Choice::owned_keyed("2", "room.library"),
    ]);
    view.localize(|key| (key == "room.library").then_some("Bibliothek"));

    let choices = &view.field("room").unwrap().choices;
    assert_eq!(choices[0].label, "Boardroom");
    assert_eq!(choices[1].label, "Bibliothek");
}

#[test]
fn the_keys_serialise_for_a_template_that_would_rather_translate_itself() {
    let json = serde_json::to_value(Signup::render()).unwrap();
    let email = &json["fields"][0];
    assert_eq!(email["label"], "signup.email.label");
    assert_eq!(email["label_key"], "signup.email.label");
    assert_eq!(json["fields"][1]["label_key"], serde_json::Value::Null);
}

#[test]
fn a_validators_message_can_be_a_key_like_any_other_string() {
    #[derive(Form, Debug)]
    #[form(validate = one_of_each)]
    struct Order {
        #[field(label = "Quantity", validate = in_stock)]
        quantity: u32,
    }

    fn in_stock(quantity: &u32) -> Result<(), Text> {
        (*quantity <= 3)
            .then_some(())
            .ok_or_else(|| Text::key("order.quantity.stock"))
    }

    fn one_of_each(order: &Order) -> Result<(), Text> {
        (order.quantity > 0)
            .then_some(())
            .ok_or_else(|| Text::key("order.empty"))
    }

    let translate = |key: &str| match key {
        "order.quantity.stock" => Some("So viele haben wir nicht."),
        "order.empty" => Some("Die Bestellung ist leer."),
        _ => None,
    };

    let Outcome::Invalid { view, errors } = Order::submit_urlencoded("quantity=9") else {
        panic!("expected the quantity to be rejected");
    };

    // Unresolved, the key is what the message reads as — and it travels
    // alongside, the way a label's does.
    let quantity = view.field("quantity").unwrap();
    assert_eq!(quantity.errors[0], "order.quantity.stock");
    assert_eq!(
        quantity.error_keys[0].as_deref(),
        Some("order.quantity.stock")
    );
    // The typed error carries it too, as the code a caller can match on.
    assert_eq!(
        errors.field("quantity").next().unwrap().code(),
        Some("order.quantity.stock")
    );

    let view = view.localized(translate);
    let quantity = view.field("quantity").unwrap();
    assert_eq!(quantity.errors[0], "So viele haben wir nicht.");
    assert_eq!(quantity.error_keys[0], None);
    assert!(
        view.to_html()
            .contains("<li>So viele haben wir nicht.</li>")
    );

    // Form-level messages are localised the same way.
    let Outcome::Invalid { view, .. } = Order::submit_urlencoded("quantity=0") else {
        panic!("expected the empty order to be rejected");
    };
    let view = view.localized(translate);
    assert_eq!(view.errors[0], "Die Bestellung ist leer.");
    assert_eq!(view.error_keys[0], None);
}

#[test]
fn a_built_in_message_is_text_and_stays_that_way() {
    let Outcome::Invalid { view, .. } = Signup::submit_urlencoded("email=nope") else {
        panic!("expected the bad address to be rejected");
    };
    let email = view.field("email").unwrap();

    // Nothing to resolve: the built-in messages are English text, and a caller
    // who needs them translated matches on the `ErrorKind` instead.
    assert_eq!(email.errors.len(), 1);
    assert_eq!(email.error_keys, [None]);
}

#[test]
fn an_error_added_to_a_view_after_the_fact_can_be_keyed_too() {
    let mut view = Signup::render();
    view.add_field_error("email", Text::key("signup.email.taken"));
    view.add_error("signup.busy"); // a plain string is text, not a key

    view.localize(|key| match key {
        "signup.email.taken" => Some("Diese Adresse ist vergeben."),
        "signup.busy" => Some("nicht übersetzt"),
        _ => None,
    });

    let email = view.field("email").unwrap();
    assert_eq!(email.errors[0], "Diese Adresse ist vergeben.");
    assert!(email.has_errors && view.has_errors);
    assert_eq!(view.errors[0], "signup.busy");
}

#[test]
fn localizing_a_re_render_keeps_both_the_values_and_the_translations() {
    let outcome = Signup::submit_urlencoded("email=nope&country=de");
    let Outcome::Invalid { view, .. } = outcome else {
        panic!("expected the bad address to be rejected");
    };
    let view = view.localized(german);

    let email = view.field("email").unwrap();
    assert_eq!(email.value.as_deref(), Some("nope"));
    assert_eq!(email.label.as_deref(), Some("E-Mail-Adresse"));
    assert!(!email.errors.is_empty());
}
