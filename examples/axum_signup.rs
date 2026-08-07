//! A signup form served by axum, using `html_form::axum::Form`.
//!
//! The page a form sits in is written once, as a `Renderer`. Everything else in
//! the application then says which form it is about and nothing more:
//!
//! * `GET /` puts up the blank form.
//! * `POST /signup` handles the submission. A failed validation never reaches
//!   the handler. The extractor rejects with the form again, `Page` renders it,
//!   and the answer is `400` with what the user typed and the message on each
//!   field.
//! * `GET /account` puts up the same form, filled in from a value. The handler
//!   returns the extractor's own type, so the page is the one `Page` makes.
//!
//! Run with: `cargo run --example axum_signup --features axum`
//! then open <http://127.0.0.1:3000>.

use std::sync::atomic::{AtomicI32, Ordering};

use axum::Router;
use axum::response::{Html, IntoResponse};
use axum::routing::get;
// `Form` here is the extractor, so the trait of the same name comes in under
// `_`: that is enough for `Signup::render()`, and nothing has to be renamed.
use html_form::axum::{Form, Renderer};
use html_form::{Form as _, FormView};

#[derive(html_form::Form, Debug)]
#[form(action = "/signup", method = "post", submit = "Create account")]
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

/// The page every form of this application sits in.
///
/// It renders any form that asks for no context, so one type serves the whole
/// application. It is what answers a submission that failed, and what a handler
/// that returns a form answers with.
struct Page;

impl<T: html_form::Form<Context = ()>> Renderer<T> for Page {
    fn render(view: FormView, _context: &()) -> impl IntoResponse {
        // No status here. A renderer makes a page, and the rejection is what
        // says a submission was bad.
        Html(page(&view))
    }
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

/// The empty form, where each field shows its declared default.
async fn blank_form() -> Html<String> {
    Html(page(&Signup::render()))
}

/// The submission. `Form<Signup, Page>` goes last, because it consumes the
/// body.
///
/// There is no invalid case to handle. A submission that failed validation was
/// answered before the handler was ever called, by the same `Page`.
async fn signup(form: Form<Signup, Page>) -> Html<String> {
    let signup = form.into_inner();
    println!("created {signup:?}");

    Html(format!(
        "<!doctype html>\n<title>Welcome</title>\n\
         <h1>Welcome</h1>\n<p>Account created for <strong>{}</strong>.</p>\n",
        html_form::escape(&signup.email)
    ))
}

/// The same form, filled in from a value. Returning the extractor's own type
/// puts the value through `Page`, so this page and the one a failed submission
/// gets are the same page.
async fn account() -> Form<Signup, Page> {
    Form::new(Signup {
        email: "ada@example.com".to_owned(),
        password: String::new(),
        age: Some(36),
        newsletter: true,
        uid: 1,
    })
}

/// The layout, and the one place that knows what a page of this application
/// looks like. The renderer above and the blank form both go through it.
fn page(view: &FormView) -> String {
    // A view knows whether anything on it failed, so the layout can say so once
    // above the form as well as on each field.
    let banner = if view.has_errors {
        "<p role=\"alert\">Some fields need another look.</p>\n"
    } else {
        ""
    };

    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>Accounts</title>\n</head>\n<body>\n<h1>Accounts</h1>\n{banner}{}\n\
         </body>\n</html>\n",
        view.to_html()
    )
}
