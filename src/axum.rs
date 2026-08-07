//! Axum integration: submit a form straight out of a request.
//!
//! The `axum` feature turns it on. It depends on `axum-core`, not on `axum`
//! itself, so it fits any handler in an axum 0.8 application.
//!
//! There are two extractors, and they differ in who renders the form again
//! when validation fails:
//!
//! * [`Outcome<T>`](crate::Outcome) hands the failure to the handler. The
//!   handler runs either way and decides what to send.
//! * [`Form<T, R>`] rejects with the form itself. The handler runs only on a
//!   valid submission, and it holds the parsed value and nothing else. The
//!   [`Renderer`] `R` turns the failed submission into the response, at the
//!   edge where axum asks for one.
//!
//! Each one consumes the body, so it is the *last* argument to a handler.
//!
//! # `Outcome<T>`: the handler decides
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
//! For this extractor a failed *validation* is not a rejection. It is
//! [`Outcome::Invalid`], and the handler still runs. Its rejection type covers
//! only the cases with no submission to validate. See [`FormRejection`].
//!
//! # `Form<T, R>`: the rejection is the form
//!
//! Write the re-render once, as a [`Renderer`], and the handler is left with
//! the valid case:
//!
//! ```no_run
//! use axum::response::{Html, IntoResponse};
//! use html_form::FormView;
//! use html_form::axum::{Form, Renderer};
//!
//! #[derive(html_form::Form)]
//! #[form(action = "/signup", method = "post")]
//! struct Signup {
//!     #[field(type = "email", label = "Email address")]
//!     email: String,
//! }
//!
//! /// The whole page the form sits in, for every form that fails.
//! struct Page;
//!
//! impl<T: html_form::Form<Context = ()>> Renderer<T> for Page {
//!     fn render(view: FormView, _context: &()) -> impl IntoResponse {
//!         Html(format!("<h1>Check the form</h1>{}", view.to_html()))
//!     }
//! }
//!
//! // Nothing is left to match on: the submission is valid or the handler
//! // never ran.
//! async fn signup(form: Form<Signup, Page>) -> Html<String> {
//!     Html(format!("Welcome, {}!", form.data.email))
//! }
//!
//! # let _: axum::Router =
//! axum::Router::new().route("/signup", axum::routing::post(signup))
//! # ;
//! ```
//!
//! A renderer builds a page, not a status. The status belongs to the rejection,
//! which is the one that knows what went wrong: `400 Bad Request` for a
//! submission that failed validation, `415 Unsupported Media Type` for a body
//! that was never a form, and whatever the context's own extractor said for a
//! context that turned the request down. The same page returned by a handler is
//! therefore a plain `200`. [`Builtin`] is the renderer around
//! [`FormView::to_html`](crate::FormView::to_html), and it asks for nothing but
//! the `html` feature.
//!
//! Nothing in the [`Rejection`] is a response yet. The invalid case is an
//! [`Invalid`], which holds the [`FormView`], the [`FormErrors`] and the
//! context, and runs `R` only when it is asked for a response. What rejected
//! the request is therefore still readable as itself: a layer can log the
//! errors, and a handler can map the rejection to something else and never
//! render the page at all. The context's own rejection type goes on untouched
//! for the same reason, which is what the `C` in `Rejection<T, R, C>` is.
//!
//! The same type is the response as well. A handler that returns one shows the
//! form filled in from the value, through the same renderer, so the page a
//! `GET` puts up and the page a failed `POST` puts up are the same page:
//!
//! ```no_run
//! # use axum::response::{Html, IntoResponse};
//! # use html_form::FormView;
//! # use html_form::axum::{Form, Renderer};
//! # #[derive(html_form::Form)]
//! # struct Signup { #[field(type = "email")] email: String }
//! # struct Page;
//! # impl<T: html_form::Form<Context = ()>> Renderer<T> for Page {
//! #     fn render(view: FormView, _context: &()) -> impl IntoResponse { Html(view.to_html()) }
//! # }
//! /// The form to edit an account, already filled in.
//! async fn edit() -> Form<Signup, Page> {
//!     Form::new(Signup { email: "ada@example.com".to_owned() })
//! }
//! ```
//!
//! # Where the values come from
//!
//! As with `axum::Form`, the crate reads a `GET` or `HEAD` request from the
//! query string, and every other method from the body. That body must be
//! `application/x-www-form-urlencoded`. `multipart/form-data` is out of scope.
//! Parse it with a multipart crate, then use
//! [`Values::from_pairs`](crate::Values::from_pairs) and
//! [`submit`](crate::Form::submit).
//!
//! # Forms that ask for a context
//!
//! An extractor has nothing but the request. [`Outcome<T>`] therefore extracts
//! only a form whose [`Context`](crate::Form::Context) the caller need not
//! supply, and a form that wants one is submitted in the handler, where the
//! context is: take the body with `axum::Form<Values>` or `Bytes`, then call
//! [`submit_with_context`](crate::Form::submit_with_context).
//!
//! [`Form<T, R>`] takes the other way out. It reads the context out of the
//! request as well, so the context has to be an axum extractor of its own:
//! `T::Context: FromRequestParts<S>`. A session loaded from a cookie, or a
//! locale read off `Accept-Language`, is already written that way. A form that
//! declares no context asks for `()`, which axum extracts from any request and
//! any state, so nothing is required of a form that needs nothing.

