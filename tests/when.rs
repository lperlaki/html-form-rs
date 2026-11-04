//! Fields that apply only while another field holds a value.

use html_form::{ErrorKind, Form, Outcome};

fn is_port(port: &Option<u16>) -> bool {
    port.is_some_and(|port| port > 0)
}

#[derive(Form, Debug)]
struct Interface {
    #[field(default = "dhcp")]
    #[option("dhcp", "DHCP")]
    #[option("static", "Static")]
    mode: String,

    #[field(required, pattern = "[0-9.]+", when(field = "mode", is_one_of = ["static"]))]
    addresses: Vec<String>,

    #[field(required, when(field = "mode", is_one_of = ["static"]))]
    gateway: Option<String>,

    #[field(validate = is_port, when(field = "mode", is_one_of = ["static"]))]
    port: Option<u16>,

    tunnel: bool,

    #[field(when(field = "tunnel", is_one_of = ["on"]))]
    peer: Option<String>,
}

#[test]
fn a_field_that_does_not_apply_is_neither_parsed_nor_validated() {
    let form = Interface::from_urlencoded("mode=dhcp&addresses=not-an-address&gateway=x&port=nope")
        .unwrap();
    assert!(form.addresses.is_empty());
    assert_eq!(form.gateway, None);
    assert_eq!(form.port, None);
}

#[test]
fn a_field_that_applies_is_required_like_any_other() {
    let errors = Interface::from_urlencoded("mode=static&addresses=&port=1").unwrap_err();
    assert_eq!(
        errors.field("addresses").next().unwrap().kind,
        ErrorKind::Required
    );
    assert_eq!(
        errors.field("gateway").next().unwrap().kind,
        ErrorKind::Required
    );
}

/// A custom check does not run on what a field that does not apply holds.
#[test]
fn a_validator_only_runs_while_its_field_applies() {
    assert!(Interface::from_urlencoded("mode=dhcp").is_ok());
    let errors = Interface::from_urlencoded("mode=static&addresses=1.1.1.1&gateway=x").unwrap_err();
    assert!(errors.has_field("port"));
}

#[test]
fn a_box_decides_as_a_browser_posts_it_and_as_a_record_fills_it_in() {
    let form = Interface::from_urlencoded("mode=dhcp&tunnel=on&peer=a").unwrap();
    assert_eq!(form.peer.as_deref(), Some("a"));
    assert_eq!(
        Interface::from_urlencoded("mode=dhcp&peer=a").unwrap().peer,
        None
    );

    // `render_filled` writes `true` for a ticked box, not `on`.
    assert!(form.render_filled().field("peer").unwrap().applies);
    let parsed = Interface::from_values(&form.to_values()).unwrap();
    assert_eq!(parsed.peer.as_deref(), Some("a"));
}

#[test]
fn the_view_goes_by_the_default_on_a_blank_form_and_by_the_values_after() {
    let blank = Interface::render();
    let field = blank.field("gateway").unwrap();
    assert!(!field.applies);
    let when = field.when.as_ref().unwrap();
    assert_eq!(when.field, "mode");
    assert_eq!(when.is_one_of, ["static"]);

    let Outcome::Invalid { view, .. } = Interface::submit_urlencoded("mode=static") else {
        panic!("the addresses are missing");
    };
    assert!(view.field("gateway").unwrap().applies);
}

/// Shut, so the browser neither posts it nor enforces its `required`. There, so
/// a script can open it.
#[test]
fn the_markup_shuts_what_does_not_apply() {
    let html = Interface::render().to_html();
    assert!(
        html.contains(
            r#"<fieldset class="html-form__when" data-when-field="mode" data-when-values="static" disabled hidden>"#
        ),
        "{html}"
    );

    let Outcome::Invalid { view, .. } = Interface::submit_urlencoded("mode=static") else {
        panic!("the addresses are missing");
    };
    assert!(view.to_html().contains(
        r#"<fieldset class="html-form__when" data-when-field="mode" data-when-values="static">"#
    ));
}

/// The field that decides sits under the same prefix.
#[test]
fn a_condition_reads_the_field_beside_it_inside_a_flatten() {
    #[derive(Form, Debug)]
    struct Uplink {
        #[field(flatten, prefix = "wan_")]
        wan: Interface,
    }

    let form = Uplink::from_urlencoded("wan_mode=dhcp&wan_addresses=nope").unwrap();
    assert!(form.wan.addresses.is_empty());
    assert!(Uplink::from_urlencoded("wan_mode=static&wan_addresses=nope").is_err());

    let view = Uplink::render();
    let when = view.field("wan_gateway").unwrap().when.as_ref().unwrap();
    assert_eq!(when.field, "wan_mode");
}
