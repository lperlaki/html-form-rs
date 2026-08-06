#![allow(dead_code)]

//! Enums as `<select>`s, custom scalar types, and adjusting a view at runtime.

use std::borrow::Cow;

use web_form::{
    Choice, Control, ErrorKind, FieldKind, FormChoice, FormErrors, FormValue, ValueError, Values,
    WebForm,
};

#[derive(FormChoice, Debug, PartialEq)]
enum Plan {
    Free,
    #[choice(value = "pro", label = "Professional")]
    Pro,
    #[choice(group = "Contact sales", disabled)]
    SelfHosted,
}

#[derive(WebForm, Debug)]
struct Subscription {
    #[field(label = "Plan")]
    plan: Plan,

    #[field(label = "Add-ons")]
    addons: Vec<Plan>,

    #[field(label = "Downgrade to")]
    downgrade: Option<Plan>,

    #[field(type = "radio", label = "Billing period")]
    #[option("monthly", "Monthly")]
    #[option("yearly", "Yearly")]
    period: String,

    #[field(type = "checkbox", label = "Notify me about")]
    notify: Vec<Plan>,
}

#[test]
fn an_enum_becomes_a_select_with_its_variants_as_options() {
    assert_eq!(<Plan as FormValue>::CONTROL.kind(), FieldKind::Select);

    let view = Subscription::render();
    let plan = view.field("plan").unwrap();

    assert_eq!(plan.kind, FieldKind::Select);
    assert_eq!(plan.choices.len(), 3);
    // Variant names become kebab-case values and spaced labels…
    assert_eq!(plan.choices[0].value, "free");
    assert_eq!(plan.choices[0].label, "Free");
    assert_eq!(plan.choices[2].value, "self-hosted");
    assert_eq!(plan.choices[2].label, "Self Hosted");
    // …unless the variant says otherwise.
    assert_eq!(plan.choices[1].value, "pro");
    assert_eq!(plan.choices[1].label, "Professional");
    assert!(plan.choices[2].disabled);
    assert_eq!(plan.choices[2].group.as_deref(), Some("Contact sales"));
}

#[test]
fn an_enum_field_parses_and_rejects_by_value() {
    let form = Subscription::from_urlencoded("plan=pro&period=monthly").unwrap();
    assert_eq!(form.plan, Plan::Pro);
    assert_eq!(form.downgrade, None);
    assert!(form.addons.is_empty());

    let errors = Subscription::from_urlencoded("plan=platinum&period=monthly").unwrap_err();
    assert!(matches!(
        errors.field("plan").next().unwrap().kind,
        ErrorKind::NotAChoice
    ));
}

