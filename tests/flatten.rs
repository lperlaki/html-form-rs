#![allow(dead_code)]

//! Reusing one form inside another: `#[field(flatten)]`, with and without a
//! name prefix.

use web_form::{ErrorKind, WebForm};

#[derive(WebForm, Debug, PartialEq)]
struct Address {
    #[field(label = "Street")]
    street: String,

    #[field(label = "Postcode", pattern = r"\d{5}")]
    postcode: String,

    #[field(label = "Country", default = "de")]
    #[option("de", "Germany")]
    #[option("ch", "Switzerland")]
    country: String,
}

#[derive(WebForm, Debug)]
#[form(action = "/orders")]
struct Order {
    #[field(label = "Customer")]
    customer: String,

    #[field(flatten, prefix = "billing_", legend = "Billing address")]
    billing: Address,

    #[field(flatten, prefix = "shipping_", legend = "Shipping address")]
    shipping: Address,
}

/// The same sub-form without a prefix, sharing the parent's namespace.
#[derive(WebForm, Debug)]
struct Profile {
    name: String,

    #[field(flatten)]
    address: Address,
}

fn order_body() -> String {
    [
        "customer=Ada",
        "billing_street=Main+1",
        "billing_postcode=12345",
        "billing_country=de",
        "shipping_street=Side+2",
        "shipping_postcode=54321",
        "shipping_country=ch",
    ]
    .join("&")
}

#[test]
fn a_flattened_form_contributes_its_fields_to_the_parent() {
    let names: Vec<String> = Order::spec().fields().into_iter().map(|f| f.name).collect();
    assert_eq!(
        names,
        [
            "customer",
            "billing_street",
            "billing_postcode",
            "billing_country",
            "shipping_street",
            "shipping_postcode",
            "shipping_country",
        ]
    );
}

#[test]
fn the_same_sub_form_can_be_embedded_twice() {
    let order = Order::from_urlencoded(&order_body()).unwrap();

    assert_eq!(order.billing.postcode, "12345");
    assert_eq!(order.shipping.postcode, "54321");
    assert_eq!(order.shipping.country, "ch");
}

#[test]
fn without_a_prefix_the_names_are_shared_with_the_parent() {
    let names: Vec<String> = Profile::spec()
        .fields()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names, ["name", "street", "postcode", "country"]);

    let profile =
        Profile::from_urlencoded("name=Ada&street=Main+1&postcode=12345&country=de").unwrap();
    assert_eq!(profile.address.street, "Main 1");
}

#[test]
fn constraints_travel_with_the_sub_form() {
    let view = Order::render();
    for name in ["billing_postcode", "shipping_postcode"] {
        assert_eq!(view.field(name).unwrap().pattern.as_deref(), Some(r"\d{5}"));
        assert!(view.field(name).unwrap().required);
    }
}

#[test]
fn errors_are_keyed_by_the_prefixed_name() {
    let body = order_body()
        .replace("shipping_postcode=54321", "shipping_postcode=nope")
        .replace("billing_street=Main+1", "billing_street=");
    let errors = Order::from_urlencoded(&body).unwrap_err();

    assert_eq!(errors.len(), 2);
    assert!(matches!(
        errors.field("billing_street").next().unwrap().kind,
        ErrorKind::Required
    ));
    assert!(matches!(
        errors.field("shipping_postcode").next().unwrap().kind,
        ErrorKind::Pattern { .. }
    ));
    // The unprefixed name belongs to nobody.
    assert!(!errors.has_field("postcode"));
}

#[test]
fn a_rejected_submission_re_renders_both_copies_independently() {
    let body = order_body().replace("shipping_postcode=54321", "shipping_postcode=nope");
    let view = Order::submit_urlencoded(&body).view().unwrap();

    assert!(view.field("shipping_postcode").unwrap().has_errors);
    assert_eq!(
        view.field("shipping_postcode").unwrap().value.as_deref(),
        Some("nope")
    );
    // The other copy of the same sub-form is untouched.
    assert!(!view.field("billing_postcode").unwrap().has_errors);
    assert_eq!(
        view.field("billing_postcode").unwrap().value.as_deref(),
        Some("12345")
    );
}

#[test]
fn each_group_is_rendered_as_its_own_fieldset() {
    let html = Order::render().to_html();

    assert!(html.contains("<legend>Billing address</legend>"));
    assert!(html.contains("<legend>Shipping address</legend>"));
    assert_eq!(html.matches("<fieldset").count(), 2);
    assert_eq!(html.matches("</fieldset>").count(), 2);
    assert!(html.contains(r#"name="billing_street" id="billing_street""#));
}

#[test]
fn defaults_inside_a_flattened_form_still_apply() {
    let view = Order::render();
    let country = view.field("shipping_country").unwrap();
    assert_eq!(country.value.as_deref(), Some("de"));
    assert!(
        country
            .choices
            .iter()
            .find(|c| c.value == "de")
            .unwrap()
            .selected
    );
}

#[test]
fn nesting_more_than_one_level_deep_composes() {
    #[derive(WebForm, Debug)]
    struct Contact {
        #[field(type = "tel", label = "Phone")]
        phone: String,

        #[field(flatten, prefix = "home_")]
        home: Address,
    }

    #[derive(WebForm, Debug)]
    struct Employee {
        name: String,

        #[field(flatten, prefix = "contact_")]
        contact: Contact,
    }

    let names: Vec<String> = Employee::spec()
        .fields()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(
        names,
        [
            "name",
            "contact_phone",
            "contact_home_street",
            "contact_home_postcode",
            "contact_home_country",
        ]
    );

    let employee = Employee::from_urlencoded(
        "name=Ada&contact_phone=+49123&contact_home_street=Main+1\
         &contact_home_postcode=12345&contact_home_country=de",
    )
    .unwrap();
    assert_eq!(employee.contact.home.postcode, "12345");
}

#[test]
fn a_flattened_form_round_trips_through_fill() {
    let order = Order::from_urlencoded(&order_body()).unwrap();
    let values = order.to_values();

    assert_eq!(values.get("billing_postcode"), Some("12345"));
    assert_eq!(values.get("shipping_postcode"), Some("54321"));

    let again = Order::from_values(&values).unwrap();
    assert_eq!(again.billing, order.billing);
    assert_eq!(again.shipping, order.shipping);
}

#[test]
fn a_sub_form_is_still_a_form_on_its_own() {
    // The whole point of the reuse: `Address` did not have to be written
    // differently to be embeddable.
    let address = Address::from_urlencoded("street=Main+1&postcode=12345&country=de").unwrap();
    assert_eq!(address.street, "Main 1");

    let names: Vec<String> = Address::spec()
        .fields()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names, ["street", "postcode", "country"]);
}
