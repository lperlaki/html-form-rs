#![cfg_attr(docsrs, feature(doc_cfg))]
// The one thing a `const` spec cannot describe is the type of the context its
// glue reads, so a render hands that context over erased. It travels as a
// `&dyn Any` and each piece of glue downcasts, which keeps the pairing of a
// spec with a context a checked one — and leaves the whole crate, generated
// code included, with nothing to write `unsafe` for.
#![forbid(unsafe_code)]
// The crate's front page is the README, so the two cannot drift: every Rust
// block in it is a doctest that `cargo test` runs. The long form is
// `docs/guide.md`, included by the `guide` module below on the same terms.
#![doc = include_str!("../README.md")]
//!
//! # Where to look next
//!
//! The README above is the tour, and the [`guide`] is the long form of it:
//! every attribute, every type, and the whole of the framework integration.
//! These are the items both name, for jumping straight to one:
//!
//! Each bullet names items this build has. A feature that is off takes its own
//! bullet out of the list, because a link to an item that is not compiled is a
//! `rustdoc` warning and not a pointer anybody can follow.
//!
//! * [`Form`] is the trait `#[derive(Form)]` implements, and it holds every
//!   render and parse call. [`FormValue`] is the same for one *value*.
#![cfg_attr(
    feature = "derive",
    doc = "* [`FormChoice`] turns a fieldless enum into a set of options."
)]
//! * [`FormSpec`] is the static description the derive emits, and [`FormView`]
//!   is the runtime one a template renders. [`Control`] is what keeps an
//!   attribute from landing on a control that cannot hold it, and [`Text`] is
//!   what every person-facing string is: literal text, or an i18n key.
//! * [`Outcome`] is what a submission comes back as, [`FormErrors`] is what
//!   went wrong, and [`ErrorKind`] names the constraint each value broke.
//! * [`Values`] is a submission untyped, off the wire or out of JSON.
//! * [`FieldValidation`] and [`FormValidation`] list every shape a
//!   `validate = ...` function may return; [`FieldValidator`] and
//!   [`FormValidator`] list what it may take. [`DefaultSource`] does the same
//!   for `default = ...`, and [`FieldDefault`] is the slot in the spec that
//!   holds all three kinds of default.
//! * [`Provides`] is how a form with a context flattens one without.
#![cfg_attr(
    feature = "axum",
    doc = "* The [`axum`] module holds both extractors and the \
           [`Renderer`](axum::Renderer) trait behind `#[form(renderer = ...)]`."
)]

#[cfg(feature = "axum")]
pub mod axum;

/// The long form of the README: every attribute, every type, and the whole of
/// the framework integration.
///
/// It is a page of its own so the crate root stays a tour rather than a manual.
/// Nothing is declared here — every Rust block in it is a doctest that
/// `cargo test` runs, so the guide and the crate cannot drift.
pub mod guide {
    #![doc = include_str!("../docs/guide.md")]
}

mod context;
mod error;
#[cfg(feature = "html")]
mod html;
mod kind;
mod runtime;
mod spec;
mod validate;
mod value;
mod values;
mod view;

#[cfg(feature = "axum")]
pub use axum::FormRejection;
pub use context::{DefaultSource, EmptyContext, Provides, WithContext, WithoutContext};
pub use error::{ErrorKind, FieldError, FormErrors, ValueError};
#[cfg(feature = "html")]
pub use html::escape;
pub use kind::FieldKind;
pub use runtime::ParseCtx;
pub use spec::{
    Attr, Bounds, Choice, ChoiceStyle, ChooseControl, Control, Entry, FieldDefault, FieldSpec,
    FileControl, Flattened, FormEncType, FormMethod, FormSpec, Generate, NumberControl,
    NumberFormat, Provider, ResolvedField, TemporalControl, TemporalFormat, Text, TextControl,
    TextFormat, TextareaControl,
};
pub use validate::{FieldValidation, FieldValidator, FormValidation, FormValidator};
pub use value::FormValue;
pub use values::Values;
pub use view::{AttrView, ChoiceView, FieldView, FormView, is_attr_name};

#[doc(hidden)]
pub use runtime::__private;

#[cfg(feature = "derive")]
pub use html_form_derive::{Form, FormChoice, FormValue};

/// Everything needed to declare and use a form.
pub mod prelude {
    // `Form` and `FormValue` each name the trait and, with the `derive`
    // feature, the macro. One `use` brings in whichever of the two you mean.
    #[cfg(feature = "derive")]
    pub use crate::{Choice, FormChoice};
    pub use crate::{FieldKind, Form, FormErrors, FormValue, FormView, Outcome, Values};
}

/// The result of handling a submission.
///
/// The invalid case carries the typed errors and a [`FormView`] ready to render
/// back to the user, with the values they entered.
#[derive(Debug)]
pub enum Outcome<T> {
    Valid(T),
    Invalid {
        errors: FormErrors,
        view: Box<FormView>,
    },
}