#[test]
fn a_vec_of_enums_is_a_multiple_select() {
    let view = Subscription::render();
    assert!(view.field("addons").unwrap().multiple);
    assert!(!view.field("addons").unwrap().required);

    let form =
        Subscription::from_urlencoded("plan=free&period=yearly&addons=pro&addons=free").unwrap();
    assert_eq!(form.addons, [Plan::Pro, Plan::Free]);

    let html = view.to_html();
    assert!(html.contains(r#"<select name="addons" id="addons" multiple>"#));
}

#[test]
fn a_radio_group_labels_each_option() {
    let html = Subscription::render().to_html();

    assert!(html.contains(r#"<input type="radio" name="period" id="period-0""#));
    assert!(html.contains(r#"value="yearly""#));
    // The group caption is not a `<label for=…>` pointing at only the first
    // radio; it names the group instead.
    assert!(html.contains(r#"<span class="web-form__label" id="period-label">Billing period"#));
    assert!(html.contains(r#"role="radiogroup" aria-labelledby="period-label""#));
    // …and it comes before the options it captions.
    assert!(html.find("period-label").unwrap() < html.find("radiogroup").unwrap());
}

#[test]
fn a_selected_radio_survives_a_failed_submission() {
    let view = Subscription::submit_urlencoded("plan=nope&period=yearly")
        .view()
        .unwrap();

    let period = view.field("period").unwrap();
    assert!(
        period
            .choices
            .iter()
            .find(|c| c.value == "yearly")
            .unwrap()
            .selected
    );
    assert!(view.to_html().contains(r#"value="yearly" checked"#));
}

#[test]
fn a_checkbox_group_is_one_box_per_option() {
    let view = Subscription::render();
    let notify = view.field("notify").unwrap();

    assert_eq!(notify.kind, FieldKind::CheckboxGroup);
    assert_eq!(notify.choices.len(), 3);
    // A checkbox group is multi-valued whatever the field is typed as.
    assert!(notify.multiple);

    let html = view.to_html();
    assert!(html.contains(r#"<input type="checkbox" name="notify" id="notify-0""#));
    assert!(html.contains(r#"id="notify-2" value="self-hosted" disabled>"#));
    // Captioned like a radio group rather than labelled through the first box.
    assert!(html.contains(r#"<span class="web-form__label" id="notify-label">Notify me about"#));
    assert!(html.contains(r#"role="group" aria-labelledby="notify-label""#));
    assert!(html.find("notify-label").unwrap() < html.find(r#"role="group""#).unwrap());
}

#[test]
fn a_checkbox_group_collects_every_box_ticked() {
    let form =
        Subscription::from_urlencoded("plan=free&period=yearly&notify=pro&notify=free").unwrap();
    assert_eq!(form.notify, [Plan::Pro, Plan::Free]);

    // An absent group means "nothing ticked", not "fall back to the default".
    let form = Subscription::from_urlencoded("plan=free&period=yearly").unwrap();
    assert!(form.notify.is_empty());
}

#[test]
fn ticked_boxes_survive_a_failed_submission() {
    let view = Subscription::submit_urlencoded("plan=nope&period=yearly&notify=pro")
        .view()
        .unwrap();

    let ticked: Vec<&str> = view
        .field("notify")
        .unwrap()
        .choices
        .iter()
        .filter(|c| c.selected)
        .map(|c| c.value.as_ref())
        .collect();
    assert_eq!(ticked, ["pro"]);
    assert!(view.to_html().contains(r#"value="pro" checked"#));
}

#[derive(WebForm, Debug)]
struct Interests {
    #[field(type = "checkbox", required, label = "Topics")]
    #[option("news", "News")]
    #[option("offers", "Offers")]
    topics: Vec<String>,
}

#[test]
fn a_required_checkbox_group_is_enforced_by_the_server_not_the_browser() {
    let html = Interests::render().to_html();

    // `required` on a checkbox would mean "tick *this* box", which is not what
    // a required group asks for.
    assert!(!html.contains(" required"));
    assert!(html.contains(r#"role="group" aria-labelledby="topics-label" aria-required="true""#));

    assert!(Interests::from_urlencoded("").is_err());
    let form = Interests::from_urlencoded("topics=news").unwrap();
    assert_eq!(form.topics, ["news"]);
}

#[test]
fn the_enum_round_trips_back_out() {
    let form =
        Subscription::from_urlencoded("plan=self-hosted&period=monthly&downgrade=free").unwrap();
    let values = form.to_values();

    assert_eq!(values.get("plan"), Some("self-hosted"));
    assert_eq!(values.get("downgrade"), Some("free"));
}

// ─── A hand-written scalar ────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
struct Slug(String);

impl FormValue for Slug {
    const CONTROL: Control = Control::TEXT;

    fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
        if !raw.is_empty() && raw.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
            Ok(Slug(raw.to_owned()))
        } else {
            Err(ValueError::new("lowercase letters and dashes"))
        }
    }

    fn to_form_value(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.0)
    }
}

#[derive(WebForm, Debug)]
struct Article {
    #[field(label = "URL slug", placeholder = "my-article")]
    slug: Slug,
}

#[test]
fn a_custom_form_value_plugs_straight_in() {
    assert_eq!(
        Article::render().field("slug").unwrap().kind,
        FieldKind::Text
    );

    let article = Article::from_urlencoded("slug=hello-world").unwrap();
    assert_eq!(article.slug, Slug("hello-world".into()));

    let errors = Article::from_urlencoded("slug=Hello_World").unwrap_err();
    assert_eq!(
        errors.field("slug").next().unwrap().message,
        "Enter lowercase letters and dashes."
    );
}

// ─── Runtime adjustments to a view ────────────────────────────────────────────

struct Room {
    id: String,
    name: String,
}

#[derive(WebForm, Debug)]
struct Booking {
    #[field(type = "select", label = "Room")]
    room: String,
}

#[test]
fn options_that_only_exist_at_runtime_can_be_filled_in_before_rendering() {
    // Options that were not known when the form was declared.
    let rooms: Vec<Room> = vec![
        Room {
            id: "oak".into(),
            name: "OAK".into(),
        },
        Room {
            id: "birch".into(),
            name: "BIRCH".into(),
        },
    ];
    let rooms = rooms.iter().map(|r| Choice::owned(&r.id, &r.name));

    let mut view = Booking::render();
    view.field_mut("room").unwrap().set_choices(rooms);

    let html = view.to_html();
    assert!(html.contains(r#"<option value="oak">OAK</option>"#));
    assert!(html.contains(r#"<option value="birch">BIRCH</option>"#));
}

#[test]
fn a_selection_survives_choices_being_replaced() {
    let submitted = Values::parse("room=birch");
    let mut view = Booking::render_with(&submitted, &FormErrors::new());
    view.field_mut("room")
        .unwrap()
        .set_choices([Choice::new("oak", "Oak"), Choice::new("birch", "Birch")]);

    let birch = view
        .field("room")
        .unwrap()
        .choices
        .iter()
        .find(|c| c.value == "birch")
        .unwrap();
    assert!(birch.selected);
}

#[test]
fn errors_only_the_database_knows_about_can_be_added_afterwards() {
    let mut view = Booking::render();
    assert!(!view.has_errors);

    assert!(view.add_field_error("room", "That room is already booked."));
    assert!(!view.add_field_error("nonexistent", "…"));

    assert!(view.has_errors);
    assert!(view.field("room").unwrap().has_errors);
    assert!(
        view.to_html()
            .contains("<li>That room is already booked.</li>")
    );
}

#[test]
fn dump() {
    println!("{}", Subscription::render().to_html());
    println!("{}", Interests::render().to_html());
}
