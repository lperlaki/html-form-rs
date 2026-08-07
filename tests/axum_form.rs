//! The other axum extractor: `Form<T, R>`, which rejects with the form itself.

#![cfg(all(feature = "axum", feature = "html"))]

use axum::body::Body;
use axum::extract::{FromRequest, FromRequestParts};
use axum::handler::Handler;
use axum::http::request::Parts;
use axum::http::{Request, StatusCode, header};
use axum::response::{Html, IntoResponse};
use html_form::FormView;
use html_form::axum::{Builtin, Form, Rejection, Renderer};

mod common;
use common::{Page, body_of, post};

#[derive(html_form::Form, Debug)]
#[form(action = "/signup", method = "post")]
struct Signup {
    #[field(type = "email", label = "Email address")]
    email: String,

    #[field(label = "Age", min = 18, max = 120)]
    age: Option<u32>,
}

#[tokio::test]
async fn a_valid_submission_reaches_the_handler() {
    let form: Form<Signup, Page> =
        Form::from_request(post("/signup", "email=a%40example.com&age=30"), &())
            .await
            .unwrap();

    assert_eq!(form.data.email, "a@example.com");
    // The value is reachable without naming the field, and can be taken out.
    assert_eq!(form.age, Some(30));
    assert_eq!(form.into_inner().email, "a@example.com");
}

#[tokio::test]
async fn get_reads_the_query_string() {
    let request = Request::builder()
        .method("GET")
        .uri("/signup?email=a%40example.com&age=30")
        .body(Body::empty())
        .unwrap();

    let form: Form<Signup, Page> = Form::from_request(request, &()).await.unwrap();
    assert_eq!(form.age, Some(30));
}

