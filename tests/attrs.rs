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

// ─── Constraints from constants ───────────────────────────────────────────────

mod constants {
    use std::ops::RangeInclusive;

    use html_form::Form;

    const MTU_MIN: u32 = 576;
    const MTU_MAX: u32 = 9000;
    const MTU_RANGE: RangeInclusive<i32> = -5..=5;
    const NAME_MAX: usize = 8;
    const HOST: &str = "[a-z]+";

    mod limits {
        pub const STEP: u8 = 4;
    }

    #[derive(Form, Debug)]
    struct Link {
        #[field(min = MTU_MIN, max = MTU_MAX, step = limits::STEP)]
        mtu: u32,
        #[field(min = *MTU_RANGE.start(), max = *MTU_RANGE.end() + 1)]
        offset: i32,
        #[field(minlength = NAME_MAX - 6, maxlength = NAME_MAX, pattern = HOST)]
        name: String,
        #[field(min = -3, max = 3.5)]
        literal: f32,
    }

    #[test]
    fn the_rendered_bounds_equal_the_constants() {
        let view = Link::render();
        let mtu = view.field("mtu").unwrap();
        assert_eq!(mtu.min.as_deref(), Some("576"));
        assert_eq!(mtu.max.as_deref(), Some("9000"));
        assert_eq!(mtu.step.as_deref(), Some("4"));

        let offset = view.field("offset").unwrap();
        assert_eq!(offset.min.as_deref(), Some("-5"));
        assert_eq!(offset.max.as_deref(), Some("6"));

        let name = view.field("name").unwrap();
        assert_eq!(name.minlength, Some(2));
        assert_eq!(name.maxlength, Some(8));
        assert_eq!(name.pattern.as_deref(), Some("[a-z]+"));

        let literal = view.field("literal").unwrap();
        assert_eq!(literal.min.as_deref(), Some("-3"));
        assert_eq!(literal.max.as_deref(), Some("3.5"));
    }

    #[test]
    fn the_server_enforces_them_too() {
        let errors =
            Link::from_urlencoded("mtu=100&offset=-9&name=TOOLONGNAME&literal=9").unwrap_err();
        assert!(errors.has_field("mtu"));
        assert!(errors.has_field("offset"));
        assert!(errors.has_field("name"));
        assert!(errors.has_field("literal"));

        assert!(Link::from_urlencoded("mtu=1500&offset=0&name=wan&literal=1").is_ok());
    }

    #[derive(html_form::FormValue, Debug)]
    #[value(min = MTU_MIN, max = MTU_MAX)]
    struct Mtu(u32);

    #[derive(Form, Debug)]
    struct WithType {
        mtu: Mtu,
    }

    #[test]
    fn a_type_takes_them_as_a_field_does() {
        let view = WithType::render();
        assert_eq!(view.field("mtu").unwrap().min.as_deref(), Some("576"));
        assert!(WithType::from_urlencoded("mtu=100").is_err());
        assert_eq!(WithType::from_urlencoded("mtu=1500").unwrap().mtu.0, 1500);
    }

    /// The extremes of the formatting, which the bounds above do not reach.
    #[test]
    fn every_width_of_integer_formats() {
        use html_form::__private::Int;
        assert_eq!(Int::new(0).as_str(), "0");
        assert_eq!(Int::new(-1).as_str(), "-1");
        assert_eq!(Int::new(i128::MIN).as_str(), i128::MIN.to_string());
        assert_eq!(Int::new(i128::MAX).as_str(), i128::MAX.to_string());
    }
}
