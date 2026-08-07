//! What the axum suites both need to make a request and read a response.
//!
//! An integration test file is its own crate, so these would otherwise be
//! written once per suite, and the two copies of the renderer would be free to
//! drift until the suites no longer tested the same page.

use axum::body::Body;
use axum::http::{Request, header};
use axum::response::{Html, IntoResponse, Response};
use html_form::FormView;
use html_form::axum::Renderer;

/// A renderer of the kind an application writes once: the form inside its page.
pub struct Page;

impl<T: html_form::Form> Renderer<T> for Page {
    fn render(view: FormView, _context: &T::Context) -> impl IntoResponse {
        Html(format!("<h1>Check the form</h1>{}", view.to_html()))
    }
}

/// A form submission, the way a browser sends one.
pub fn post(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

/// The response body, as the string a test asserts against.
pub async fn body_of(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}
