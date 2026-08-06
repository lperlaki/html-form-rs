//! What a form's own functions are handed besides the value they are looking at.
//!
//! Everything in a [`FormSpec`](crate::FormSpec) is `const`, so the two escape
//! hatches that run code — `#[field(default = ...)]` and `validate = ...` —
//! used to see nothing of the request they were serving. A CSRF token that has
//! to match the session had to reach them through thread-local state.
//!
//! [`Form::Context`](crate::Form::Context) is that missing argument. It is
//! whatever the form says it is, it is handed in at the call that renders or
//! parses, and it reaches every function the form declares:
//!
//! * `#[field(default = path)]`, where `path` may be `fn() -> String` or
//!   `fn(&Context) -> String` — see [`DefaultSource`];
//! * `#[field(validate = path)]`, `fn(&T) -> V` or `fn(&T, &Context) -> V`;
//! * `#[form(validate = path)]`, `fn(&Self) -> V` or `fn(&Self, &Context) -> V`.
//!
//! Which shape a function has is read off its own signature rather than
//! declared, so a form that gains a context does not have to rewrite the checks
//! that never needed one.
//!
//! # Flattening across contexts
//!
//! A flattened sub-form is parsed and rendered with the enclosing form's
//! context, so by default the two have to agree. [`Provides`] is how they stop
//! having to: it says which context an enclosing one can hand down, and its
//! blanket impl is the identity — every context provides itself. A form that
//! wants no context at all asks for `()`, which is the one thing a context has
//! to be told it can supply:
//!
//! ```
//! # use html_form::Provides;
//! struct Session {
//!     token: String,
//! }
//!
//! // `Session` can stand in wherever a context-free sub-form is flattened.
//! impl Provides<()> for Session {
//!     fn provide(&self) -> &() {
//!         &()
//!     }
//! }
//! ```

use std::borrow::Cow;

/// Marker: the function ignores the context.
///
/// Never written down. It is inferred from the function's own arity, which is
/// what lets [`DefaultSource`], [`FieldValidator`](crate::FieldValidator) and
/// [`FormValidator`](crate::FormValidator) each accept two shapes without the
/// derive — which cannot see a signature — having to know which was written.
pub enum WithoutContext {}

/// Marker: the function takes the context as its last argument.
pub enum WithContext {}

/// How an enclosing form's context supplies the one a flattened sub-form asks
/// for.
///
/// Every context provides itself, which is the whole of it for a form flattened
/// into another that shares its context. Write an impl to hand a sub-form
/// something else — most often `()`, for a sub-form that was written without a
/// context and should not have to gain one just to be reused.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot supply the `{C}` a flattened sub-form is asking for",
    label = "this form's context is `{Self}`",
    note = "a flattened sub-form is parsed and rendered with the enclosing form's context",
    note = "write `impl html_form::Provides<{C}> for {Self}` to hand it one of its own"
)]
pub trait Provides<C> {
    /// The context to hand down.
    fn provide(&self) -> &C;
}

impl<C> Provides<C> for C {
    fn provide(&self) -> &C {
        self
    }
}

/// What `#[field(default = path)]` may name: a function producing the value a
/// field starts a render with.
///
/// Either arity will do, and the crate works out which was written:
///
/// | Signature | Called with |
/// |---|---|
/// | `fn() -> impl Into<Cow<'static, str>>` | nothing |
/// | `fn(&Context) -> impl Into<Cow<'static, str>>` | the render's context |
///
/// The return type is any string: `String` for a token minted on the spot,
/// `&'static str` for one that was already around.
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
    /// The value this field starts with, produced afresh for one render.
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

/// A context there is nothing to decide about, so a caller need not be asked
/// for one.
///
/// A form whose context is one of these has the short names —
/// [`render`](crate::Form::render), [`from_values`](crate::Form::from_values),
/// [`submit`](crate::Form::submit) — alongside the `…_with_context` ones,
/// and they hand [`EMPTY`](EmptyContext::EMPTY) over on the caller's behalf.
///
/// `()` is the only implementor here, and is what a form that declares no
/// context gets. Implement it for a context of your own that carries no
/// decision either — a unit struct standing in for "no session yet".
#[diagnostic::on_unimplemented(
    message = "`{Self}` is a context a caller has to supply",
    note = "the methods without a context are for a form whose context is `()`",
    note = "call the `…_with_context` one, or implement `html_form::EmptyContext` for `{Self}`"
)]
pub trait EmptyContext: 'static {
    /// The one value of this context, handed to a form on the caller's behalf.
    const EMPTY: &Self;
}

impl EmptyContext for () {
    const EMPTY: &Self = &();
}
