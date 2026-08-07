//! The declarative description of a form: what the derive macro produces and
//! what both the renderer and the parser read.
//!
//! `const` data builds all of a [`FormSpec`]. `#[derive(Form)]` therefore emits
//! it as the associated constant [`Form::SPEC`](crate::Form::SPEC) and gives
//! out a `&'static` reference to memory the compiler laid out. There is no
//! allocation, no lazy setup, and nothing to run at render time. [`Control`]
//! and everything inside it is `Copy`, which makes the merge the derive
//! performs a plain `const fn`.
//!
//! Every string here is a `&'static str` or a `'static` [`Cow`], which lets the
//! render format borrow all of it in place of copying it. See
//! [`FormView`](crate::FormView).
//!
//! The one thing a spec holds that is not data is a *function pointer*, which
//! is `const` data all the same. A [`FieldDefault`] carries the glue that
//! produces one field's default, and a [`Flattened`] carries the glue that
//! hands a sub-form the context it asks for. Both take the render's context as
//! an erased pointer, because one shared `const` cannot name the type of a
//! context. `#[derive(Form)]` writes each function beside the spec that calls
//! it.
//!
//! Every string a person reads is a [`Text`], such as a label, help text or a
//! legend. A [`Text`] is either literal text or an i18n key. The crate never
//! resolves a key itself. See [`FormView::localize`](crate::FormView::localize).
//!
//! # Where an attribute lives
//!
//! [`Control`] carries the attributes that change **how the crate validates a
//! value**. It also carries the few that exactly one control accepts:
//! `rows`/`cols`, `accept`, and the choice list. Nothing else can name them, so
//! this type cannot hold `minlength` on a date or `accept` on a `<select>`.
//!
//! [`FieldSpec`] carries identity and presentation, such as the label, the help
//! text, `readonly` and `placeholder`. Nearly every control shares those.
//!
//! Anything this crate has no opinion about, such as `data-*`, `hx-*` and
//! `aria-*`, goes into the [`Attr`] list both of them carry. The crate writes
//! that list out as given.

use std::borrow::Cow;
use std::fmt;
use std::ptr::NonNull;

use serde::{Serialize, Serializer};

use crate::FieldKind;

/// A custom attribute. The crate renders it onto the `<form>` or the control
/// as written.
///
/// It carries the markup the crate knows nothing about. It never takes part in
/// validation, and the built-in renderer emits it after every attribute the
/// crate generates itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attr {
    pub name: &'static str,
    /// `None` renders a bare boolean attribute, as `inert` or `hidden` are.
    pub value: Option<&'static str>,
}

impl Attr {
    /// `name="value"`.
    pub const fn new(name: &'static str, value: &'static str) -> Self {
        Self {
            name,
            value: Some(value),
        }
    }

    /// A valueless attribute, such as `inert`.
    pub const fn flag(name: &'static str) -> Self {
        Self { name, value: None }
    }
}

/// A string a person reads: literal text, or a key for an i18n backend.
///
/// In an attribute, how you write the value tells the two apart. `label =
/// "Email address"` is text, and `label = t("signup.email.label")` is a key.
/// The difference reaches the render format, which shows the key next to the
/// string. A template can then resolve the key, and so can
/// [`FormView::localize`](crate::FormView::localize).
///
/// A key that no backend knows stays in place. The view shows the key itself,
/// which is a visible bug rather than a silently blank label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    /// The literal text, or the i18n key. [`Text::is_key`] says which.
    pub content: Cow<'static, str>,
    /// Whether [`Text::content`] is a key to look up rather than text to show.
    pub is_key: bool,
}

impl Text {
    /// Literal text, for a `const` position.
    pub const fn literal(text: &'static str) -> Self {
        Self {
            content: Cow::Borrowed(text),
            is_key: false,
        }
    }

    /// An i18n key, for a `const` position.
    pub const fn key(key: &'static str) -> Self {
        Self {
            content: Cow::Borrowed(key),
            is_key: true,
        }
    }

    /// Literal text built at runtime.
    pub fn owned(text: impl Into<String>) -> Self {
        Self {
            content: Cow::Owned(text.into()),
            is_key: false,
        }
    }

    /// An i18n key built at runtime.
    pub fn owned_key(key: impl Into<String>) -> Self {
        Self {
            content: Cow::Owned(key.into()),
            is_key: true,
        }
    }

    /// The content, whichever kind it is.
    pub fn as_str(&self) -> &str {
        &self.content
    }

    /// The i18n key, or `None` for literal text.
    pub fn key_str(&self) -> Option<&str> {
        self.is_key.then(|| self.content.as_ref())
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}

impl From<&'static str> for Text {
    fn from(text: &'static str) -> Self {
        Text::literal(text)
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Text::owned(text)
    }
}

impl From<Cow<'static, str>> for Text {
    fn from(text: Cow<'static, str>) -> Self {
        Self {
            content: text,
            is_key: false,
        }
    }
}

impl Serialize for Text {
    /// Serializes as the string it holds, so a template renders text whether or
    /// not something has resolved the key. [`FormView`](crate::FormView) makes
    /// the same trade: the key travels in a companion field, not in place of
    /// the text.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.content)
    }
}

/// One selectable value of a `<select>`, a radio group or a checkbox group.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub value: Cow<'static, str>,
    pub label: Text,
    pub disabled: bool,
    /// When set, the crate renders the choice inside an `<optgroup>` with this
    /// label.
    pub group: Option<Text>,
}

impl Choice {
    pub const DEFAULT: Self = Self {
        value: Cow::Borrowed(""),
        label: Text::literal(""),
        disabled: false,
        group: None,
    };

    /// A choice known at compile time, for a `const` position.
    pub const fn new(value: &'static str, label: &'static str) -> Self {
        Self {
            value: Cow::Borrowed(value),
            label: Text::literal(label),
            disabled: false,
            group: None,
        }
    }

    /// A choice whose label is an i18n key, for a `const` position.
    pub const fn keyed(value: &'static str, label_key: &'static str) -> Self {
        Self {
            value: Cow::Borrowed(value),
            label: Text::key(label_key),
            disabled: false,
            group: None,
        }
    }

