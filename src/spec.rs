//! The declarative description of a form: what the derive macro produces and
//! what both the renderer and the parser read.
//!
//! A [`FormSpec`] is built entirely from `const` data so `#[derive(WebForm)]`
//! can emit it as a `static` item and hand out a `&'static` reference — no
//! allocation, no lazy initialisation. [`Control`] and everything inside it is
//! `Copy`, which is what makes the merge the derive performs a plain `const fn`.
//!
//! Every string a person reads — a label, help text, a legend — is a [`Text`],
//! which is either literal text or an i18n key. The crate never resolves a key
//! itself; see [`FormView::localize`](crate::FormView::localize).
//!
//! # Where an attribute lives
//!
//! [`Control`] carries the attributes that change **how a value is validated**,
//! plus the handful that exactly one control accepts (`rows`/`cols`, `accept`,
//! the choice list). Nothing else can name them, so `minlength` on a date or
//! `accept` on a `<select>` are not states this type can be in.
//!
//! [`FieldSpec`] carries identity and presentation — the label, the help text,
//! `readonly`, `placeholder` — which nearly every control shares.
//!
//! Anything this crate has no opinion about — `data-*`, `hx-*`, `aria-*` — goes
//! into the [`Attr`] list both of them carry, and is written out verbatim.

use std::borrow::Cow;

use crate::FieldKind;

/// A custom attribute, rendered onto the `<form>` or the control as written.
///
/// This is the escape hatch for markup the crate knows nothing about. It never
/// takes part in validation, and the built-in renderer emits it after every
/// attribute the crate generates itself.
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
/// In an attribute the two are told apart by how they are written — `label =
/// "Email address"` is text, `label = t("signup.email.label")` is a key. Which
/// one it is survives all the way into the render format, where the key is
/// exposed alongside the string so a template can resolve it, or
/// [`FormView::localize`](crate::FormView::localize) can.
///
/// A key that no backend recognises stays put: the view shows the key itself,
/// which is a visible bug rather than a silently blank label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    /// The literal text, or the i18n key — [`Text::is_key`] says which.
    pub content: Cow<'static, str>,
    /// Whether [`Text::content`] is a key to look up rather than text to show.
    pub is_key: bool,
}

impl Text {
    /// Literal text, usable in `const` position.
    pub const fn literal(text: &'static str) -> Self {
        Self {
            content: Cow::Borrowed(text),
            is_key: false,
        }
    }

    /// An i18n key, usable in `const` position.
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

/// One selectable value of a `<select>`, a radio group or a checkbox group.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub value: Cow<'static, str>,
    pub label: Text,
    pub disabled: bool,
    /// When set, the choice is rendered inside an `<optgroup>` with this label.
    pub group: Option<Text>,
}

impl Choice {
    pub const DEFAULT: Self = Self {
        value: Cow::Borrowed(""),
        label: Text::literal(""),
        disabled: false,
        group: None,
    };

    /// A choice known at compile time, usable in `const` position.
    pub const fn new(value: &'static str, label: &'static str) -> Self {
        Self {
            value: Cow::Borrowed(value),
            label: Text::literal(label),
            disabled: false,
            group: None,
        }
    }

    /// A choice whose label is an i18n key, usable in `const` position.
    pub const fn keyed(value: &'static str, label_key: &'static str) -> Self {
        Self {
            value: Cow::Borrowed(value),
            label: Text::key(label_key),
            disabled: false,
            group: None,
        }
    }

    /// A choice built at runtime — options loaded from a database, say.
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

/// `min` / `max` / `step`, shared by the numeric and the date/time controls.
///
/// Kept as strings rather than numbers so that an `i128` or `u64` bound
/// survives exactly, and so that a date bound needs no calendar type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub min: Option<&'static str>,
    pub max: Option<&'static str>,
    /// `"any"` disables the step check.
    pub step: Option<&'static str>,
}

impl Bounds {
    pub const DEFAULT: Self = Self {
        min: None,
        max: None,
        step: None,
    };

    /// Whether anything is set at all.
    pub const fn is_empty(&self) -> bool {
        self.min.is_none() && self.max.is_none() && self.step.is_none()
    }
}

