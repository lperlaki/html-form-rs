#![allow(dead_code)]

//! Custom attributes: markup the crate has no opinion about, carried from the
//! derive through the spec and the view into the HTML.

use html_form::{Attr, Form};

#[derive(Form)]
#[form(action = "/search")]
#[form(attr("hx-post" = "/search", "data-turbo" = "false"))]
struct Search {
    #[field(
        label = "Query",
        attr("hx-trigger" = "keyup changed delay:300ms", "data-index" = 3, autocorrect = "off")
    )]
    query: String,

    #[field(type = "select", attr(inert))]
    #[option("new", "New")]
    #[option("old", "Old")]
    sort: Option<String>,

    #[field(label = "Notes", type = "textarea")]
    notes: Option<String>,
}

#[test]
fn custom_attributes_reach_the_spec() {
    let spec = Search::spec();
    assert_eq!(
        spec.attrs,
        [
            Attr::new("hx-post", "/search"),
            Attr::new("data-turbo", "false"),
        ]
    );

    // Non-string literals are flattened the same way `min = 18` is.
    assert_eq!(
        spec.field("query").unwrap().spec.attrs[1],
        Attr::new("data-index", "3")
    );
    // A bare word is a boolean attribute.
    assert_eq!(
        spec.field("sort").unwrap().spec.attrs,
        [Attr::flag("inert")]
    );
    assert!(spec.field("notes").unwrap().spec.attrs.is_empty());
}

#[test]
fn custom_attributes_are_rendered_in_declaration_order() {
    let html = Search::render().to_html();

    assert!(
        html.contains(r#"method="post" class="html-form" hx-post="/search" data-turbo="false">"#)
    );
    assert!(html.contains(
        r#"<input type="text" name="query" id="query" required hx-trigger="keyup changed delay:300ms" data-index="3" autocorrect="off">"#
    ));
    assert!(html.contains(r#"<select name="sort" id="sort" inert>"#));
}

#[test]
fn custom_attribute_values_are_escaped() {
    #[derive(Form)]
    struct Risky {
        #[field(attr("data-payload" = r#"" onload="x"#))]
        name: String,
    }

    let html = Risky::render().to_html();
    assert!(!html.contains(r#"onload="x""#));
    assert!(html.contains(r#"data-payload="&quot; onload=&quot;x""#));
}

#[test]
fn the_view_carries_them_for_template_engines() {
    let view = Search::render();
    let json = serde_json::to_value(&view).unwrap();

    assert_eq!(json["attrs"][0]["name"], "hx-post");
    assert_eq!(json["attrs"][0]["value"], "/search");
    // A boolean attribute has no value.
    assert!(json["fields"][1]["attrs"][0]["value"].is_null());
}

#[test]
fn a_custom_attribute_can_be_set_at_render_time() {
    let mut view = Search::render();
    view.set_attr("hx-post", Some("/search?page=2"));
    view.set_attr("data-page", Some("2"));
    view.field_mut("query").unwrap().set_attr("inert", None);

    let html = view.to_html();
    // Replacing keeps the attribute where it was declared.
    assert!(html.contains(r#"hx-post="/search?page=2" data-turbo="false" data-page="2">"#));
    assert!(html.contains(r#"autocorrect="off" inert>"#));
}

#[test]
fn every_field_of_a_flattened_form_keeps_its_own_attributes() {
    #[derive(Form)]
    struct Address {
        #[field(attr("data-autofill" = "street"))]
        street: String,
    }

    #[derive(Form)]
    struct Order {
        #[field(flatten, prefix = "billing_")]
        billing: Address,
    }

    let view = Order::render();
    let street = view.field("billing_street").unwrap();
    assert_eq!(street.attrs[0].name, "data-autofill");
    assert_eq!(street.attrs[0].value.as_deref(), Some("street"));
}