    /// A choice built at runtime, such as an option that comes from a database.
    pub fn owned(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: Cow::Owned(value.into()),
            label: Text::owned(label),
            disabled: false,
            group: None,
        }
    }

    /// A choice built at runtime whose label is an i18n key.
    pub fn owned_keyed(value: impl Into<String>, label_key: impl Into<String>) -> Self {
        Self {
            value: Cow::Owned(value.into()),
            label: Text::owned_key(label_key),
            disabled: false,
            group: None,
        }
    }
}

// ─── Controls ─────────────────────────────────────────────────────────────────

/// `min`, `max` and `step`, shared by the numeric and the date and time
/// controls.
///
/// The crate keeps them as strings, not numbers. An `i128` or `u64` bound
/// therefore survives exactly, and a date bound needs no calendar type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub min: Option<&'static str>,
    pub max: Option<&'static str>,
    /// `"any"` turns the step check off.
    pub step: Option<&'static str>,
}

impl Bounds {
    pub const DEFAULT: Self = Self {
        min: None,
        max: None,
        step: None,
    };

    /// Whether any of the three is set.
    pub const fn is_empty(&self) -> bool {
        self.min.is_none() && self.max.is_none() && self.step.is_none()
    }
}

impl Default for Bounds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Which kind of free-text `<input>` a [`Control::Text`] is.
///
/// The variants differ in the format check the server makes. That is why they
/// are one enum and not five controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFormat {
    Text,
    Password,
    Tel,
    Search,
    Url,
    /// `multiple` accepts a comma-separated list of addresses, as HTML does.
    /// `multiple` means nothing on any other text control.
    Email {
        multiple: bool,
    },
}

/// An `<input>` that carries free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextControl {
    pub format: TextFormat,
    /// An unanchored regular expression that the whole value must match.
    pub pattern: Option<&'static str>,
    /// In Unicode scalar values, as the HTML spec counts them.
    pub minlength: Option<usize>,
    pub maxlength: Option<usize>,
}

impl TextControl {
    pub const DEFAULT: Self = Self {
        format: TextFormat::Text,
        pattern: None,
        minlength: None,
        maxlength: None,
    };
}

/// A `<textarea>`. It takes lengths, but no `pattern`, unlike an `<input>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextareaControl {
    pub minlength: Option<usize>,
    pub maxlength: Option<usize>,
    pub rows: Option<u32>,
    pub cols: Option<u32>,
}

impl TextareaControl {
    pub const DEFAULT: Self = Self {
        minlength: None,
        maxlength: None,
        rows: None,
        cols: None,
    };
}

/// A spinner or a slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberFormat {
    Number,
    Range,
}

/// An `<input type="number">` or an `<input type="range">`. The crate compares
/// `min`, `max` and `step` as numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberControl {
    pub format: NumberFormat,
    pub bounds: Bounds,
}

impl NumberControl {
    pub const DEFAULT: Self = Self {
        format: NumberFormat::Number,
        bounds: Bounds::DEFAULT,
    };
}

/// Which ISO 8601 shape a [`Control::Temporal`] expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalFormat {
    /// `YYYY-MM-DD`
    Date,
    /// `HH:MM[:SS]`
    Time,
    /// `YYYY-MM-DDTHH:MM`
    DatetimeLocal,
    /// `YYYY-MM`
    Month,
    /// `YYYY-Www`
    Week,
}

/// A date or time `<input>`. The crate compares `min` and `max` as strings,
/// which for these formats *is* the order in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalControl {
    pub format: TemporalFormat,
    pub bounds: Bounds,
}

impl TemporalControl {
    pub const DEFAULT: Self = Self {
        format: TemporalFormat::Date,
        bounds: Bounds::DEFAULT,
    };
}

/// Whether a fixed set of options renders as a menu, as radio buttons, or as
/// checkboxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceStyle {
    Select,
    Radio,
    /// One checkbox per option, all sharing the field's name. It carries many
    /// values by design, so it is the usable alternative to
    /// `<select multiple>`.
    Checkbox,
}

/// A control whose value has to be one of a declared set.
///
/// An empty list means the options arrive at render time, through
/// [`FieldView::set_choices`](crate::FieldView::set_choices). The crate then
/// rejects nothing, because the spec does not know what it may allow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChooseControl {
    pub style: ChoiceStyle,
    /// `<select multiple>`. A radio group always carries one value, and a
    /// checkbox group always carries many. For those two the style decides
    /// this, not the field.
    pub multiple: bool,
    pub choices: &'static [Choice],
}

impl ChooseControl {
    pub const DEFAULT: Self = Self {
        style: ChoiceStyle::Select,
        multiple: false,
        choices: &[],
    };
}

/// An `<input type="file">`. The server checks nothing here again, because the
/// crate never sees the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileControl {
    /// Comma-separated MIME types or extensions, such as `"image/*,.pdf"`.
    pub accept: Option<&'static str>,
    pub multiple: bool,
}

impl FileControl {
    pub const DEFAULT: Self = Self {
        accept: None,
        multiple: false,
    };
}

/// Which control a field renders as, with every attribute that control, and
/// only that control, accepts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    Text(TextControl),
    Textarea(TextareaControl),
    Number(NumberControl),
    Temporal(TemporalControl),
    Choose(ChooseControl),
    Checkbox,
    Color,
    File(FileControl),
    Hidden,
}

impl Control {
    /// A plain `<input type="text">`.
    pub const TEXT: Self = Control::Text(TextControl::DEFAULT);
    /// A plain `<input type="number">`.
    pub const NUMBER: Self = Control::Number(NumberControl::DEFAULT);
    /// A `<select>` with no options declared yet.
    pub const SELECT: Self = Control::Choose(ChooseControl::DEFAULT);

