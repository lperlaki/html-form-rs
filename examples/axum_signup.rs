//! A signup form served by axum, using `#[form(renderer = ...)]`.
//!
//! The page a form sits in is written once, as `page`. The form names it, and
//! from there the struct *is* the extractor and the response. Every handler
//! then says which form it is about and nothing more:
//!
//! * `GET /` puts up the blank form.
//! * `POST /signup` handles the submission. A failed validation never reaches
//!   the handler. The extractor rejects with the form again, `page` renders it,
//!   and the answer is `400` with what the user typed and the message on each
//!   field.
//! * `GET /account` puts up the same form, filled in from a value. The handler
//!   returns the form itself, so the page is the one `page` makes.
//!
//! A renderer that serves several forms is a marker type implementing
//! `Renderer` in place of a function, and `html_form::axum::Form<Signup, Page>`
//! is the same wiring spelled out at each handler.
//!
//! Run with: `cargo run --example axum_signup --features axum`
//! then open <http://127.0.0.1:3000>.

use std::sync::atomic::{AtomicI32, Ordering};

use axum::Router;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::routing::get;
use html_form::{Form, FormView};
// The trait comes in under `_`: that is enough for `Signup::render()`, and the
// derive of the same name does not have to be renamed.

#[derive(Form, Debug)]
#[form(
    action = "/signup",
    method = "post",
    submit = "Create account",
    // The whole axum half of this example. `Signup` is now an extractor and a
    // response, and `page` is what answers a submission that failed.
    renderer = page
)]
struct Signup {
    #[field(
        type = "email",
        label = "Email address",
        autocomplete = "email",
        placeholder = "you@example.com"
    )]
    email: String,

    #[field(
        type = "password",
        label = "Password",
        minlength = 12,
        help = "At least 12 characters."
    )]
    password: String,

    #[field(label = "Age", min = 18, max = 120)]
    age: Option<u32>,

    #[field(label = "Subscribe to the newsletter", default = true)]
    newsletter: bool,

    /// `reset` says this field is the form's own. It shows what `new_id`
    /// produced for *this* render, on a blank form, on a submission that came
    /// back with errors, and on the filled-in form at `/account` alike. What
    /// the browser sent under this name is never shown again. A CSRF token is
    /// the same field with `type = "hidden"`.
    #[field(label = "The ID", default = new_id, type = "text", reset)]
    uid: i32,
}

/// One counter for the whole server, not one per worker thread. A
/// `thread_local!` here would hand out the same few numbers over and over,
/// because the runtime answers consecutive requests on different threads, and
/// a repeated number is exactly what a field that failed to reset looks like.
static COUNTER: AtomicI32 = AtomicI32::new(1);

fn new_id() -> i32 {
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(blank_form))
        .route("/account", get(account))
        .route("/signup", axum::routing::post(signup));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

/// The empty form, where each field shows its declared default. It goes through
/// the same `page` the renderer does, because it is the same page.
async fn blank_form() -> impl IntoResponse {
    page(Signup::render())
}

/// The submission. `Signup` goes last, because it consumes the body.
///
/// There is no invalid case to handle, and no wrapper to unwrap. A submission
/// that failed validation was answered before the handler was ever called, by
/// the same `page`.
async fn signup(signup: Signup) -> Html<String> {
    println!("created {signup:?}");

    Html(format!(
        "<!doctype html>\n<title>Welcome</title>\n\
         <h1>Welcome</h1>\n<p>Account created for <strong>{}</strong>.</p>\n",
        html_form::escape(&signup.email)
    ))
}

/// The same form, filled in from a value. Returning the form puts the value
/// through `page`, so this page and the one a failed submission gets are the
/// same page.
async fn account() -> Signup {
    Signup {
        email: "ada@example.com".to_owned(),
        password: String::new(),
        age: Some(36),
        newsletter: true,
        uid: 1,
    }
}

/// The layout, the one place that knows what a page of this application looks
/// like, and the renderer the form named.
///
/// A renderer is a function of a view, so this is one already. The view arrives
/// by value, as `Renderer::render` takes it, and the answer is anything that is
/// a response.
///
/// No status is set here. A renderer makes a page, and the rejection is what
/// says a submission was bad.
fn page(view: FormView) -> Html<String> {
    // A view knows whether anything on it failed, so the layout can say so once
    // above the form as well as on each field.
    let banner = if view.has_errors {
        "<p role=\"alert\">Some fields need another look.</p>\n"
    } else {
        ""
    };

    Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>Accounts</title>\n</head>\n<body>\n<h1>Accounts</h1>\n{banner}{}\n\
         </body>\n</html>\n",
        view.to_html()
    ))
}