impl<T> Outcome<T> {
    pub fn is_valid(&self) -> bool {
        matches!(self, Outcome::Valid(_))
    }

    /// The parsed value, without the re-render.
    pub fn ok(self) -> Option<T> {
        match self {
            Outcome::Valid(value) => Some(value),
            Outcome::Invalid { .. } => None,
        }
    }

    /// The re-render, without the parsed value.
    pub fn view(self) -> Option<Box<FormView>> {
        match self {
            Outcome::Valid(_) => None,
            Outcome::Invalid { view, .. } => Some(view),
        }
    }

    /// Turn into a `Result` whose error is the form to render again.
    pub fn into_result(self) -> Result<T, Box<FormView>> {
        match self {
            Outcome::Valid(value) => Ok(value),
            Outcome::Invalid { view, .. } => Err(view),
        }
    }
}

/// A struct that describes an HTML form.
///
/// `#[derive(Form)]` implements it. The derive generates the members without a
/// body. The trait provides everything else.
///
/// Every method that renders or parses takes the form's
/// [`Context`](Self::Context) and says so in its name. Each one has a twin
/// without the argument and without the suffix, such as `Signup::render()` in
/// place of `Signup::render_with_context(&())`. The twin is available where the
/// context is an [`EmptyContext`], which covers every form that declares none.
///
/// # Pairing the spec with the context
///
/// [`SPEC`](Self::SPEC) should be a spec written for *this* form: every
/// [`FieldDefault`] it holds, and every [`Provider`] on a flatten inside it,
/// names `Self::Context` as the type it reads back out of the erased context it
/// is called with.
///
/// A spec cannot name a context type. It is one `const` shared by every render
/// of every caller, so the context that reaches its glue arrives as a
/// [`&dyn Any`](std::any::Any), and the glue downcasts. `#[derive(Form)]` writes
/// both halves together, so they always agree. A hand-written impl agrees by
/// building its own spec, or by naming the `SPEC` of a form whose `Context` is
/// the same type.
///
/// Borrowing another form's `SPEC` under a different `Context` is the way to
/// disagree, and it *panics* on the first render that runs a generated default
/// or crosses such a flatten. Nothing about it is undefined, which is why this
/// trait is safe to implement: the downcast is a check, and it names the type
/// the glue wanted.
pub trait Form: Sized {
    /// What this form's own functions receive besides the value they look at:
    /// the session, a database handle, the request's locale. It holds whatever
    /// `#[field(default = ...)]` and `validate = ...` need to know and a
    /// `const` spec cannot hold.
    ///
    /// It is `()` for a form that needs nothing, which is what the derive
    /// assumes until `#[form(context = ...)]` says otherwise.
    /// [`DefaultSource`], [`FieldValidator`] and [`FormValidator`] are how a
    /// function reaches it. [`Provides`] lets a form with a context flatten one
    /// without a context.
    ///
    /// It is `'static` because a render hands it to the spec's glue erased, and
    /// the glue names the type back with a downcast. A context that borrows is
    /// therefore a context that holds an owned handle — a `String` rather than
    /// a `&str` — or one reached through an `Arc`.
    type Context: 'static;

    /// The static description of this form.
    ///
    /// It is a constant, not a function. The derive builds all of it at
    /// const-evaluation time. A flattened sub-form is therefore a reference the
    /// compiler has already resolved, not a call made at render time, and any
    /// `const` context can read `SPEC`.
    ///
    /// This is the constant the pairing above is about: its glue downcasts the
    /// erased context to a [`Context`](Self::Context), so it should be a spec
    /// written for this form. See the trait's own docs.
    const SPEC: &'static FormSpec;

    /// Parse the form out of `ctx`, and honor the flatten prefix in scope.
    ///
    /// Returns `None` when it could not produce a value. It pushes errors into
    /// the context in place of returning them, so parsing always visits every
    /// field.
    fn parse_in(ctx: &mut ParseCtx<'_, Self::Context>) -> Option<Self>;

    /// Write this value out again as raw submitted values, so you can render an
    /// existing record into the form it came from.
    fn fill_in(&self, values: &mut Values, prefix: &str);

    /// [`Form::SPEC`], for a call site that would rather not name the type.
    fn spec() -> &'static FormSpec {
        Self::SPEC
    }

    /// Parse and validate a submission.
    fn from_values_with_context(
        values: &Values,
        context: &Self::Context,
    ) -> Result<Self, FormErrors> {
        let mut ctx = ParseCtx::new(values, context);
        let parsed = Self::parse_in(&mut ctx);
        let errors = ctx.into_errors();
        match parsed {
            Some(value) if errors.is_empty() => Ok(value),
            _ => Err(errors),
        }
    }