    /// The flat discriminant that the render format uses.
    pub const fn kind(&self) -> FieldKind {
        match self {
            Control::Text(text) => match text.format {
                TextFormat::Text => FieldKind::Text,
                TextFormat::Password => FieldKind::Password,
                TextFormat::Tel => FieldKind::Tel,
                TextFormat::Search => FieldKind::Search,
                TextFormat::Url => FieldKind::Url,
                TextFormat::Email { .. } => FieldKind::Email,
            },
            Control::Textarea(_) => FieldKind::Textarea,
            Control::Number(number) => match number.format {
                NumberFormat::Number => FieldKind::Number,
                NumberFormat::Range => FieldKind::Range,
            },
            Control::Temporal(temporal) => match temporal.format {
                TemporalFormat::Date => FieldKind::Date,
                TemporalFormat::Time => FieldKind::Time,
                TemporalFormat::DatetimeLocal => FieldKind::DatetimeLocal,
                TemporalFormat::Month => FieldKind::Month,
                TemporalFormat::Week => FieldKind::Week,
            },
            Control::Choose(choose) => match choose.style {
                ChoiceStyle::Select => FieldKind::Select,
                ChoiceStyle::Radio => FieldKind::Radio,
                ChoiceStyle::Checkbox => FieldKind::CheckboxGroup,
            },
            Control::Checkbox => FieldKind::Checkbox,
            Control::Color => FieldKind::Color,
            Control::File(_) => FieldKind::File,
            Control::Hidden => FieldKind::Hidden,
        }
    }

    pub const fn pattern(&self) -> Option<&'static str> {
        match self {
            Control::Text(text) => text.pattern,
            _ => None,
        }
    }

    pub const fn minlength(&self) -> Option<usize> {
        match self {
            Control::Text(text) => text.minlength,
            Control::Textarea(area) => area.minlength,
            _ => None,
        }
    }

    pub const fn maxlength(&self) -> Option<usize> {
        match self {
            Control::Text(text) => text.maxlength,
            Control::Textarea(area) => area.maxlength,
            _ => None,
        }
    }

    /// The `min`, `max` and `step` triple, for the two controls that have one.
    pub const fn bounds(&self) -> Option<&Bounds> {
        match self {
            Control::Number(number) => Some(&number.bounds),
            Control::Temporal(temporal) => Some(&temporal.bounds),
            _ => None,
        }
    }

    pub const fn accept(&self) -> Option<&'static str> {
        match self {
            Control::File(file) => file.accept,
            _ => None,
        }
    }

    pub const fn rows(&self) -> Option<u32> {
        match self {
            Control::Textarea(area) => area.rows,
            _ => None,
        }
    }

    pub const fn cols(&self) -> Option<u32> {
        match self {
            Control::Textarea(area) => area.cols,
            _ => None,
        }
    }

    /// Whether the control submits more than one value.
    pub const fn multiple(&self) -> bool {
        match self {
            Control::Text(TextControl {
                format: TextFormat::Email { multiple },
                ..
            }) => *multiple,
            Control::Choose(choose) => choose.multiple,
            Control::File(file) => file.multiple,
            _ => false,
        }
    }

    /// The declared options. It is empty for every control that has none.
    pub const fn choices(&self) -> &'static [Choice] {
        match self {
            Control::Choose(choose) => choose.choices,
            _ => &[],
        }
    }

    /// Whether a submitted value has to be in [`Control::choices`].
    pub const fn restricts_choices(&self) -> bool {
        !self.choices().is_empty()
    }

    /// Whether the control submits nothing when the user leaves it alone. An
    /// absent value then means "unchecked", not "missing".
    pub const fn is_checkable(&self) -> bool {
        matches!(
            self,
            Control::Checkbox
                | Control::Choose(ChooseControl {
                    style: ChoiceStyle::Radio | ChoiceStyle::Checkbox,
                    ..
                })
        )
    }
}

// ─── Defaults ─────────────────────────────────────────────────────────────────

/// The glue that produces one field's default.
///
/// `context` points at the [`Context`](crate::Form::Context) of the form whose
/// spec holds the default. It is an erased pointer because a [`FormSpec`] is
/// one `const` that every render of every caller shares. It cannot name a
/// context type, and it cannot hold a closure over one. `#[derive(Form)]`
/// writes the two halves together: the function that reads the pointer as a
/// `&Context`, and the spec that carries the function.
///
/// What comes back is the string the field renders, whatever type the value
/// started as. That is the one type every default has in common, so the crate
/// names it here in place of erasing it too.
pub type Generate = unsafe fn(context: *const ()) -> Cow<'static, str>;

/// The value a field starts a render with.
///
/// One slot holds every kind of default. `default = "web"` is a literal written
/// into the spec, `default` alone is the field type's own `Default::default()`,
/// and `default = fresh_token` is a function the form runs once per render, for
/// the values a `const` cannot hold: a CSRF token, a nonce, today's date. Each
/// becomes a [`Generate`] the derive writes for that one field, so the renderer
/// makes one call and needs to know nothing about which kind of default it got,
/// nor about the type the value started as.
///
/// A default belongs to *rendering* alone. It never stands in while parsing. If
/// it did, a submission that left the CSRF token out would arrive holding a
/// fresh and valid one.
#[derive(Clone, Copy)]
pub struct FieldDefault {
    generate: Generate,
    literal: Option<&'static str>,
}

impl FieldDefault {
    /// Declare a default, as the derive does.
    ///
    /// `literal` is the text for a default written into the spec, and `None`
    /// for one only a call can produce. The crate calls `generate` either way.
    /// What it reads off `literal` is whether the spec already says what the
    /// value is, which is what tells a value the form owns from one written
    /// down. See [`FieldDefault::is_generated`].
    ///
    /// # Safety
    ///
    /// `generate` has to read its `context` as nothing but a shared reference
    /// to the [`Context`](crate::Form::Context) of the form whose [`FormSpec`]
    /// holds this default. One pointer reaches every field of one render, and
    /// only that form knows what it points at.
    pub const unsafe fn new(generate: Generate, literal: Option<&'static str>) -> Self {
        Self { generate, literal }
    }