use std::fmt;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use axum_core::extract::{FromRequest, FromRequestParts, Request};
use axum_core::response::{IntoResponse, Response};
use bytes::Bytes;
use http::{Method, StatusCode, header};

use crate::{EmptyContext, FormErrors, FormView, Outcome, Values};

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
    // `Form<T, R>` is the other answer: it extracts the context too.
    T: crate::Form<Context: EmptyContext> + Send,
    S: Send + Sync,
{
    type Rejection = FormRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        Ok(T::submit(&read_values(req, state).await?))
    }
}

/// A valid submission, and the [`Renderer`] that answered the invalid ones.
///
/// It is an extractor. Where [`Outcome<T>`] hands a failed validation to the
/// handler, this one rejects with the form again, rendered by `R`. The handler
/// therefore has the parsed value and no case left to consider.
///
/// `R` is a marker. It holds no data, and it is what names the re-render in the
/// handler's signature. One renderer serves every form that shares a layout,
/// because [`Renderer<T>`] is a trait an empty type implements once, generically
/// over the forms it renders.
///
/// It goes the other way as well. A handler may *return* one, and the response
/// is the form filled in from the value. See the [`IntoResponse`] impl.
///
/// ```
/// use axum::extract::FromRequest;
/// use axum::http::{Request, header};
/// use axum::body::Body;
/// use html_form::axum::{Builtin, Form};
///
/// #[derive(html_form::Form)]
/// struct Signup {
///     #[field(type = "email")]
///     email: String,
/// }
///
/// # #[tokio::main] async fn main() {
/// let request = Request::post("/signup")
///     .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
///     .body(Body::from("email=a%40example.com"))
///     .unwrap();
///
/// let form: Form<Signup, Builtin> = Form::from_request(request, &()).await.unwrap();
/// assert_eq!(form.data.email, "a@example.com");
/// # }
/// ```
pub struct Form<T, R> {
    /// The submission, parsed and validated.
    pub data: T,
    /// `R` is named by the type and never held, so the struct carries nothing
    /// of it.
    renderer: PhantomData<R>,
}

impl<T, R> Form<T, R> {
    /// A form around a value that is already valid, for a test or a call that
    /// builds the handler's argument itself.
    pub fn new(data: T) -> Self {
        Form {
            data,
            renderer: PhantomData,
        }
    }

    /// The submission, without the renderer it arrived under.
    pub fn into_inner(self) -> T {
        self.data
    }
}