impl Default for Bounds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Which flavour of free-text `<input>` a [`Control::Text`] is.
///
/// The variants differ in the format check applied on the server, which is why
/// they are one enum rather than five controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFormat {
    Text,
    Password,
    Tel,
    Search,
    Url,
    /// `multiple` accepts a comma-separated list of addresses, as HTML does.
    /// No other text control has a meaning for `multiple`.
    Email {
        multiple: bool,
    },
}

/// `<input>` carrying free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextControl {
    pub format: TextFormat,
    /// An (unanchored) regular expression the whole value must match.
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

/// `<textarea>`, which takes lengths but — unlike an `<input>` — no `pattern`.
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

/// `<input type="number">` or `<input type="range">`: `min`/`max`/`step` compare
/// numerically.
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

/// A date or time `<input>`: `min`/`max` compare lexicographically, which for
/// these formats *is* chronological order.
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
    /// One checkbox per option, all sharing the field's name. Multi-valued by
    /// construction, so it is the usable alternative to `<select multiple>`.
    Checkbox,
}

/// A control whose value has to be one of a declared set.
///
/// An empty list means the options are supplied at render time, through
/// [`FieldView::set_choices`](crate::FieldView::set_choices); nothing is
/// rejected in that case, because the spec does not know what is allowed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChooseControl {
    pub style: ChoiceStyle,
    /// `<select multiple>`. A radio group is single-valued by definition and a
    /// checkbox group multi-valued by definition, so for those two the style
    /// decides this rather than the field.
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

/// `<input type="file">`. Nothing here is re-checked on the server: the crate
/// never sees the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileControl {
    /// Comma-separated MIME types / extensions, e.g. `"image/*,.pdf"`.
    pub accept: Option<&'static str>,
    pub multiple: bool,
}

impl FileControl {
    pub const DEFAULT: Self = Self {
        accept: None,
        multiple: false,
    };
}

/// Which control a field renders as, together with every attribute that
/// control — and only that control — accepts.
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

    /// The flat discriminant the render format uses.
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

    /// The `min`/`max`/`step` triple, for the two controls that have one.
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

    /// The declared options, empty for everything that has none.
    pub const fn choices(&self) -> &'static [Choice] {
        match self {
            Control::Choose(choose) => choose.choices,
            _ => &[],
        }
    }

    /// Whether a submitted value has to appear in [`Control::choices`].
    pub const fn restricts_choices(&self) -> bool {
        !self.choices().is_empty()
    }

    /// Whether the control submits nothing when the user leaves it alone, so
    /// that an absent value means "unchecked" rather than "missing".
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

// ─── Fields and forms ─────────────────────────────────────────────────────────

/// Everything known statically about a single field.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    /// The submitted name, *relative* to any enclosing flatten prefix.
    pub name: &'static str,
    /// Visible label. `None` renders no `<label>`.
    pub label: Option<Text>,
    /// The control, and the attributes only it accepts.
    pub control: Control,
    /// A value must be present and non-empty.
    pub required: bool,
    /// Pre-filled value for a blank form. Also used when the field is entirely
    /// absent from a submission (but *not* when it is submitted empty, so that
    /// clearing a field keeps working).
    pub default: Option<&'static str>,
    pub placeholder: Option<Text>,
    /// Help text rendered next to the control and wired up via `aria-describedby`.
    pub help: Option<Text>,
    pub autocomplete: Option<&'static str>,
    /// Overrides the generated `id`.
    pub id: Option<&'static str>,
    pub class: Option<&'static str>,
    pub disabled: bool,
    pub readonly: bool,
    pub autofocus: bool,
    /// Attributes the crate has no opinion about, rendered onto the control
    /// verbatim. Declared with `#[field(attr(...))]`.
    pub attrs: &'static [Attr],
}