    /// The text of a default written into the spec, and `None` for one that
    /// only a call produces.
    pub const fn literal(&self) -> Option<&'static str> {
        self.literal
    }

    /// Whether the form produces this value by calling something, rather than
    /// reading it out of the spec.
    ///
    /// A field that holds such a value holds one nobody typed. A hidden one is
    /// therefore the form's own on every path: the crate mints it again in
    /// place of echoing what came in. See [`FieldSpec::reset`].
    pub const fn is_generated(&self) -> bool {
        self.literal.is_none()
    }

    /// Run the glue.
    ///
    /// # Safety
    ///
    /// `context` has to point at a live [`Context`](crate::Form::Context) of
    /// the form this default belongs to, which is what [`FieldDefault::new`]
    /// promised the glue would receive.
    pub(crate) unsafe fn value(&self, context: *const ()) -> Cow<'static, str> {
        // SAFETY: the caller vouches for the context, and `new` for the glue.
        unsafe { (self.generate)(context) }
    }
}

impl fmt::Debug for FieldDefault {
    /// The literal, or `<generated>` for one only a call produces. The address
    /// of the glue says nothing a reader of a spec can use.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.literal {
            Some(literal) => literal.fmt(f),
            None => f.write_str("<generated>"),
        }
    }
}

impl PartialEq for FieldDefault {
    /// Two defaults are the same when the same glue produces them. A literal
    /// says so outright, and for a generated one there is nothing to compare
    /// but the function itself.
    fn eq(&self, other: &Self) -> bool {
        self.literal == other.literal && std::ptr::fn_addr_eq(self.generate, other.generate)
    }
}

/// How an enclosing form's context becomes the one a flattened sub-form's own
/// defaults receive.
///
/// It is [`Provides::provide`](crate::Provides::provide) with the types erased,
/// for the reason [`Generate`] erases them: one `const` spec describes every
/// render, and the two forms need not share a context type.
#[derive(Clone, Copy)]
pub struct Provider(unsafe fn(context: *const ()) -> *const ());

impl Provider {
    /// The enclosing context, handed down as it stands.
    ///
    /// That is what a sub-form which shares the enclosing form's context type
    /// asks for, and it is the whole of what the blanket
    /// `impl<C> Provides<C> for C` does.
    pub const IDENTITY: Self = Self(|context| context);

    /// Declare a bridge, as the derive does for `#[field(flatten)]`.
    ///
    /// # Safety
    ///
    /// The call has to read its argument as nothing but a shared reference to
    /// the [`Context`](crate::Form::Context) of the form whose [`FormSpec`]
    /// holds this flatten, and to give back a pointer to a live context of the
    /// sub-form's own type.
    pub const unsafe fn new(provide: unsafe fn(context: *const ()) -> *const ()) -> Self {
        Self(provide)
    }

    /// # Safety
    ///
    /// `context` has to point at a live context of the enclosing form.
    pub(crate) unsafe fn provide(self, context: *const ()) -> *const () {
        // SAFETY: the caller vouches for the context, and `new` for the bridge.
        unsafe { (self.0)(context) }
    }
}

impl fmt::Debug for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Provider")
    }
}

// ─── Fields and forms ─────────────────────────────────────────────────────────

/// Everything the crate knows about a single field at compile time.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    /// The submitted name, *relative* to any enclosing flatten prefix.
    pub name: &'static str,
    /// The visible label. `None` renders no `<label>`.
    pub label: Option<Text>,
    /// The control, and the attributes only it accepts.
    pub control: Control,
    /// The value must be present and not empty.
    pub required: bool,
    /// The value a blank form starts with, and the glue that produces it.
    ///
    /// It applies to a render and to nothing else. A blank form shows it, and
    /// so does a field that [resets](FieldSpec::reset). A parse never reads it:
    /// a submission that left a field out left it out. See [`FieldDefault`],
    /// which holds a literal and a generated default alike.
    pub default: Option<FieldDefault>,
    /// The field shows its default on *every* render, and never what the
    /// submission or the record holds.
    ///
    /// It is what a field the form owns rather than the user needs: a CSRF
    /// token, a nonce, a password box that must not come back filled in. The
    /// value the form made is the only one worth showing, and a rejected token
    /// sent back would make the retry fail exactly as the first attempt did.
    ///
    /// This is the whole answer by the time a spec exists, and the renderer
    /// asks nothing else. `#[field(reset)]` writes it down. Where a field says
    /// nothing, the derive decides it: a **hidden** control whose default is
    /// [generated](FieldDefault::is_generated) resets on its own, because it
    /// can hold nothing a caller would miss. `#[field(reset = false)]` turns
    /// even that off.
    pub reset: bool,
    pub placeholder: Option<Text>,
    /// Help text. The crate renders it next to the control and points
    /// `aria-describedby` at it.
    pub help: Option<Text>,
    pub autocomplete: Option<&'static str>,
    /// Overrides the generated `id`.
    pub id: Option<&'static str>,
    pub class: Option<&'static str>,
    pub disabled: bool,
    pub readonly: bool,
    pub autofocus: bool,
    /// Attributes the crate has no opinion about. The crate renders them onto
    /// the control as written. Declare them with `#[field(attr(...))]`.
    pub attrs: &'static [Attr],
}

impl FieldSpec {
    pub const DEFAULT: Self = Self {
        name: "",
        label: None,
        control: Control::TEXT,
        required: false,
        default: None,
        reset: false,
        placeholder: None,
        help: None,
        autocomplete: None,
        id: None,
        class: None,
        disabled: false,
        readonly: false,
        autofocus: false,
        attrs: &[],
    };

