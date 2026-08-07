#![allow(dead_code)]

//! `Values` through serde: the same form served to a JSON client and to a
//! browser, with the submission the only thing that differed.

use html_form::{Form, Values};
use serde_json::json;

#[derive(Form, Debug)]
struct Signup {
    #[field(type = "email")]
    email: String,
    age: Option<u32>,
    newsletter: bool,
    tags: Vec<String>,
}

#[test]
fn an_object_is_a_submission() {
    let values: Values = serde_json::from_value(json!({
        "email": "ada@example.com",
        "age": 36,
        "newsletter": true,
        "tags": ["rust", "forms"],
    }))
    .unwrap();

    let signup = Signup::from_values(&values).unwrap();
    assert_eq!(signup.email, "ada@example.com");
    assert_eq!(signup.age, Some(36));
    assert!(signup.newsletter);
    assert_eq!(signup.tags, ["rust", "forms"]);
}

#[test]
fn a_json_value_is_a_string_whatever_the_client_typed_it_as() {
    let typed: Values = serde_json::from_str(r#"{"age": 36, "newsletter": true}"#).unwrap();
    let quoted: Values = serde_json::from_str(r#"{"age": "36", "newsletter": "true"}"#).unwrap();
    assert_eq!(typed, quoted);

    let floats: Values = serde_json::from_str(r#"{"weight": 1.5}"#).unwrap();
    assert_eq!(floats.get("weight"), Some("1.5"));

    // A negative integer is still a signed `i64` to `serde_json`, not the
    // `u64` a positive one deserializes as.
    let negative: Values = serde_json::from_str(r#"{"balance": -5}"#).unwrap();
    assert_eq!(negative.get("balance"), Some("-5"));
}

#[test]
fn null_is_no_value_rather_than_an_empty_one() {
    let values: Values = serde_json::from_str(r#"{"email": "a@b.com", "age": null}"#).unwrap();
    assert!(!values.contains("age"));

    // Which is what an absent field means everywhere else: `Option` sees
    // nothing, and a field with a default falls back to it.
    let signup = Signup::from_values(&values).unwrap();
    assert_eq!(signup.age, None);

    // A client clearing a field sends the empty string, as the browser does.
    let cleared: Values = serde_json::from_str(r#"{"email": "a@b.com", "age": ""}"#).unwrap();
    assert!(cleared.contains("age"));
    assert_eq!(Signup::from_values(&cleared).unwrap().age, None);
}

#[test]
fn a_list_is_a_name_submitted_repeatedly() {
    let values: Values = serde_json::from_str(r#"{"tag": ["x", "y"], "one": ["only"]}"#).unwrap();
    assert_eq!(values.all("tag").collect::<Vec<_>>(), ["x", "y"]);
    assert_eq!(values.len(), 3);

    // An empty list is a name with nothing under it, so it is not submitted.
    let empty: Values = serde_json::from_str(r#"{"tag": []}"#).unwrap();
    assert!(empty.is_empty());
}

#[test]
fn a_list_of_pairs_is_the_other_way_to_write_one() {
    let values: Values =
        serde_json::from_str(r#"[["tag", "x"], ["email", "a@b.com"], ["tag", "y"]]"#).unwrap();

    assert_eq!(values.get("email"), Some("a@b.com"));
    assert_eq!(values.all("tag").collect::<Vec<_>>(), ["x", "y"]);
    // Unlike an object, this keeps every repeat exactly where it was.
    assert_eq!(values, Values::parse("tag=x&email=a%40b.com&tag=y"));
}

#[test]
fn a_submission_serialises_as_the_object_it_came_from() {
    let values = Values::parse("email=a%40b.com&tag=x&tag=y");
    assert_eq!(
        serde_json::to_value(&values).unwrap(),
        json!({"email": "a@b.com", "tag": ["x", "y"]})
    );

    // A single value is a string, not a list of one.
    let one = Values::parse("tag=x");
    assert_eq!(serde_json::to_value(&one).unwrap(), json!({"tag": "x"}));
}

#[test]
fn names_keep_the_order_they_were_submitted_in() {
    let values = Values::parse("z=1&a=2&z=3&m=4");
    let json = serde_json::to_string(&values).unwrap();
    // A repeated name is written once, where its first value stood.
    assert_eq!(json, r#"{"z":["1","3"],"a":"2","m":"4"}"#);
}

#[test]
fn a_form_round_trips_through_json() {
    let signup = Signup {
        email: "ada@example.com".to_owned(),
        age: Some(36),
        newsletter: true,
        tags: vec!["rust".to_owned(), "forms".to_owned()],
    };

    let json = serde_json::to_string(&signup.to_values()).unwrap();
    let values: Values = serde_json::from_str(&json).unwrap();
    let back = Signup::from_values(&values).unwrap();

    assert_eq!(back.email, signup.email);
    assert_eq!(back.age, signup.age);
    assert!(back.newsletter);
    assert_eq!(back.tags, signup.tags);
}

#[test]
fn an_invalid_submission_is_still_reported_field_by_field() {
    let values: Values = serde_json::from_value(json!({"email": "nope", "age": 7})).unwrap();
    let errors = Signup::from_values(&values).unwrap_err();

    assert!(errors.has_field("email"));
    // And serialises for the client that sent JSON in the first place.
    assert_eq!(
        serde_json::to_value(&errors).unwrap(),
        json!({"form": [], "fields": {"email": ["Enter a valid email address."]}})
    );
}

#[test]
fn a_nested_object_says_what_to_write_instead() {
    let error = serde_json::from_str::<Values>(r#"{"billing": {"street": "Main 1"}}"#).unwrap_err();
    assert!(error.to_string().contains("flatten"));
}