impl FieldSpec {
    pub const DEFAULT: Self = Self {
        name: "",
        label: None,
        control: Control::TEXT,
        required: false,
        default: None,
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

    /// The `id` used for the control, defaulting to the (prefixed) field name.
    pub fn id_for(&self, full_name: &str) -> String {
        match self.id {
            Some(id) => id.to_owned(),
            None => sanitize_id(full_name),
        }
    }
}

/// Turn a field path such as `billing.street` into something usable as a DOM id.
pub(crate) fn sanitize_id(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    out
}

/// A sub-form spliced into an enclosing form.
///
/// Produced by `#[field(flatten)]`. The sub-form's spec is referenced through a
/// function pointer rather than a reference so that the two `static` items do
/// not have to be initialised in any particular order.
#[derive(Debug, Clone)]
pub struct Flattened {
    /// Prepended to every field name of the sub-form. Empty for a plain flatten;
    /// set it to embed the same sub-form more than once.
    pub prefix: &'static str,
    /// A legend rendered above the group by the built-in renderer.
    pub legend: Option<Text>,
    pub spec: fn() -> &'static FormSpec,
}

/// One item in a form's field list.
// The variants differ in size, but boxing the large one would put the spec out
// of reach of `const` construction, which is the whole point.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Entry {
    Field(FieldSpec),
    Flatten(Flattened),
}

/// How a form is submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormMethod {
    Get,
    Post,
    Dialog,
}

/// The `enctype` a submission is encoded with.
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
    /// Caption of the submit button emitted by the built-in renderer.
    pub submit_label: Option<Text>,
    /// Attributes the crate has no opinion about, rendered onto the `<form>`
    /// verbatim. Declared with `#[form(attr(...))]`.
    pub attrs: &'static [Attr],
    pub entries: &'static [Entry],
}

/// Guards against a form that (directly or indirectly) flattens itself.
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
    /// If the index is out of range or names a flattened sub-form. Generated
    /// code never does this.
    pub fn field_at(&self, index: usize) -> &FieldSpec {
        match self.entries.get(index) {
            Some(Entry::Field(f)) => f,
            _ => panic!("form spec entry {index} is not a field"),
        }
    }

    /// The entry at `index`, which the derive knows to be a flattened sub-form.
    ///
    /// # Panics
    /// If the index is out of range or names a plain field.
    pub fn flatten_at(&self, index: usize) -> &Flattened {
        match self.entries.get(index) {
            Some(Entry::Flatten(f)) => f,
            _ => panic!("form spec entry {index} is not a flattened sub-form"),
        }
    }

    /// Every field of this form and of any flattened sub-form, in render order,
    /// with names resolved against the accumulated prefixes.
    pub fn fields(&self) -> Vec<ResolvedField> {
        let mut out = Vec::new();
        self.collect_fields("", None, 0, &mut out);
        out
    }

    fn collect_fields(
        &self,
        prefix: &str,
        group: Option<&'static Text>,
        depth: usize,
        out: &mut Vec<ResolvedField>,
    ) {
        assert!(
            depth < MAX_FLATTEN_DEPTH,
            "form flattening nested more than {MAX_FLATTEN_DEPTH} levels deep — \
             is a form flattening itself?"
        );
        for entry in self.entries {
            match entry {
                Entry::Field(field) => out.push(ResolvedField {
                    name: format!("{prefix}{}", field.name),
                    spec: field,
                    group,
                }),
                Entry::Flatten(flat) => {
                    let sub = (flat.spec)();
                    let nested = format!("{prefix}{}", flat.prefix);
                    let group = flat.legend.as_ref().or(group);
                    sub.collect_fields(&nested, group, depth + 1, out);
                }
            }
        }
    }

    /// Look up a resolved field by its full (prefixed) name.
    pub fn field(&self, full_name: &str) -> Option<ResolvedField> {
        self.fields().into_iter().find(|f| f.name == full_name)
    }
}

/// A field together with the full name it is submitted under.
#[derive(Debug, Clone)]
pub struct ResolvedField {
    /// Fully-qualified submitted name, including any flatten prefixes. Owned
    /// because prefixes are concatenated while walking the spec.
    pub name: String,
    pub spec: &'static FieldSpec,
    /// The legend of the innermost flattened group this field came from.
    pub group: Option<&'static Text>,
}