    /// The `id` of the control. It defaults to the prefixed field name.
    // Not `&str`. The point is to give back the name itself when the name is
    // already a usable id, which needs the borrow the `Cow` carries.
    #[allow(clippy::ptr_arg)]
    pub fn id_for(&self, full_name: &Cow<'static, str>) -> Cow<'static, str> {
        match self.id {
            Some(id) => Cow::Borrowed(id),
            None => sanitize_id(full_name),
        }
    }
}

/// Turn a field path such as `billing.street` into a usable DOM id.
///
/// A name that is already a usable id comes back untouched, not rebuilt. Every
/// name written as a Rust identifier is already usable.
#[allow(clippy::ptr_arg)] // See `id_for`.
pub(crate) fn sanitize_id(name: &Cow<'static, str>) -> Cow<'static, str> {
    fn is_id_char(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'
    }

    if name.chars().all(is_id_char) {
        return name.clone();
    }
    Cow::Owned(
        name.chars()
            .map(|ch| if is_id_char(ch) { ch } else { '-' })
            .collect(),
    )
}

/// A sub-form put inside an enclosing form.
///
/// `#[field(flatten)]` produces it. It names the sub-form's spec directly.
/// [`Form::SPEC`](crate::Form::SPEC) is an associated *constant*, so the
/// compiler resolves the reference while it const-evaluates the enclosing spec.
/// No call happens at render time.
#[derive(Debug, Clone)]
pub struct Flattened {
    /// The crate puts this in front of every field name of the sub-form. It is
    /// empty for a plain flatten. Set it to use the same sub-form more than
    /// once.
    pub prefix: &'static str,
    /// A legend. The built-in renderer puts it above the group.
    pub legend: Option<Text>,
    pub spec: &'static FormSpec,
    /// What the sub-form's own defaults receive in place of the enclosing
    /// form's context. [`Provider::IDENTITY`] is right wherever the two forms
    /// share a context type, which is every flatten the derive writes without
    /// a [`Provides`](crate::Provides) impl of its own.
    pub context: Provider,
}

/// One item in a form's field list.
// The variants differ in size. Boxing the large one would put the spec out of
// reach of `const` construction, which is the whole point.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Entry {
    Field(FieldSpec),
    Flatten(Flattened),
}

/// How the browser submits a form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormMethod {
    Get,
    Post,
    Dialog,
}

/// The `enctype` that encodes a submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormEncType {
    UrlEncoded,
    MultipartFormData,
    TextPlain,
}

/// The static description of a whole form.
#[derive(Debug, Clone)]
pub struct FormSpec {
    pub id: Option<&'static str>,
    pub name: Option<&'static str>,
    pub action: Option<&'static str>,
    pub method: Option<FormMethod>,
    pub enctype: Option<FormEncType>,
    pub novalidate: bool,
    pub class: Option<&'static str>,
    /// The caption of the submit button the built-in renderer emits.
    pub submit_label: Option<Text>,
    /// Attributes the crate has no opinion about. The crate renders them onto
    /// the `<form>` as written. Declare them with `#[form(attr(...))]`.
    pub attrs: &'static [Attr],
    pub entries: &'static [Entry],
}

/// Guards against a form that flattens itself, directly or through another.
const MAX_FLATTEN_DEPTH: usize = 32;

impl FormSpec {
    pub const DEFAULT: Self = Self {
        id: None,
        name: None,
        action: None,
        method: None,
        enctype: None,
        novalidate: false,
        class: None,
        submit_label: None,
        attrs: &[],
        entries: &[],
    };

    /// The entry at `index`, which the derive knows to be a plain field.
    ///
    /// # Panics
    /// If the index is out of range, or if it names a flattened sub-form.
    /// Generated code never does this.
    pub fn field_at(&self, index: usize) -> &FieldSpec {
        match self.entries.get(index) {
            Some(Entry::Field(f)) => f,
            _ => panic!("form spec entry {index} is not a field"),
        }
    }

    /// The entry at `index`, which the derive knows to be a flattened sub-form.
    ///
    /// # Panics
    /// If the index is out of range, or if it names a plain field.
    pub fn flatten_at(&self, index: usize) -> &Flattened {
        match self.entries.get(index) {
            Some(Entry::Flatten(f)) => f,
            _ => panic!("form spec entry {index} is not a flattened sub-form"),
        }
    }

    /// Visit every field of this form and of any flattened sub-form, in render
    /// order, and resolve each name against the prefixes collected so far.
    ///
    /// This is what the renderer walks. A form that flattens nothing allocates
    /// nothing, because a resolved name *is* the borrowed name in the spec.
    /// Only a field reached through a non-empty prefix needs a new name.
    pub fn walk(&self, mut visit: impl FnMut(ResolvedField)) {
        self.walk_in("", None, 0, None, &mut visit);
    }

    /// [`FormSpec::walk`], with the context that the fields' own defaults
    /// receive. It is the walk a render makes, and the only one that can run a
    /// generated default.
    ///
    /// # Safety
    ///
    /// `context` has to point at a live
    /// [`Context`](crate::Form::Context) of the form this spec describes.
    pub(crate) unsafe fn walk_with_context(
        &self,
        context: NonNull<()>,
        mut visit: impl FnMut(ResolvedField),
    ) {
        self.walk_in("", None, 0, Some(context), &mut visit);
    }

    // `dyn FnMut`, not a second generic parameter. The recursion would
    // otherwise build one copy of this function per nesting depth.
    fn walk_in(
        &self,
        prefix: &str,
        group: Option<&'static Text>,
        depth: usize,
        context: Option<NonNull<()>>,
        visit: &mut dyn FnMut(ResolvedField),
    ) {
        assert!(
            depth < MAX_FLATTEN_DEPTH,
            "form flattening nested more than {MAX_FLATTEN_DEPTH} levels deep — \
             is a form flattening itself?"
        );
        for entry in self.entries {
            match entry {
                Entry::Field(field) => visit(ResolvedField {
                    name: join(prefix, field.name),
                    spec: field,
                    group,
                    context,
                }),
                Entry::Flatten(flat) => {
                    let nested = join(prefix, flat.prefix);
                    let group = flat.legend.as_ref().or(group);
                    // The sub-form's defaults read the context its own `Form`
                    // impl names, which the enclosing one supplies.
                    let context = context.map(|context| {
                        // SAFETY: the walk carries the context of the form
                        // whose spec holds this flatten, which is what the
                        // bridge was written to read.
                        let provided = unsafe { flat.context.provide(context.as_ptr().cast()) };
                        NonNull::new(provided.cast_mut())
                            .expect("a context provider gives back a reference")
                    });
                    flat.spec.walk_in(&nested, group, depth + 1, context, visit);
                }
            }
        }
    }