// These three are written out rather than derived. A derive would ask `R` for
// the same trait, and `R` is a marker this never holds, so the submission alone
// decides what the extractor can do.
impl<T: fmt::Debug, R> fmt::Debug for Form<T, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Form").field(&self.data).finish()
    }
}

impl<T: Clone, R> Clone for Form<T, R> {
    fn clone(&self) -> Self {
        Form::new(self.data.clone())
    }
}

impl<T: Copy, R> Copy for Form<T, R> {}

impl<T, R> Deref for Form<T, R> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}

impl<T, R> DerefMut for Form<T, R> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

/// The form filled in from the value, rendered by `R`. It is what a handler
/// *returns* to show an edit form, and it goes through the same renderer the
/// rejection does, so both pages come out alike.
///
/// The view is [`render_filled`](crate::Form::render_filled): the value in each
/// field and no error on any of them. Nothing here sets a status either, so the
/// page a handler puts up is a plain `200`. The rejection is where a status is
/// decided, because that is where something went wrong.
///
/// ```
/// # use axum::http::StatusCode;
/// # use axum::response::IntoResponse;
/// # use html_form::axum::{Builtin, Form};
/// # #[derive(html_form::Form)]
/// # struct Signup { #[field(type = "email")] email: String }
/// # let signup = Signup { email: "a@example.com".to_owned() };
/// let response = Form::<_, Builtin>::new(signup).into_response();
///
/// assert_eq!(response.status(), StatusCode::OK);
/// ```
///
/// The form holds no context, so this is for a form that asks for none. Render
/// one that does in the handler, with
/// [`render_filled_with_context`](crate::Form::render_filled_with_context).
impl<T, R> IntoResponse for Form<T, R>
where
    T: crate::Form<Context: EmptyContext>,
    R: Renderer<T>,
{
    fn into_response(self) -> Response {
        let context = <T::Context as EmptyContext>::EMPTY;
        R::render(self.data.render_filled_with_context(context), context).into_response()
    }
}

/// How [`Form<T, R>`] answers a submission that failed validation.
///
/// The view is the form again. It carries the values the user typed and the
/// message on each field, so a renderer is usually the page around
/// [`FormView::to_html`](crate::FormView::to_html), or the template that reads
/// the view.
///
/// The function gets the form's [`Context`](crate::Form::Context), so a
/// renderer may reach for whatever the context holds: the session that names
/// the user, a template engine, the locale to translate the labels into. It is
/// not `async`, because a view already holds everything the render reads. A
/// renderer that still has to wait for something answers with a body that
/// streams, which the runtime polls after the response has gone out.
///
/// A renderer builds a page, and not the status that carries it. Whatever it
/// sets is replaced: [`Invalid`] answers `400 Bad Request`, and a form a handler
/// returns is a `200`. What a renderer does own is the rest of the response, the
/// content type and any header the page needs among it.
///
/// ```
/// use axum::response::{Html, IntoResponse};
/// use html_form::FormView;
/// use html_form::axum::Renderer;
///
/// /// The locale a form was rendered under.
/// struct Locale(&'static str);
///
/// struct Translated;
///
/// impl<T: html_form::Form<Context = Locale>> Renderer<T> for Translated {
///     fn render(view: FormView, locale: &Locale) -> impl IntoResponse {
///         Html(view.localized(|key| translate(locale, key)).to_html())
///     }
/// }
/// # fn translate(locale: &Locale, key: &str) -> Option<String> { let _ = (locale, key); None }
/// ```
pub trait Renderer<T: crate::Form> {
    /// The response for a submission that did not validate.
    fn render(view: FormView, context: &T::Context) -> impl IntoResponse;
}

