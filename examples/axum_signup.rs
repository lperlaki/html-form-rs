//! A signup form served by axum, using `Outcome<T>` as an extractor.
//!
//! `GET /` renders the blank form, `POST /signup` handles the submission. A
//! failed validation is not an extractor rejection: the handler still runs, and
//! gets back the same form to render again — with the values the user typed and
//! the messages attached to their fields.
//!
//! Run with: `cargo run --example axum_signup --features axum`
//! then open <http://127.0.0.1:3000>.

use axum::Router;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use web_form::{Outcome, WebForm};

#[derive(WebForm, Debug)]
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
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(blank_form))
        .route("/signup", axum::routing::post(signup));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

/// The empty form, each field showing its declared default.
async fn blank_form() -> Html<String> {
    Html(page("Sign up", &Signup::render().to_html()))
}

/// The submission. `Outcome<Signup>` goes last, because it consumes the body.
async fn signup(form: Outcome<Signup>) -> Response {
    match form {
        Outcome::Valid(signup) => Html(page(
            "Welcome",
            &format!(
                "<p>Account created for <strong>{}</strong>.</p>",
                web_form::escape(&signup.email)
            ),
        ))
        .into_response(),

        // Every problem at once, each one already attached to its field.
        Outcome::Invalid { errors, view } => {
            eprintln!("rejected {} field(s): {errors}", errors.len());
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Html(page("Sign up", &view.to_html())),
            )
                .into_response()
        }
    }
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n</head>\n<body>\n<h1>{title}</h1>\n{body}\n</body>\n</html>\n"
    )
}