    /// Every field of this form and of any flattened sub-form, in render order,
    /// in one `Vec`. [`FormSpec::walk`] does the same without the `Vec`.
    pub fn fields(&self) -> Vec<ResolvedField> {
        let mut out = Vec::new();
        self.walk(|field| out.push(field));
        out
    }

    /// Find a resolved field by its full, prefixed name.
    pub fn field(&self, full_name: &str) -> Option<ResolvedField> {
        let mut found = None;
        self.walk(|field| {
            if found.is_none() && field.name == full_name {
                found = Some(field);
            }
        });
        found
    }
}

/// `prefix` and `name` joined. With no prefix to add, this borrows `name`
/// outright, which covers every field of a form that flattens nothing.
pub(crate) fn join(prefix: &str, name: &'static str) -> Cow<'static, str> {
    if prefix.is_empty() {
        return Cow::Borrowed(name);
    }
    let mut joined = String::with_capacity(prefix.len() + name.len());
    joined.push_str(prefix);
    joined.push_str(name);
    Cow::Owned(joined)
}

/// A field with the full name the browser submits it under.
#[derive(Debug, Clone)]
pub struct ResolvedField {
    /// The fully qualified submitted name, with every flatten prefix. It is a
    /// [`Cow`] because a prefix has to join the name in the spec, and only when
    /// a prefix exists.
    pub name: Cow<'static, str>,
    pub spec: &'static FieldSpec,
    /// The legend of the innermost flattened group that holds this field.
    pub group: Option<&'static Text>,
    /// The context this field's own default receives, which the walk resolved
    /// through every flatten it passed. It is `None` for a walk that carries
    /// none, which is every walk but a render's.
    context: Option<NonNull<()>>,
}

impl ResolvedField {
    /// The value this field starts a render with.
    ///
    /// It runs the glue in the spec, once, here, and only where the render asks
    /// for it. A generated default is therefore never produced for a field that
    /// is about to show what the user typed instead, which matters for a
    /// generator that hands out something the server also records.
    ///
    /// A walk with no context can still answer for a literal, because no call
    /// produces one. A generated default needs the render that asked for it.
    pub(crate) fn default(&self) -> Option<Cow<'static, str>> {
        let default = self.spec.default?;
        match self.context {
            // SAFETY: the walk carried the context of the form this field
            // belongs to, which is the one its glue was written to read.
            Some(context) => Some(unsafe { default.value(context.as_ptr().cast()) }),
            None => default.literal().map(Cow::Borrowed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Text ─────────────────────────────────────────────────────────────────

    #[test]
    fn text_is_either_something_to_show_or_something_to_look_up() {
        let literal = Text::literal("Email address");
        assert!(!literal.is_key);
        assert_eq!(literal.as_str(), "Email address");
        assert_eq!(literal.key_str(), None);

        let key = Text::key("signup.email.label");
        assert!(key.is_key);
        assert_eq!(key.as_str(), "signup.email.label");
        assert_eq!(key.key_str(), Some("signup.email.label"));
    }

    #[test]
    fn either_kind_can_also_be_built_at_runtime() {
        assert_eq!(Text::owned(String::from("Row 3")), Text::literal("Row 3"));
        assert_eq!(Text::owned_key("a.b"), Text::key("a.b"));
        assert_eq!(Text::owned_key("a.b").key_str(), Some("a.b"));
    }

    #[test]
    fn text_is_empty_when_it_holds_nothing() {
        assert!(Text::literal("").is_empty());
        assert!(!Text::literal(" ").is_empty());
        assert!(!Text::key("a.b").is_empty());
    }

    /// However a string arrives, it is text to show until something says it is
    /// a key. Only `t("…")` and the two constructors say so.
    #[test]
    fn a_plain_string_of_any_type_becomes_literal_text() {
        for text in [
            Text::from("Email"),
            Text::from("Email".to_owned()),
            Text::from(Cow::Borrowed("Email")),
            Text::from(Cow::Owned("Email".to_owned())),
        ] {
            assert!(!text.is_key);
            assert_eq!(text.as_str(), "Email");
        }
    }

    /// A template renders the text whether or not something has resolved the
    /// key, so the key travels in a companion field and never in place of it.
    #[test]
    fn text_serialises_as_the_string_it_holds() {
        assert_eq!(
            serde_json::to_value(Text::literal("Email")).unwrap(),
            serde_json::json!("Email")
        );
        assert_eq!(
            serde_json::to_value(Text::key("signup.email")).unwrap(),
            serde_json::json!("signup.email")
        );
    }

    // ─── Attributes and choices ───────────────────────────────────────────────

    #[test]
    fn an_attribute_is_a_pair_or_a_bare_flag() {
        assert_eq!(
            Attr::new("data-role", "primary"),
            Attr {
                name: "data-role",
                value: Some("primary")
            }
        );
        assert_eq!(Attr::flag("inert").value, None);
    }

    #[test]
    fn a_choice_can_be_written_in_a_const_or_built_at_runtime() {
        let compile_time = Choice::new("de", "Germany");
        assert_eq!(compile_time.value, "de");
        assert_eq!(compile_time.label, Text::literal("Germany"));
        assert!(!compile_time.disabled);
        assert_eq!(compile_time.group, None);

        assert_eq!(Choice::owned("de", "Germany"), compile_time);
        assert_eq!(
            Choice::keyed("de", "country.de"),
            Choice::owned_keyed("de", "country.de")
        );
        assert!(Choice::keyed("de", "country.de").label.is_key);

        assert_eq!(Choice::DEFAULT.value, "");
        assert!(Choice::DEFAULT.label.is_empty());
    }

    // ─── What each control accepts ────────────────────────────────────────────

    #[test]
    fn each_control_reports_the_flat_kind_it_renders_as() {
        let cases: &[(Control, FieldKind)] = &[
            (Control::TEXT, FieldKind::Text),
            (text(TextFormat::Password), FieldKind::Password),
            (text(TextFormat::Tel), FieldKind::Tel),
            (text(TextFormat::Search), FieldKind::Search),
            (text(TextFormat::Url), FieldKind::Url),
            (
                text(TextFormat::Email { multiple: false }),
                FieldKind::Email,
            ),
            (text(TextFormat::Email { multiple: true }), FieldKind::Email),
            (
                Control::Textarea(TextareaControl::DEFAULT),
                FieldKind::Textarea,
            ),
            (Control::NUMBER, FieldKind::Number),
            (number(NumberFormat::Range), FieldKind::Range),
            (temporal(TemporalFormat::Date), FieldKind::Date),
            (temporal(TemporalFormat::Time), FieldKind::Time),
            (
                temporal(TemporalFormat::DatetimeLocal),
                FieldKind::DatetimeLocal,
            ),
            (temporal(TemporalFormat::Month), FieldKind::Month),
            (temporal(TemporalFormat::Week), FieldKind::Week),
            (Control::SELECT, FieldKind::Select),
            (choose(ChoiceStyle::Radio), FieldKind::Radio),
            (choose(ChoiceStyle::Checkbox), FieldKind::CheckboxGroup),
            (Control::Checkbox, FieldKind::Checkbox),
            (Control::Color, FieldKind::Color),
            (Control::File(FileControl::DEFAULT), FieldKind::File),
            (Control::Hidden, FieldKind::Hidden),
        ];
        for (control, kind) in cases {
            assert_eq!(control.kind(), *kind, "{control:?}");
        }
    }

    fn text(format: TextFormat) -> Control {
        Control::Text(TextControl {
            format,
            ..TextControl::DEFAULT
        })
    }

    fn number(format: NumberFormat) -> Control {
        Control::Number(NumberControl {
            format,
            ..NumberControl::DEFAULT
        })
    }

    fn temporal(format: TemporalFormat) -> Control {
        Control::Temporal(TemporalControl {
            format,
            ..TemporalControl::DEFAULT
        })
    }

    fn choose(style: ChoiceStyle) -> Control {
        Control::Choose(ChooseControl {
            style,
            ..ChooseControl::DEFAULT
        })
    }

    /// An accessor answers for the controls that have the attribute, and
    /// `None` for every other one. That is what lets the renderer ask without
    /// matching on the control first.
    #[test]
    fn an_attribute_only_answers_from_the_control_that_accepts_it() {
        let text = Control::Text(TextControl {
            pattern: Some("[a-z]+"),
            minlength: Some(2),
            maxlength: Some(8),
            ..TextControl::DEFAULT
        });
        assert_eq!(text.pattern(), Some("[a-z]+"));
        assert_eq!(text.minlength(), Some(2));
        assert_eq!(text.maxlength(), Some(8));
        assert_eq!(text.bounds(), None);
        assert_eq!(text.rows(), None);
        assert_eq!(text.accept(), None);

        let area = Control::Textarea(TextareaControl {
            minlength: Some(2),
            maxlength: Some(8),
            rows: Some(5),
            cols: Some(40),
        });
        assert_eq!(area.pattern(), None, "a textarea has no pattern");
        assert_eq!(area.minlength(), Some(2));
        assert_eq!(area.maxlength(), Some(8));
        assert_eq!(area.rows(), Some(5));
        assert_eq!(area.cols(), Some(40));

        let bounds = Bounds {
            min: Some("0"),
            max: Some("10"),
            step: Some("1"),
        };
        assert_eq!(
            Control::Number(NumberControl {
                bounds,
                ..NumberControl::DEFAULT
            })
            .bounds(),
            Some(&bounds)
        );
        assert_eq!(
            Control::Temporal(TemporalControl {
                bounds,
                ..TemporalControl::DEFAULT
            })
            .bounds(),
            Some(&bounds)
        );

        let file = Control::File(FileControl {
            accept: Some("image/*"),
            multiple: true,
        });
        assert_eq!(file.accept(), Some("image/*"));
        assert_eq!(Control::Checkbox.accept(), None);
        assert_eq!(Control::Checkbox.minlength(), None);
        assert_eq!(Control::Checkbox.maxlength(), None);
        assert_eq!(Control::Checkbox.rows(), None);
        assert_eq!(Control::Checkbox.cols(), None);
        assert_eq!(Control::Checkbox.bounds(), None);
    }

    /// Three unrelated controls submit more than one value, so one question
    /// covers all three.
    #[test]
    fn a_control_says_whether_it_submits_more_than_one_value() {
        assert!(text(TextFormat::Email { multiple: true }).multiple());
        assert!(!text(TextFormat::Email { multiple: false }).multiple());
        assert!(!text(TextFormat::Text).multiple());
        assert!(
            Control::Choose(ChooseControl {
                multiple: true,
                ..ChooseControl::DEFAULT
            })
            .multiple()
        );
        assert!(
            Control::File(FileControl {
                multiple: true,
                accept: None
            })
            .multiple()
        );
        assert!(!Control::Checkbox.multiple());
    }

    #[test]
    fn only_a_chooser_declares_options_and_so_only_it_restricts_them() {
        const CHOICES: &[Choice] = &[Choice::new("de", "Germany")];
        let declared = Control::Choose(ChooseControl {
            choices: CHOICES,
            ..ChooseControl::DEFAULT
        });
        assert_eq!(declared.choices().len(), 1);
        assert!(declared.restricts_choices());

        // Options that arrive at render time are not in the spec, so there is
        // nothing here to restrict the value to.
        assert!(Control::SELECT.choices().is_empty());
        assert!(!Control::SELECT.restricts_choices());
        assert!(Control::TEXT.choices().is_empty());
        assert!(!Control::TEXT.restricts_choices());
    }

    /// A control that submits nothing when the user leaves it alone is the one
    /// case where an absent value means "unchecked" and not "missing".
    #[test]
    fn the_checkable_controls_are_the_box_and_the_two_groups() {
        assert!(Control::Checkbox.is_checkable());
        assert!(choose(ChoiceStyle::Radio).is_checkable());
        assert!(choose(ChoiceStyle::Checkbox).is_checkable());

        assert!(!choose(ChoiceStyle::Select).is_checkable());
        assert!(!Control::TEXT.is_checkable());
        assert!(!Control::Hidden.is_checkable());
    }

    // ─── Ids ──────────────────────────────────────────────────────────────────

    /// Every name written as a Rust identifier is already a usable id, so it
    /// comes back untouched rather than rebuilt.
    #[test]
    fn a_name_that_is_already_a_usable_id_is_borrowed_not_built() {
        for name in ["email", "billing_street", "a-b", "x1", ""] {
            let name = Cow::Borrowed(name);
            assert!(
                matches!(sanitize_id(&name), Cow::Borrowed(_)),
                "{name:?} needed no rebuilding"
            );
            assert_eq!(sanitize_id(&name), name);
        }
    }

    #[test]
    fn every_other_character_of_a_field_path_becomes_a_hyphen() {
        assert_eq!(
            sanitize_id(&Cow::Borrowed("billing.street")),
            "billing-street"
        );
        assert_eq!(sanitize_id(&Cow::Borrowed("a[0].b")), "a-0--b");
        assert_eq!(sanitize_id(&Cow::Borrowed("\u{e9}")), "-");
    }

    #[test]
    fn a_field_may_name_its_own_id_instead() {
        let named = FieldSpec {
            id: Some("custom-id"),
            ..FieldSpec::DEFAULT
        };
        assert_eq!(named.id_for(&Cow::Borrowed("billing.street")), "custom-id");
        assert_eq!(
            FieldSpec::DEFAULT.id_for(&Cow::Borrowed("billing.street")),
            "billing-street"
        );
    }

    // ─── Walking a spec ───────────────────────────────────────────────────────

    const STREET: FieldSpec = FieldSpec {
        name: "street",
        ..FieldSpec::DEFAULT
    };
    const CITY: FieldSpec = FieldSpec {
        name: "city",
        ..FieldSpec::DEFAULT
    };
    static ADDRESS: FormSpec = FormSpec {
        entries: &[Entry::Field(STREET), Entry::Field(CITY)],
        ..FormSpec::DEFAULT
    };
    static ORDER: FormSpec = FormSpec {
        entries: &[
            Entry::Field(FieldSpec {
                name: "email",
                ..FieldSpec::DEFAULT
            }),
            Entry::Flatten(Flattened {
                prefix: "billing_",
                legend: Some(Text::literal("Billing")),
                spec: &ADDRESS,
                context: Provider::IDENTITY,
            }),
            Entry::Flatten(Flattened {
                prefix: "",
                legend: None,
                spec: &ADDRESS,
                context: Provider::IDENTITY,
            }),
        ],
        ..FormSpec::DEFAULT
    };

    #[test]
    fn a_prefix_is_only_built_where_there_is_one_to_add() {
        assert_eq!(join("", "street"), "street");
        assert!(matches!(join("", "street"), Cow::Borrowed(_)));
        assert_eq!(join("billing_", "street"), "billing_street");
        assert!(matches!(join("billing_", "street"), Cow::Owned(_)));
    }

    #[test]
    fn walking_visits_every_field_of_every_sub_form_in_render_order() {
        let fields = ORDER.fields();
        assert_eq!(
            fields.iter().map(|f| f.name.as_ref()).collect::<Vec<_>>(),
            ["email", "billing_street", "billing_city", "street", "city"]
        );
        // A field takes the legend of the innermost group that holds it, and
        // a field outside every group takes none.
        assert_eq!(fields[0].group, None);
        assert_eq!(fields[1].group.map(Text::as_str), Some("Billing"));
        assert_eq!(fields[3].group, None);
        // The spec each one resolves to is the one in the sub-form.
        assert_eq!(fields[1].spec.name, "street");
    }

    #[test]
    fn a_field_can_be_found_by_the_name_it_is_submitted_under() {
        assert_eq!(ORDER.field("billing_city").unwrap().spec.name, "city");
        assert_eq!(ORDER.field("email").unwrap().name, "email");
        assert!(ORDER.field("city").is_some(), "the unprefixed copy");
        assert!(ORDER.field("nothing_here").is_none());
    }

    #[test]
    fn an_entry_can_be_read_by_index_once_its_kind_is_known() {
        assert_eq!(ORDER.field_at(0).name, "email");
        assert_eq!(ORDER.flatten_at(1).prefix, "billing_");
        assert_eq!(ORDER.flatten_at(1).spec.entries.len(), 2);
    }

    /// Generated code never asks for the wrong kind, so this is a bug in the
    /// caller rather than a case to report.
    #[test]
    #[should_panic(expected = "is not a field")]
    fn asking_for_a_field_where_a_sub_form_stands_panics() {
        ORDER.field_at(1);
    }

    #[test]
    #[should_panic(expected = "is not a flattened sub-form")]
    fn asking_for_a_sub_form_where_a_field_stands_panics() {
        ORDER.flatten_at(0);
    }

    #[test]
    #[should_panic(expected = "is not a field")]
    fn asking_past_the_end_panics_too() {
        ORDER.field_at(99);
    }

    /// A `const` spec can name itself through a `static`, which a walk would
    /// otherwise follow forever.
    #[test]
    #[should_panic(expected = "flattening itself")]
    fn a_form_that_flattens_itself_is_caught_rather_than_looping() {
        static LOOP: FormSpec = FormSpec {
            entries: &[Entry::Flatten(Flattened {
                prefix: "x_",
                legend: None,
                spec: &LOOP,
                context: Provider::IDENTITY,
            })],
            ..FormSpec::DEFAULT
        };
        LOOP.walk(|_| {});
    }

    #[test]
    fn a_form_with_no_entries_walks_nothing() {
        assert!(FormSpec::DEFAULT.fields().is_empty());
        assert!(FormSpec::DEFAULT.field("anything").is_none());
    }
}