/// A submission that arrived whole and failed validation.
///
/// It is what [`Form<T, R>`] rejects with, and it holds the render rather than
/// the rendered page: `R` runs when the response is asked for, and not before.
/// Until then this is still a form and a set of errors. A layer that logs what
/// failed, or a handler that maps the rejection, reads them as such:
///
/// ```
/// # use axum::extract::FromRequest;
/// # use axum::http::{Request, StatusCode, header};
/// # use axum::body::Body;
/// # use axum::response::IntoResponse;
/// # use html_form::axum::{Builtin, Form, Rejection};
/// # #[derive(html_form::Form, Debug)]
/// # struct Signup { #[field(type = "email")] email: String }
/// # #[tokio::main] async fn main() {
/// # let request = Request::post("/signup")
/// #     .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
/// #     .body(Body::from("email=nope"))
/// #     .unwrap();
/// let rejection = Form::<Signup, Builtin>::from_request(request, &()).await.unwrap_err();
///
/// let Rejection::Invalid(invalid) = rejection else { panic!("this one validates") };
/// assert_eq!(invalid.errors.len(), 1);
/// assert_eq!(invalid.view.field("email").unwrap().value.as_deref(), Some("nope"));
///
/// // The page is built here, out of the view and the context it kept, and
/// // this is what says the submission was bad.
/// let response = invalid.into_response();
/// assert_eq!(response.status(), StatusCode::BAD_REQUEST);
/// # }
/// ```
///
/// The context is the one the request was parsed with, kept because that is
/// what [`Renderer::render`] is given.
pub struct Invalid<T: crate::Form, R> {
    /// The form again, with what the user typed and the message on each field.
    pub view: Box<FormView>,
    /// What was wrong, field by field.
    pub errors: FormErrors,
    /// What this submission was parsed with, and what the renderer gets.
    pub context: T::Context,
    renderer: PhantomData<R>,
}

impl<T: crate::Form, R> Invalid<T, R> {
    /// The three parts, under the renderer that will answer with them.
    pub fn new(view: Box<FormView>, errors: FormErrors, context: T::Context) -> Self {
        Invalid {
            view,
            errors,
            context,
            renderer: PhantomData,
        }
    }
}

impl<T: crate::Form<Context: fmt::Debug>, R> fmt::Debug for Invalid<T, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Invalid")
            .field("view", &self.view)
            .field("errors", &self.errors)
            .field("context", &self.context)
            .finish()
    }
}

impl<T: crate::Form, R> Invalid<T, R> {
    /// What a submission that failed validation answers with: `400 Bad
    /// Request`. The page under it is the renderer's, and the status is not.
    pub const STATUS: StatusCode = StatusCode::BAD_REQUEST;
}

/// This is where the renderer runs, and where the status is put on the page it
/// made.
impl<T: crate::Form, R: Renderer<T>> IntoResponse for Invalid<T, R> {
    fn into_response(self) -> Response {
        (Self::STATUS, R::render(*self.view, &self.context)).into_response()
    }
}

/// Why the handler did not run.
///
/// Nothing here is a response yet. Each variant is what turned the request
/// down, in the type it turned it down as: `C` is whatever the context's own
/// extractor rejects with, and [`Invalid`] is the form that failed. Each one
/// becomes a response at the edge, where axum asks for one.
///
/// The status is decided here rather than by the renderer, because what went
/// wrong is what a status is about:
///
/// | Variant | Status |
/// |---|---|
/// | [`Request`](Rejection::Request), not `application/x-www-form-urlencoded` | `415 Unsupported Media Type` |
/// | [`Request`](Rejection::Request), a body the server could not read | the one the body rejection came with, `400` among them |
/// | [`Context`](Rejection::Context) | whatever that extractor answers with |
/// | [`Invalid`](Rejection::Invalid) | `400 Bad Request` |
#[non_exhaustive]
pub enum Rejection<T: crate::Form, R, C> {
    /// There was no submission to validate. See [`FormRejection`].
    Request(FormRejection),
    /// The form's context is an extractor of its own, and it rejected the
    /// request. Nothing was parsed, and this is its own rejection type.
    Context(C),
    /// The submission arrived whole and failed validation.
    Invalid(Invalid<T, R>),
}

