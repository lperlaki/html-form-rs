//! What a form's own functions receive besides the value they look at.
//!
//! Everything in a [`FormSpec`](crate::FormSpec) is `const`. The two escape
//! hatches that run code, `#[field(default = ...)]` and `validate = ...`,
//! therefore used to see nothing of the request they served. A CSRF token that
//! has to match the session had to reach them through thread-local state.
//!
//! [`Form::Context`](crate::Form::Context) is that missing argument. It is
//! whatever the form says it is. The caller passes it in at the call that
//! renders or parses, and it reaches every function the form declares:
//!
//! * `#[field(default = path)]`, where `path` may be `fn() -> String` or
//!   `fn(&Context) -> String`. See [`DefaultSource`].
//! * `#[field(validate = path)]`, `fn(&T) -> V` or `fn(&T, &Context) -> V`.
//! * `#[form(validate = path)]`, `fn(&Self) -> V` or `fn(&Self, &Context) -> V`.
//!
//! You do not declare which shape a function has. The crate reads it off the
//! signature, so a form that gains a context does not have to rewrite the checks
//! that never needed one.
//!
//! # Flattening across contexts
//!
//! The crate parses and renders a flattened sub-form with the enclosing form's
//! context, so by default the two have to agree. [`Provides`] removes that
//! requirement. It says which context an enclosing one can give away, and its
//! blanket impl is the identity: every context provides itself. A form that
//! wants no context at all asks for `()`, which is the one thing a context has
//! to be told it can supply:
//!
//! ```
//! # use html_form::Provides;
//! struct Session {
//!     token: String,
//! }
//!
//! // `Session` stands in wherever a form flattens a context-free sub-form.
//! impl Provides<()> for Session {
//!     fn provide(&self) -> &() {
//!         &()
//!     }
//! }
//! ```

use std::borrow::Cow;

/// Marker: the function ignores the context.
///
/// You never write it down. The crate infers it from the function's own arity.
/// That is what lets [`DefaultSource`], [`FieldValidator`](crate::FieldValidator)
/// and [`FormValidator`](crate::FormValidator) each accept two shapes. The
/// derive cannot see a signature, so it never has to know which one you wrote.
pub enum WithoutContext {}

/// Marker: the function takes the context as its last argument.
pub enum WithContext {}

/// How an enclosing form's context supplies the one a flattened sub-form asks
/// for.
///
/// Every context provides itself. That is the whole of it where a sub-form
/// shares the context of the form it goes into. Write an impl to give a
/// sub-form something else. Most often that is `()`, for a sub-form written
/// without a context that should not have to gain one to be reused.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot supply the `{C}` that a flattened sub-form asks for",
    label = "this form's context is `{Self}`",
    note = "the crate renders and parses a flattened sub-form with the enclosing form's context",
    note = "write `impl html_form::Provides<{C}> for {Self}` to give it one of its own"
)]
pub trait Provides<C> {
    /// The context to give away.
    fn provide(&self) -> &C;
}

impl<C> Provides<C> for C {
    fn provide(&self) -> &C {
        self
    }
}

/// What `#[field(default = path)]` may name: a function that produces the value
/// a field starts a render with.
///
/// Either arity works, and the crate finds out which one you wrote:
///
/// | Signature | The crate calls it with |
/// |---|---|
/// | `fn() -> impl Into<Cow<'static, str>>` | nothing |
/// | `fn(&Context) -> impl Into<Cow<'static, str>>` | the render's context |
///
/// The return type is any string: `String` for a token you mint on the spot,
/// `&'static str` for one that already exists.
///
/// ```
/// use html_form::Form;
///
/// struct Session {
///     csrf: String,
/// }
///
/// #[derive(Form)]
/// #[form(context = Session)]
/// struct Comment {
///     #[field(type = "hidden", default = issued_token)]
///     csrf_token: String,
///     #[field(type = "textarea", default = "")]
///     body: String,
/// }
///
/// fn issued_token(session: &Session) -> String {
///     session.csrf.clone()
/// }
///
/// let session = Session { csrf: "3f9c".to_owned() };
/// let view = Comment::render_with_context(&session);
/// assert_eq!(view.field("csrf_token").unwrap().value.as_deref(), Some("3f9c"));
/// ```
pub trait DefaultSource<C, M> {
    /// The value this field starts with, produced anew for one render.
    fn generate(&self, context: &C) -> Cow<'static, str>;
}

impl<F, S, C> DefaultSource<C, WithoutContext> for F
where
    F: Fn() -> S,
    S: Into<Cow<'static, str>>,
{
    fn generate(&self, _context: &C) -> Cow<'static, str> {
        self().into()
    }
}

impl<F, S, C> DefaultSource<C, WithContext> for F
where
    F: Fn(&C) -> S,
    S: Into<Cow<'static, str>>,
{
    fn generate(&self, context: &C) -> Cow<'static, str> {
        self(context).into()
    }
}

/// A context with nothing to decide, so the crate never asks a caller for one.
///
/// A form with such a context gets the short names next to the
/// `…_with_context` ones: [`render`](crate::Form::render),
/// [`from_values`](crate::Form::from_values) and
/// [`submit`](crate::Form::submit). They pass
/// [`EMPTY`](EmptyContext::EMPTY) on the caller's behalf.
///
/// `()` is the only implementor here, and it is what a form that declares no
/// context gets. Implement it for a context of your own that carries no
/// decision either, such as a unit struct that stands for "no session yet".
#[diagnostic::on_unimplemented(
    message = "`{Self}` is a context the caller has to supply",
    note = "the methods without a context are for a form whose context is `()`",
    note = "call the `…_with_context` one, or implement `html_form::EmptyContext` for `{Self}`"
)]
pub trait EmptyContext: 'static {
    /// The one value of this context, passed to a form on the caller's behalf.
    const EMPTY: &Self;
}

impl EmptyContext for () {
    const EMPTY: &Self = &();
}
