//! `#[form(renderer = ...)]`: the form is the extractor and the response.

#![cfg(all(feature = "axum", feature = "html"))]

use axum::body::Body;
use axum::extract::{FromRequest, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{Request, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use html_form::FormView;
use std::marker::PhantomData;

use html_form::axum::{AsRenderer, HasRenderer, IntoRenderer, Rejection, Renderer};

mod common;
use common::{Page, body_of, post};

#[derive(html_form::Form, Debug)]
#[form(action = "/signup", method = "post", renderer = Page)]
struct Signup {
    #[field(type = "email", label = "Email address")]
    email: String,

    #[field(label = "Age", min = 18, max = 120)]
    age: Option<u32>,
}

#[tokio::test]
async fn the_struct_itself_is_the_extractor() {
    let signup = Signup::from_request(post("/signup", "email=a%40example.com&age=30"), &())
        .await
        .unwrap();

    assert_eq!(signup.email, "a@example.com");
    assert_eq!(signup.age, Some(30));
}

/// The whole point: a handler that names the struct never sees a failure, and
/// the rejection is already the page the renderer built.
#[tokio::test]
async fn a_failed_submission_is_rejected_with_the_named_renderer() {
    let rejection = Signup::from_request(post("/signup", "email=nope&age=7"), &())
        .await
        .unwrap_err();

    let Rejection::Invalid(invalid) = rejection else {
        panic!("the body was a form, and it validated badly")
    };
    assert_eq!(invalid.errors.len(), 2);

    let response = invalid.into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let html = body_of(response).await;
    assert!(html.contains("<h1>Check the form</h1>"));
    // The re-render carries what was typed.
    assert!(html.contains(r#"value="nope""#));
}

/// The derive makes a renderer of its own, named after the form, because a
/// function has no type anybody can write down. It is what a rejection written
/// out is named under, and it is zero-sized, as every renderer is.
#[test]
fn the_derive_names_the_renderer_it_generated() {
    fn renders<R: Renderer<Signup>>() -> usize {
        size_of::<R>()
    }

    assert_eq!(renders::<SignupRenderer>(), 0);

    // The type a handler would write out to map the rejection itself.
    type Rejected = Rejection<Signup, SignupRenderer, std::convert::Infallible>;
    let _: fn(Rejected) -> Response = IntoResponse::into_response;
}

/// `HasRenderer` is what a bound asks for when it means "a form that knows how
/// it is rendered". It names the renderer, and it makes the page without the
/// status either of the two impls above would have put on it.
#[tokio::test]
async fn a_form_that_declared_a_renderer_can_be_asked_for_its_page() {
    fn edit<T: HasRenderer<Context = ()>>(record: &T) -> Response {
        record.render_form()
    }

    let signup = Signup {
        email: "ada@example.com".to_owned(),
        age: Some(36),
    };

    // The same renderer the extractor rejects through.
    let () = assert_renders::<Signup, SignupRenderer>();

    let html = body_of(edit(&signup)).await;
    assert!(html.contains("<h1>Check the form</h1>"));
    assert!(html.contains(r#"value="ada@example.com""#));

    // The status is the response's to decide, not the render's. `Note` says
    // `201`, and its page says nothing.
    let note = Note {
        body: "Hello".to_owned(),
    };
    assert_eq!(note.render_form().status(), StatusCode::OK);
}

/// The associated type is the struct the derive generated, and nothing else.
fn assert_renders<T, R>()
where
    T: HasRenderer<Renderer = R>,
    R: Renderer<T>,
{
}

/// Each of the three shapes `renderer = ...` accepts carries nothing: a
/// renderer is a type and never a value, because `Renderer::render` takes no
/// `self`. That is what lets `render_view` take the value the attribute named
/// by `self` and cost nothing for it.
#[test]
fn every_shape_a_renderer_takes_holds_nothing() {
    fn size<T, R, M>(_renderer: R) -> usize
    where
        T: html_form::Form,
        R: IntoRenderer<T, M>,
    {
        size_of::<R>()
    }

    // A `Renderer` reaches its own impl, under the `AsRenderer` marker.
    fn is_page<T: html_form::Form>(_: PhantomData<Page>)
    where
        Page: IntoRenderer<T, AsRenderer>,
    {
    }
    is_page::<Signup>(PhantomData);

    // A unit struct, a plain function and one that takes the context.
    assert_eq!(size::<Signup, _, _>(Page), 0);
    assert_eq!(size::<Note, _, _>(plain), 0);
    assert_eq!(size::<Comment, _, _>(with_session), 0);
}

/// A body that was never a form is the same rejection it always was.
#[tokio::test]
async fn a_body_that_is_not_a_form_is_unsupported_media_type() {
    let request = Request::builder()
        .method("POST")
        .uri("/signup")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();

    let rejection = Signup::from_request(request, &()).await.unwrap_err();
    assert!(matches!(rejection, Rejection::Request(_)));
    assert_eq!(
        rejection.into_response().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}

/// The other half: the struct is a response, and it goes through the same
/// renderer, so the edit page and the failed submission look alike.
#[tokio::test]
async fn the_struct_itself_is_the_response() {
    let signup = Signup {
        email: "ada@example.com".to_owned(),
        age: Some(36),
    };

    let response = signup.into_response();
    assert_eq!(response.status(), StatusCode::OK);

    let html = body_of(response).await;
    assert!(html.contains("<h1>Check the form</h1>"));
    assert!(html.contains(r#"value="ada@example.com""#));
}

/// Both impls are what a handler signature asks for, and nothing else is
/// written at the call site.
#[tokio::test]
async fn a_handler_takes_the_form_and_returns_one() {
    use axum::handler::Handler;

    async fn signup(mut form: Signup) -> Signup {
        form.email = form.email.to_lowercase();
        form
    }

    let response = signup
        .call(post("/signup", "email=ADA%40example.com&age=36"), ())
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        body_of(response)
            .await
            .contains(r#"value="ada@example.com""#)
    );

    // And the failure never reached it.
    let response = signup.call(post("/signup", "email=nope"), ()).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ─── A function in place of a `Renderer` ─────────────────────────────────────

fn plain(view: FormView) -> Html<String> {
    Html(format!("<main>{}</main>", view.to_html()))
}

#[derive(html_form::Form, Debug)]
#[form(renderer = plain, status = 201)]
struct Note {
    body: String,
}

#[tokio::test]
async fn a_function_stands_in_for_a_renderer() {
    let rejection = Note::from_request(post("/notes", ""), &())
        .await
        .unwrap_err();
    let html = body_of(rejection.into_response()).await;
    assert!(html.starts_with("<main>"));
}

/// A closure is a renderer too. It captures nothing — there is nothing in scope
/// at a struct declaration to capture — so it is the same zero-sized decision a
/// function path is.
#[derive(html_form::Form, Debug)]
#[form(renderer = |view: FormView| Html(format!("<aside>{}</aside>", view.to_html())))]
struct Tag {
    name: String,
}

#[tokio::test]
async fn a_closure_stands_in_for_a_renderer() {
    let rejection = Tag::from_request(post("/tags", ""), &()).await.unwrap_err();
    let html = body_of(rejection.into_response()).await;
    assert!(html.starts_with("<aside>"));
}

/// `status` is the status of the response the *form* makes. The rejection keeps
/// its own, because that is where something went wrong.
#[tokio::test]
async fn status_overrides_only_what_the_form_answers_with() {
    let note = Note {
        body: "Hello".to_owned(),
    };
    assert_eq!(note.into_response().status(), StatusCode::CREATED);

    let rejection = Note::from_request(post("/notes", ""), &())
        .await
        .unwrap_err();
    assert_eq!(rejection.into_response().status(), StatusCode::BAD_REQUEST);
}

// ─── The built-in renderer, a `StatusCode`, and leaving an impl out ──────────

/// `status` also takes a `StatusCode` written out, which its own type checks.
/// And `from_request = false` leaves the extractor out, so this one is a
/// response and nothing more.
#[derive(html_form::Form, Debug)]
#[form(
    renderer = html_form::axum::Builtin,
    status = StatusCode::ACCEPTED,
    from_request = false
)]
struct Receipt {
    #[field(type = "hidden")]
    reference: String,
}

#[tokio::test]
async fn a_status_code_written_out_is_a_status_too() {
    let receipt = Receipt {
        reference: "r-1".to_owned(),
    };
    let response = receipt.into_response();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert!(body_of(response).await.contains(r#"value="r-1""#));
}

/// `Receipt` names a renderer, and still is not an extractor.
#[test]
fn from_request_false_leaves_the_extractor_out() {
    fn is_extractor<T: FromRequest<()>>() {}

    is_extractor::<Signup>();
    // is_extractor::<Receipt>() would not compile.
    let _ = is_extractor::<Note>;
}

// ─── A generic form ──────────────────────────────────────────────────────────

#[derive(html_form::Form, Debug)]
#[form(method = "post", renderer = Page)]
struct WithCsrf<T> {
    #[field(type = "hidden", default = "3f9c")]
    csrf_token: String,

    #[field(flatten)]
    inner: T,
}

#[tokio::test]
async fn a_generic_form_is_an_extractor_and_a_response() {
    let wrapped: WithCsrf<Note> =
        WithCsrf::from_request(post("/notes", "csrf_token=3f9c&body=Hello"), &())
            .await
            .unwrap();
    assert_eq!(wrapped.inner.body, "Hello");

    let response = wrapped.into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_of(response).await.contains(r#"value="Hello""#));
}

// ─── A form whose context the request carries ────────────────────────────────

/// A session an extractor of its own reads off the request.
#[derive(Clone, Debug)]
struct Session {
    csrf: String,
}

impl<S: Send + Sync> FromRequestParts<S> for Session {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let csrf = parts
            .headers
            .get("x-csrf")
            .and_then(|value| value.to_str().ok())
            .ok_or((StatusCode::UNAUTHORIZED, "no session"))?;
        Ok(Session {
            csrf: csrf.to_owned(),
        })
    }
}

/// A function renderer may take the context after the view.
fn with_session(view: FormView, session: &Session) -> Html<String> {
    Html(format!("<!-- {} -->{}", session.csrf, view.to_html()))
}

fn issued(session: &Session) -> String {
    session.csrf.clone()
}

fn is_ours(submitted: &String, session: &Session) -> bool {
    *submitted == session.csrf
}

#[derive(html_form::Form, Debug)]
#[form(method = "post", context = Session, renderer = with_session)]
struct Comment {
    #[field(type = "hidden", default = issued, validate = is_ours)]
    csrf_token: String,

    #[field(type = "textarea", maxlength = 2000)]
    body: String,
}

fn commented(body: &str, csrf: Option<&str>) -> Request<Body> {
    let mut request = post("/comments", body);
    if let Some(csrf) = csrf {
        request
            .headers_mut()
            .insert("x-csrf", csrf.parse().unwrap());
    }
    request
}

#[tokio::test]
async fn the_context_comes_out_of_the_request_too() {
    let comment = Comment::from_request(
        commented("csrf_token=3f9c&body=Nice+post", Some("3f9c")),
        &(),
    )
    .await
    .unwrap();

    assert_eq!(comment.body, "Nice post");
}

#[tokio::test]
async fn the_renderer_reads_the_context_it_was_parsed_with() {
    let rejection = Comment::from_request(commented("csrf_token=forged&body=x", Some("3f9c")), &())
        .await
        .unwrap_err();

    let html = body_of(rejection.into_response()).await;
    assert!(html.starts_with("<!-- 3f9c -->"));
}

/// The context's own rejection goes on as itself, and answers with its own
/// status.
#[tokio::test]
async fn a_context_that_turns_the_request_down_rejects_as_itself() {
    let rejection = Comment::from_request(commented("body=x", None), &())
        .await
        .unwrap_err();

    assert!(matches!(rejection, Rejection::Context(_)));
    assert_eq!(rejection.into_response().status(), StatusCode::UNAUTHORIZED);
}