impl<T: crate::Form<Context: fmt::Debug>, R, C: fmt::Debug> fmt::Debug for Rejection<T, R, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rejection::Request(rejection) => f.debug_tuple("Request").field(rejection).finish(),
            Rejection::Context(rejection) => f.debug_tuple("Context").field(rejection).finish(),
            Rejection::Invalid(invalid) => f.debug_tuple("Invalid").field(invalid).finish(),
        }
    }
}

impl<T, R, C> IntoResponse for Rejection<T, R, C>
where
    T: crate::Form,
    R: Renderer<T>,
    C: IntoResponse,
{
    fn into_response(self) -> Response {
        match self {
            Rejection::Request(rejection) => rejection.into_response(),
            Rejection::Context(rejection) => rejection.into_response(),
            Rejection::Invalid(invalid) => invalid.into_response(),
        }
    }
}

impl<T, R, S> FromRequest<S> for Form<T, R>
where
    T: crate::Form + Send,
    // The context comes out of the request, which is what lets this extractor
    // take a form that `Outcome<T>` cannot. A form that declares no context
    // asks for `()`, and axum extracts that from anything.
    T::Context: FromRequestParts<S> + Send,
    R: Renderer<T>,
    S: Send + Sync,
{
    // The context's own rejection goes on as itself, rather than as the
    // response it would have become.
    type Rejection = Rejection<T, R, <T::Context as FromRequestParts<S>>::Rejection>;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // The context reads the head, and the body is what is left for the
        // submission itself.
        let (mut parts, body) = req.into_parts();
        let context = <T::Context>::from_request_parts(&mut parts, state)
            .await
            .map_err(Rejection::Context)?;

        let values = read_values(Request::from_parts(parts, body), state)
            .await
            .map_err(Rejection::Request)?;

        match T::submit_with_context(&values, &context) {
            Outcome::Valid(data) => Ok(Form::new(data)),
            // The context comes along, because the render that has not happened
            // yet is the one that needs it.
            Outcome::Invalid { errors, view } => {
                Err(Rejection::Invalid(Invalid::new(view, errors, context)))
            }
        }
    }
}

/// The built-in renderer: the form's own HTML, and nothing around it.
///
/// It renders any form, and it reads nothing out of the context. It is the
/// renderer to reach for where the form is the whole answer, and the one to
/// replace with a [`Renderer`] of your own where the form sits inside a page.
///
/// It says the body is HTML and leaves the status alone, which is what every
/// renderer does. [`Invalid`] is what answers `400`.
///
/// The `html` feature gates it, because it is
/// [`FormView::to_html`](crate::FormView::to_html) that does the rendering.
#[cfg(feature = "html")]
#[derive(Debug, Default, Clone, Copy)]
pub struct Builtin;

#[cfg(feature = "html")]
impl<T: crate::Form> Renderer<T> for Builtin {
    fn render(view: FormView, _context: &T::Context) -> impl IntoResponse {
        (
            [(
                header::CONTENT_TYPE,
                http::HeaderValue::from_static("text/html; charset=utf-8"),
            )],
            view.to_html(),
        )
    }
}

/// The submitted values, wherever this request carries them.
async fn read_values<S>(req: Request, state: &S) -> Result<Values, FormRejection>
where
    S: Send + Sync,
{
    // A form submitted with GET carries its values in the query string. It has
    // no body, so it has no content type.
    if req.method() == Method::GET || req.method() == Method::HEAD {
        return Ok(Values::parse(req.uri().query().unwrap_or_default()));
    }

    if !is_urlencoded(&req) {
        return Err(FormRejection::UnsupportedMediaType);
    }

    let bytes = Bytes::from_request(req, state)
        .await
        .map_err(FormRejection::Body)?;

    // The percent-decode also decodes the body, so the body never has to be a
    // `str` on the way there. See `Values::parse_bytes`.
    Ok(Values::parse_bytes(&bytes))
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
