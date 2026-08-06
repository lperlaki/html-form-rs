//! The axum extractor: `Outcome<T>` straight out of a request.

#![cfg(feature = "axum")]

use axum::body::Body;
use axum::extract::FromRequest;
use axum::http::{Request, StatusCode, header};
use axum::response::IntoResponse;
use html_form::{Form, FormRejection, Outcome};

#[derive(Form, Debug)]
#[form(action = "/signup", method = "post")]
struct Signup {
    #[field(type = "email", label = "Email address")]
    email: String,

    #[field(label = "Age", min = 18, max = 120)]
    age: Option<u32>,
}

fn post(content_type: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/signup")
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body.to_owned()))
        .unwrap()
}

async fn extract(req: Request<Body>) -> Result<Outcome<Signup>, FormRejection> {
    Outcome::from_request(req, &()).await
}

#[tokio::test]
async fn post_urlencoded_body_is_parsed() {
    let req = post(
        "application/x-www-form-urlencoded",
        "email=a%40example.com&age=30",
    );
    let outcome = extract(req).await.unwrap();

    match outcome {
        Outcome::Valid(signup) => {
            assert_eq!(signup.email, "a@example.com");
            assert_eq!(signup.age, Some(30));
        }
        Outcome::Invalid { errors, .. } => panic!("unexpected errors: {errors}"),
    }
}

#[tokio::test]
async fn a_charset_parameter_is_ignored() {
    let req = post(
        "application/x-www-form-urlencoded; charset=utf-8",
        "email=a%40example.com",
    );
    assert!(extract(req).await.unwrap().is_valid());
}

/// Failing validation is not a rejection: the handler still runs, and gets the
/// form back with what the user typed and what was wrong with it.
#[tokio::test]
async fn invalid_input_is_not_a_rejection() {
    let req = post("application/x-www-form-urlencoded", "email=nope&age=7");
    let outcome = extract(req).await.unwrap();

    match outcome {
        Outcome::Valid(_) => panic!("expected the submission to be rejected"),
        Outcome::Invalid { errors, view } => {
            assert_eq!(errors.len(), 2);
            assert_eq!(view.field("email").unwrap().value.as_deref(), Some("nope"));
            assert!(view.field("age").unwrap().errors[0].contains("18"));
        }
    }
}

#[tokio::test]
async fn get_reads_the_query_string() {
    let req = Request::builder()
        .method("GET")
        .uri("/signup?email=a%40example.com&age=30")
        .body(Body::empty())
        .unwrap();

    match extract(req).await.unwrap() {
        Outcome::Valid(signup) => assert_eq!(signup.age, Some(30)),
        Outcome::Invalid { errors, .. } => panic!("unexpected errors: {errors}"),
    }
}

/// A `GET` with nothing in the query is an empty submission, not a rejection —
/// which is what a form submitted with every field left blank looks like.
#[tokio::test]
async fn get_without_a_query_is_an_empty_submission() {
    let req = Request::builder()
        .method("GET")
        .uri("/signup")
        .body(Body::empty())
        .unwrap();

    assert!(!extract(req).await.unwrap().is_valid());
}

#[tokio::test]
async fn a_json_body_is_rejected() {
    let req = post("application/json", r#"{"email":"a@example.com"}"#);
    let rejection = extract(req).await.unwrap_err();

    assert!(matches!(rejection, FormRejection::UnsupportedMediaType));
    assert_eq!(
        rejection.into_response().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}

#[tokio::test]
async fn a_body_without_a_content_type_is_rejected() {
    let req = Request::builder()
        .method("POST")
        .uri("/signup")
        .body(Body::from("email=a%40example.com"))
        .unwrap();

    assert!(matches!(
        extract(req).await.unwrap_err(),
        FormRejection::UnsupportedMediaType
    ));
}

#[tokio::test]
async fn a_non_utf8_body_costs_one_field_its_value_rather_than_the_whole_form() {
    // A stray byte in one field is not a reason to throw a submission away:
    // the body is decoded lossily, and the address next to it survives intact.
    let mut body = b"email=a%40example.com&age=".to_vec();
    body.push(0xff);
    let req = Request::builder()
        .method("POST")
        .uri("/signup")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap();

    let Outcome::Invalid { view, errors } = extract(req).await.unwrap() else {
        panic!("expected the mangled age to be rejected");
    };
    assert_eq!(errors.len(), 1);
    assert!(errors.has_field("age"));
    assert_eq!(
        view.field("email").unwrap().value.as_deref(),
        Some("a@example.com")
    );
}
