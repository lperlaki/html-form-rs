//! Axum integration: submit a form straight out of a request.
//!
//! The `axum` feature turns it on. It depends on `axum-core`, not on `axum`
//! itself, so it fits any handler in an axum 0.8 application.
//!
//! [`Outcome<T>`](crate::Outcome) is an extractor. It is the *last* argument to
//! a handler, because it consumes the body.
//!
//! ```no_run
//! use axum::response::{Html, IntoResponse, Response};
//! use axum::http::StatusCode;
//! use html_form::{Form, Outcome};
//!
//! #[derive(Form)]
//! #[form(action = "/signup", method = "post")]
//! struct Signup {
//!     #[field(type = "email", label = "Email address")]
//!     email: String,
//! }
//!
//! async fn signup(form: Outcome<Signup>) -> Response {
//!     match form {
//!         Outcome::Valid(signup) => Html(format!("Welcome, {}!", signup.email)).into_response(),
//!         // The re-render carries what the user typed and each error.
//!         Outcome::Invalid { view, .. } => {
//!             (StatusCode::UNPROCESSABLE_ENTITY, Html(view.to_html())).into_response()
//!         }
//!     }
//! }
//! ```
//!
//! A failed *validation* is not a rejection. It is [`Outcome::Invalid`], and
//! the handler still runs. The rejection type covers only the cases with no
//! submission to validate. See [`FormRejection`].
//!
//! # Where the values come from
//!
//! As with `axum::Form`, the crate reads a `GET` or `HEAD` request from the
//! query string, and every other method from the body. That body must be
//! `application/x-www-form-urlencoded`. `multipart/form-data` is out of scope.
//! Parse it with a multipart crate, then use
//! [`Values::from_pairs`](crate::Values::from_pairs) and
//! [`Form::submit`](crate::Form::submit).
//!
//! # Forms that ask for a context
//!
//! An extractor has nothing but the request, so [`Outcome<T>`] extracts only a
//! form whose [`Context`](crate::Form::Context) the caller need not supply.
//! Submit a form that wants a context in the handler, where the context is.
//! Take the body with `axum::Form<Values>` or `Bytes`, then call
//! [`Form::submit_with_context`](crate::Form::submit_with_context).

use std::fmt;

use axum_core::extract::{FromRequest, Request};
use axum_core::response::{IntoResponse, Response};
use bytes::Bytes;
use http::{Method, StatusCode, header};

use crate::{EmptyContext, Form, Outcome, Values};

/// Why there was no submission to validate.
///
/// Every variant is about the request itself: the wrong content type, or a body
/// the server could not read. A submission that arrives whole but fails
/// validation is an [`Outcome::Invalid`], not a rejection.
#[derive(Debug)]
#[non_exhaustive]
pub enum FormRejection {
    /// Not `application/x-www-form-urlencoded` (`415`).
    UnsupportedMediaType,
    /// The server could not read the body. The connection dropped, or the body
    /// went over the limit. The status comes from the rejection underneath.
    Body(axum_core::extract::rejection::BytesRejection),
}

impl FormRejection {
    /// The status this rejection answers with.
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
    // An extractor has nothing but the request, so it can submit only a form
    // that asks for no context of its own. A form that asks for one goes
    // through the handler, where the context is. See `submit_with_context`.
    T: Form<Context: EmptyContext> + Send,
    S: Send + Sync,
{
    type Rejection = FormRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // A form submitted with GET carries its values in the query string. It
        // has no body, so it has no content type.
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

        // The percent-decode also decodes the body, so the body never has to be
        // a `str` on the way there. See `Values::parse_bytes`.
        Ok(T::submit(&Values::parse_bytes(&bytes)))
    }
}

/// Whether the request declares an urlencoded body. This ignores any parameter
/// the sender added, such as `; charset=utf-8`.
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