    /// [`from_values_with_context`](Form::from_values_with_context), for a
    /// form that needs no context.
    fn from_values(values: &Values) -> Result<Self, FormErrors>
    where
        Self::Context: EmptyContext,
    {
        Self::from_values_with_context(values, empty::<Self>())
    }

    /// Parse and validate an `application/x-www-form-urlencoded` body or query
    /// string.
    fn from_urlencoded_with_context(
        body: &str,
        context: &Self::Context,
    ) -> Result<Self, FormErrors> {
        Self::from_values_with_context(&Values::parse(body), context)
    }

    /// [`from_urlencoded_with_context`](Form::from_urlencoded_with_context),
    /// for a form that needs no context.
    fn from_urlencoded(body: &str) -> Result<Self, FormErrors>
    where
        Self::Context: EmptyContext,
    {
        Self::from_urlencoded_with_context(body, empty::<Self>())
    }

    /// Parse a submission, and on failure build the form to show again.
    fn submit_with_context(values: &Values, context: &Self::Context) -> Outcome<Self> {
        match Self::from_values_with_context(values, context) {
            Ok(value) => Outcome::Valid(value),
            Err(errors) => {
                let view = Box::new(Self::render_submitted_with_context(
                    values, &errors, context,
                ));
                Outcome::Invalid { errors, view }
            }
        }
    }

    /// [`submit_with_context`](Form::submit_with_context), for a form that
    /// needs no context.
    fn submit(values: &Values) -> Outcome<Self>
    where
        Self::Context: EmptyContext,
    {
        Self::submit_with_context(values, empty::<Self>())
    }

    /// [`Form::submit_with_context`] straight from a request body.
    fn submit_urlencoded_with_context(body: &str, context: &Self::Context) -> Outcome<Self> {
        Self::submit_with_context(&Values::parse(body), context)
    }

    /// [`Form::submit`] straight from a request body.
    fn submit_urlencoded(body: &str) -> Outcome<Self>
    where
        Self::Context: EmptyContext,
    {
        Self::submit_urlencoded_with_context(body, empty::<Self>())
    }

    /// A blank form, with each field showing its declared default.
    fn render_with_context(context: &Self::Context) -> FormView {
        FormView::build::<Self>(None, &FormErrors::new(), context)
    }

    /// A blank form, for a form that needs no context.
    fn render() -> FormView
    where
        Self::Context: EmptyContext,
    {
        Self::render_with_context(empty::<Self>())
    }

    /// A blank form with every i18n key already resolved.
    ///
    /// [`FormView::localized`] is the general form. It does the same to a view
    /// from any of the other constructors:
    /// `article.render_filled().localized(&translate)`.
    fn render_localized_with_context<S, F>(translate: F, context: &Self::Context) -> FormView
    where
        F: Fn(&str) -> Option<S>,
        S: Into<std::borrow::Cow<'static, str>>,
    {
        Self::render_with_context(context).localized(translate)
    }

    /// [`render_localized_with_context`](Form::render_localized_with_context),
    /// for a form that needs no context.
    fn render_localized<S, F>(translate: F) -> FormView
    where
        F: Fn(&str) -> Option<S>,
        S: Into<std::borrow::Cow<'static, str>>,
        Self::Context: EmptyContext,
    {
        Self::render_localized_with_context(translate, empty::<Self>())
    }

    /// The form to show after a submission: the submitted values, with each
    /// error attached to its field.
    fn render_submitted_with_context(
        values: &Values,
        errors: &FormErrors,
        context: &Self::Context,
    ) -> FormView {
        FormView::build::<Self>(Some(values), errors, context)
    }

    /// [`render_submitted_with_context`](Form::render_submitted_with_context),
    /// for a form that needs no context.
    fn render_submitted(values: &Values, errors: &FormErrors) -> FormView
    where
        Self::Context: EmptyContext,
    {
        Self::render_submitted_with_context(values, errors, empty::<Self>())
    }

    /// The form filled in from an existing value — an edit form.
    fn render_filled_with_context(&self, context: &Self::Context) -> FormView {
        FormView::build::<Self>(Some(&self.to_values()), &FormErrors::new(), context)
    }

    /// [`render_filled_with_context`](Form::render_filled_with_context), for
    /// a form that needs no context.
    fn render_filled(&self) -> FormView
    where
        Self::Context: EmptyContext,
    {
        self.render_filled_with_context(empty::<Self>())
    }

    /// This value as raw submitted values.
    fn to_values(&self) -> Values {
        let mut values = Values::new();
        self.fill_in(&mut values, "");
        values
    }
}

/// The context of a form that needs no context, named once so that each short
/// method above is its `…_with_context` twin and nothing else.
fn empty<T: Form>() -> &'static T::Context
where
    T::Context: EmptyContext,
{
    <T::Context as EmptyContext>::EMPTY
}