/// The whole point of this extractor: a failed validation never reaches the
/// handler, and the rejection is already the page to send back.
#[tokio::test]
async fn an_invalid_submission_is_rejected_with_the_rendered_form() {
    let rejection = Form::<Signup, Page>::from_request(post("/signup", "email=nope&age=7"), &())
        .await
        .unwrap_err();

    assert!(matches!(rejection, Rejection::Invalid(_)));

    let response = rejection.into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let html = body_of(response).await;
    assert!(
        html.contains("<h1>Check the form</h1>"),
        "the page around it"
    );
    // What the user typed comes back, and so does every message.
    assert!(html.contains(r#"value="nope""#));
    assert!(html.contains("18"), "the message on `age`");
}

/// The rejection is not a response yet. It is the view and the errors, and the
/// renderer runs only where a response is asked for.
#[tokio::test]
async fn the_rejection_is_still_a_form_until_it_is_asked_for_a_response() {
    let rejection = Form::<Signup, Page>::from_request(post("/signup", "email=nope&age=7"), &())
        .await
        .unwrap_err();

    let Rejection::Invalid(invalid) = rejection else {
        panic!("the submission was whole, and it failed")
    };

    // Both problems, as errors rather than as markup.
    assert_eq!(invalid.errors.len(), 2);
    assert!(invalid.errors.has_field("age"));
    assert_eq!(
        invalid.view.field("email").unwrap().value.as_deref(),
        Some("nope")
    );

    // The page is built here, and not before. The status is the rejection's,
    // and the renderer never had one to give.
    let response = invalid.into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(body_of(response).await.contains("<h1>Check the form</h1>"));
}

#[tokio::test]
async fn the_builtin_renderer_answers_with_the_forms_own_html() {
    let rejection = Form::<Signup, Builtin>::from_request(post("/signup", "email=nope"), &())
        .await
        .unwrap_err();

    let response = rejection.into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    assert!(
        body_of(response)
            .await
            .contains(r#"<form action="/signup""#)
    );
}

/// A request with no submission in it is the same rejection as for
/// `Outcome<T>`, passed on with the status it came with.
#[tokio::test]
async fn a_request_that_carries_no_submission_is_a_request_rejection() {
    let request = Request::builder()
        .method("POST")
        .uri("/signup")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"email":"a@example.com"}"#))
        .unwrap();

    let rejection = Form::<Signup, Page>::from_request(request, &())
        .await
        .unwrap_err();

    assert!(matches!(
        rejection,
        Rejection::Request(html_form::FormRejection::UnsupportedMediaType)
    ));
    assert_eq!(
        rejection.into_response().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}

/// An axum handler takes it as its last argument, and the rejection is what the
/// route answers with. Nothing in the handler says so.
#[tokio::test]
async fn a_handler_takes_it_and_the_rejection_becomes_the_response() {
    async fn signup(form: Form<Signup, Page>) -> Html<String> {
        Html(format!("Welcome, {}!", form.data.email))
    }

    let welcome = signup
        .call(post("/signup", "email=a%40example.com&age=30"), ())
        .await;
    assert_eq!(welcome.status(), StatusCode::OK);
    assert_eq!(body_of(welcome).await, "Welcome, a@example.com!");

    let rejected = signup.call(post("/signup", "email=nope&age=7"), ()).await;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert!(body_of(rejected).await.contains("Check the form"));
}

/// The same type is the response, and the same renderer builds it: a handler
/// that returns one shows the form filled in from the value.
#[tokio::test]
async fn a_returned_form_is_the_form_filled_in_from_the_value() {
    let signup = Signup {
        email: "ada@example.com".to_owned(),
        age: Some(36),
    };

    let response = Form::<_, Page>::new(signup).into_response();
    let html = body_of(response).await;

    assert!(html.contains("<h1>Check the form</h1>"), "the same page");
    assert!(html.contains(r#"value="ada@example.com""#));
    assert!(html.contains(r#"value="36""#));
    // Nothing failed, so nothing is marked as having failed.
    assert!(!html.contains("aria-invalid"));
}

/// A form nothing rejected is a plain `200`, because a renderer builds a page
/// and never a status. The same renderer answers `400` under a rejection.
#[tokio::test]
async fn a_returned_form_carries_no_status_of_its_own() {
    let form = || {
        Form::<_, Page>::new(Signup {
            email: "ada@example.com".to_owned(),
            age: None,
        })
    };

    assert_eq!(form().into_response().status(), StatusCode::OK);
    // And a handler that wants another one still says so.
    assert_eq!(
        (StatusCode::CREATED, form()).into_response().status(),
        StatusCode::CREATED
    );
}

/// Taken in and handed back: a handler may round-trip the extractor.
#[tokio::test]
async fn a_handler_can_take_one_and_return_one() {
    async fn preview(form: Form<Signup, Page>) -> Form<Signup, Page> {
        form
    }

    let response = preview
        .call(post("/signup", "email=a%40example.com&age=30"), ())
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_of(response).await.contains(r#"value="a@example.com""#));
}

// A form whose context is an extractor of its own. This is what `Outcome<T>`
// cannot do: the session reaches the default, the validation and the re-render.

#[derive(Debug)]
struct Session {
    user: String,
}

impl<S: Send + Sync> FromRequestParts<S> for Session {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("x-user")
            .and_then(|user| user.to_str().ok())
            .map(|user| Session {
                user: user.to_owned(),
            })
            .ok_or((StatusCode::UNAUTHORIZED, "no session"))
    }
}

#[derive(html_form::Form, Debug)]
#[form(context = Session, action = "/comment", method = "post")]
struct Comment {
    #[field(type = "hidden", default = issued_token)]
    csrf: String,

    #[field(type = "textarea", label = "Comment", minlength = 3)]
    body: String,
}

fn issued_token(session: &Session) -> String {
    format!("token-for-{}", session.user)
}

/// The renderer reads the context, so the page it builds knows who was asking.
struct Greeting;

impl Renderer<Comment> for Greeting {
    fn render(view: FormView, session: &Session) -> impl IntoResponse {
        Html(format!("<p>Sorry, {}</p>{}", session.user, view.to_html()))
    }
}

fn comment(body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/comment")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("x-user", "ada")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test]
async fn the_context_is_extracted_from_the_request() {
    let form: Form<Comment, Greeting> = Form::from_request(comment("csrf=t&body=hello"), &())
        .await
        .unwrap();

    assert_eq!(form.body, "hello");
}

#[tokio::test]
async fn the_renderer_and_the_defaults_both_see_the_context() {
    let rejection = Form::<Comment, Greeting>::from_request(comment("csrf=t&body=no"), &())
        .await
        .unwrap_err();

    let html = body_of(rejection.into_response()).await;
    // The renderer read the session.
    assert!(html.contains("<p>Sorry, ada</p>"));
    // So did the field default: the hidden token is minted again for the page
    // the user gets back, rather than echoed out of the submission.
    assert!(html.contains(r#"value="token-for-ada""#));
    // What the user typed is still there, with the message on it.
    assert!(html.contains(">no</textarea>"));
}

/// A context that turns the request down stops the extractor before the body,
/// and its rejection goes on as itself rather than as a response.
#[tokio::test]
async fn a_context_that_rejects_stops_the_extractor() {
    let request = Request::builder()
        .method("POST")
        .uri("/comment")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("csrf=t&body=hello"))
        .unwrap();

    let rejection = Form::<Comment, Greeting>::from_request(request, &())
        .await
        .unwrap_err();

    // `Session::Rejection` is a pair, and this is still that pair.
    let Rejection::Context((status, message)) = rejection else {
        panic!("there is no session on this request")
    };
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(message, "no session");

    let response = Rejection::<Comment, Greeting, _>::Context((status, message)).into_response();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_of(response).await, "no session");
}

/// The same again for a field that resets without being hidden. The extractor
/// is the only path the browser ever takes, so this is where it has to hold.
#[derive(html_form::Form, Debug)]
#[form(action = "/order", method = "post", context = Session)]
struct Order {
    #[field(label = "Reference", default = issued_token, reset)]
    reference: String,

    #[field(label = "Seats", min = 1, max = 4)]
    seats: u32,
}

#[tokio::test]
async fn a_visible_field_that_resets_is_minted_again_for_the_rejected_page() {
    let request = Request::builder()
        .method("POST")
        .uri("/order")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("x-user", "ada")
        .body(Body::from("reference=mine&seats=9"))
        .unwrap();

    let rejection = Form::<Order, Builtin>::from_request(request, &())
        .await
        .unwrap_err();

    let html = body_of(rejection.into_response()).await;
    assert!(!html.contains("mine"), "the submitted reference came back");
    assert!(html.contains(r#"value="token-for-ada""#));
    // The field beside it keeps what the user typed, and its message.
    assert!(html.contains(r#"value="9""#));
    assert!(html.contains("4 or less"));
}
