//! Axum integration: submit a form straight out of a request.
//!
//! Enabled by the `axum` feature. It brings in `axum-core` rather than `axum`
//! itself, so it fits any handler in an axum 0.8 application.
//!
//! [`Outcome<T>`](crate::Outcome) is an extractor. It is the *last* argument to
//! a handler, because it consumes the body.
//!
//! ```no_run
//! use axum::response::{Html, IntoResponse, Response};
//! use axum::http::StatusCode;
//! use web_form::{Outcome, WebForm};
//!
//! #[derive(WebForm)]
//! #[form(action = "/signup", method = "post")]
//! struct Signup {
//!     #[field(type = "email", label = "Email address")]
//!     email: String,
//! }
//!
//! async fn signup(form: Outcome<Signup>) -> Response {
//!     match form {
//!         Outcome::Valid(signup) => Html(format!("Welcome, {}!", signup.email)).into_response(),
//!         // The re-render carries what the user typed and what was wrong with it.
//!         Outcome::Invalid { view, .. } => {
//!             (StatusCode::UNPROCESSABLE_ENTITY, Html(view.to_html())).into_response()
//!         }
//!     }
//! }
//! ```
//!
//! A failed *validation* is not a rejection: it is [`Outcome::Invalid`], and
//! the handler still runs. The rejection type covers only the cases where there
//! is no submission to validate at all — see [`FormRejection`].
//!
//! # Where the values come from
//!
//! As with axum's own `Form`, a `GET` or `HEAD` request is read from the query
//! string and every other method from the body, which must be
//! `application/x-www-form-urlencoded`. `multipart/form-data` is out of scope:
//! parse it with a multipart crate and go through
//! [`Values::from_pairs`](crate::Values::from_pairs) and
//! [`WebForm::submit`](crate::WebForm::submit).
//!
//! # Forms that ask for a context
//!
//! An extractor has nothing but the request, so [`Outcome<T>`] extracts only a
//! form whose [`Context`](crate::WebForm::Context) needs no supplying. A form
//! that wants one is submitted in the handler, where the context is: take the
//! body with axum's own `Form<Values>` or `Bytes`, then call
//! [`WebForm::submit_with_context`](crate::WebForm::submit_with_context).

use std::fmt;

use axum_core::extract::{FromRequest, Request};
use axum_core::response::{IntoResponse, Response};
use bytes::Bytes;
use http::{Method, StatusCode, header};

use crate::{EmptyContext, Outcome, Values, WebForm};

/// Why there was no submission to validate.
///
/// Every variant is about the request itself — the wrong content type, a body
/// that could not be read. A submission that arrives intact but fails
/// validation is [`Outcome::Invalid`], not a rejection.
#[derive(Debug)]
#[non_exhaustive]
pub enum FormRejection {
    /// Not `application/x-www-form-urlencoded` (`415`).
    UnsupportedMediaType,
    /// The body could not be read — disconnected, or over the body limit
    /// (whatever status the underlying rejection carries).
    Body(axum_core::extract::rejection::BytesRejection),
}

impl FormRejection {
    /// The status this rejection responds with.
    pub fn status(&self) -> StatusCode {
        match self {
            FormRejection::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            FormRejection::Body(rejection) => rejection.status(),
        }
    }
}

impl fmt::Display for FormRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormRejection::UnsupportedMediaType => f.write_str(
                "Expected a request with `Content-Type: application/x-www-form-urlencoded`",
            ),
            FormRejection::Body(rejection) => rejection.fmt(f),
        }
    }
}

impl std::error::Error for FormRejection {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FormRejection::Body(rejection) => Some(rejection),
            FormRejection::UnsupportedMediaType => None,
        }
    }
}

impl IntoResponse for FormRejection {
    fn into_response(self) -> Response {
        (self.status(), self.to_string()).into_response()
    }
}

impl<T, S> FromRequest<S> for Outcome<T>
where
    // An extractor has nothing but the request, so it can only submit a form
    // that asks for no context of its own. One that does is submitted in the
    // handler, where the context is — see `submit_with_context`.
    T: WebForm<Context: EmptyContext> + Send,
    S: Send + Sync,
{
    type Rejection = FormRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // A form submitted with GET carries its values in the query string, and
        // has no body to type.
        if req.method() == Method::GET || req.method() == Method::HEAD {
            let query = req.uri().query().unwrap_or_default();
            return Ok(T::submit_urlencoded(query));
        }

        if !is_urlencoded(&req) {
            return Err(FormRejection::UnsupportedMediaType);
        }

        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(FormRejection::Body)?;

        // The body is decoded where it is percent-decoded, so it never has to
        // be a `str` on the way there — see `Values::parse_bytes`.
        Ok(T::submit(&Values::parse_bytes(&bytes)))
    }
}

/// Whether the request declares an urlencoded body, ignoring any parameters
/// (`; charset=utf-8`) the sender chose to add.
fn is_urlencoded(req: &Request) -> bool {
    let Some(content_type) = req.headers().get(header::CONTENT_TYPE) else {
        return false;
    };
    let Ok(content_type) = content_type.to_str() else {
        return false;
    };
    let essence = content_type.split(';').next().unwrap_or_default().trim();

    essence.eq_ignore_ascii_case("application/x-www-form-urlencoded")
}
