//! Validation and conversion errors.
//!
//! Parsing does not stop at the first problem. The crate tries every field and
//! records every failure in a [`FormErrors`], so the caller gets the whole
//! picture in one pass.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use serde::{Serialize, Serializer};

use crate::spec::Text;

/// Why the crate rejected a value.
///
/// Each variant carries the constraint the value broke, so a caller can render
/// its own translated message in place of the built-in English one.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorKind {
    /// Required, but missing or blank.
    Required,
    /// Present, but the crate cannot convert it to the field's Rust type.
    Invalid { expected: Cow<'static, str> },
    /// Did not match `pattern`.
    Pattern { pattern: Cow<'static, str> },
    /// Shorter than `minlength`.
    TooShort { minlength: usize, length: usize },
    /// Longer than `maxlength`.
    TooLong { maxlength: usize, length: usize },
    /// Below `min`.
    TooSmall { min: Cow<'static, str> },
    /// Above `max`.
    TooLarge { max: Cow<'static, str> },
    /// Not on the `step` grid.
    Step { step: Cow<'static, str> },
    /// Not one of the field's declared choices.
    NotAChoice,
    /// A `#[field(validate = ...)]` or `#[form(validate = ...)]` function
    /// produced this error.
    ///
    /// This kind carries no constraint, unlike the others. A caller therefore
    /// matches on `code` to tell one custom rejection from another, and to
    /// render its own message. The crate sets `code` from the i18n key when the
    /// validator returned one, because the key is also the code. You can also
    /// name the code with [`FieldError::coded`]. A validator that returned only
    /// a message, or `false`, leaves `code` as `None`.
    Custom {
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<Cow<'static, str>>,
    },
}

impl ErrorKind {
    /// The built-in English message for this kind.
    ///
    /// A kind whose message says nothing about the value carries a `'static`
    /// message. That covers most submissions, because users produce `Required`
    /// more than any other error.
    pub fn default_message(&self) -> Cow<'static, str> {
        match self {
            ErrorKind::Required => "This field is required.".into(),
            ErrorKind::Invalid { expected } => format!("Enter {expected}.").into(),
            ErrorKind::Pattern { .. } => "This value is not in the expected format.".into(),
            ErrorKind::TooShort { minlength, length } => format!(
                "Enter at least {minlength} character{} (currently {length}).",
                plural(*minlength)
            )
            .into(),
            ErrorKind::TooLong { maxlength, length } => format!(
                "Enter at most {maxlength} character{} (currently {length}).",
                plural(*maxlength)
            )
            .into(),
            // A bound on a date reads as a point in time. A bound on a number
            // reads as a quantity.
            ErrorKind::TooSmall { min } if is_number(min) => {
                format!("Must be {min} or more.").into()
            }
            ErrorKind::TooSmall { min } => format!("Must be {min} or later.").into(),
            ErrorKind::TooLarge { max } if is_number(max) => {
                format!("Must be {max} or less.").into()
            }
            ErrorKind::TooLarge { max } => format!("Must be {max} or earlier.").into(),
            ErrorKind::Step { step } => format!("Must be a multiple of {step}.").into(),
            ErrorKind::NotAChoice => "Select one of the available options.".into(),
            ErrorKind::Custom { .. } => "This value is not valid.".into(),
        }
    }
}

fn is_number(value: &str) -> bool {
    value.parse::<f64>().is_ok()
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// A single rejection, with a message ready to display.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldError {
    pub kind: ErrorKind,
    /// What to show the person who submitted the form: literal text, or an
    /// i18n key like every other person-facing string in the crate.
    /// [`FormView::localize`](crate::FormView::localize) resolves a key with
    /// the labels. Or leave the key in place and translate it from the view.
    pub message: Text,
}

impl FieldError {
    /// An error that carries the built-in message for its kind.
    pub fn new(kind: ErrorKind) -> Self {
        let message = kind.default_message().into();
        Self { kind, message }
    }

    /// An error with a message the caller supplies.
    pub fn with_message(kind: ErrorKind, message: impl Into<Text>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// The error a `validate = ...` function produces.
    ///
    /// A message written as an i18n key is also the error's
    /// [code](ErrorKind::Custom). A validator that returns
    /// `Err(Text::key("signup.username.reserved"))` therefore produces an error
    /// that a caller can both translate and match on.
    pub fn custom(message: impl Into<Text>) -> Self {
        let message = message.into();
        let code = message.is_key.then(|| message.content.clone());
        Self {
            kind: ErrorKind::Custom { code },
            message,
        }
    }

    /// A custom error whose code is not its message. Use it for a validator
    /// that wants a stable name for the failure *and* a message that reads
    /// well on its own.
    pub fn coded(code: impl Into<Cow<'static, str>>, message: impl Into<Text>) -> Self {
        Self::with_message(
            ErrorKind::Custom {
                code: Some(code.into()),
            },
            message,
        )
    }

    /// Replace the message and keep the kind.
    pub fn message(mut self, message: impl Into<Text>) -> Self {
        self.message = message.into();
        self
    }

    /// The code of a custom error, or `None` for every other kind. The
    /// [`ErrorKind`] itself tells those apart.
    pub fn code(&self) -> Option<&str> {
        match &self.kind {
            ErrorKind::Custom { code } => code.as_deref(),
            _ => None,
        }
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message.as_str())
    }
}

impl From<ErrorKind> for FieldError {
    fn from(kind: ErrorKind) -> Self {
        FieldError::new(kind)
    }
}

impl From<Text> for FieldError {
    /// A custom error, keyed or not. See [`FieldError::custom`].
    fn from(message: Text) -> Self {
        FieldError::custom(message)
    }
}

impl From<&'static str> for FieldError {
    fn from(message: &'static str) -> Self {
        FieldError::custom(message)
    }
}

impl From<String> for FieldError {
    fn from(message: String) -> Self {
        FieldError::custom(message)
    }
}

impl From<Cow<'static, str>> for FieldError {
    fn from(message: Cow<'static, str>) -> Self {
        FieldError::custom(message)
    }
}

/// Every problem the crate found while parsing one submission.
///
/// Each error uses the *full* field name as its key, so a field inside a
/// flattened sub-form appears under its prefixed name. A field may carry more
/// than one error.
///
/// The key is a [`Cow`], not a `String`. A name comes from the spec, where it
/// is already `'static`, and only a flatten prefix makes a new one necessary.
/// The key is not an enum of the form's own field names. One set holds three
/// kinds of error: those from a flattened sub-form, those from a
/// `#[form(validate = ...)]` function, and those the caller adds through
/// `add_field_error`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FormErrors {
    form: Vec<FieldError>,
    fields: Vec<(Cow<'static, str>, FieldError)>,
}

impl FormErrors {
    pub const fn new() -> Self {
        Self {
            form: Vec::new(),
            fields: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.form.is_empty() && self.fields.is_empty()
    }

    /// The number of errors, over every field plus the form-level ones.
    pub fn len(&self) -> usize {
        self.form.len() + self.fields.len()
    }

    /// Record an error against a field.
    pub fn push(&mut self, field: impl Into<Cow<'static, str>>, error: FieldError) {
        self.fields.push((field.into(), error));
    }

    /// Record an error that belongs to the whole form rather than to one
    /// field, such as the result of a cross-field check.
    pub fn push_form(&mut self, error: FieldError) {
        self.form.push(error);
    }

    /// A shorthand for the common cross-field case.
    pub fn reject(&mut self, message: impl Into<Text>) {
        self.push_form(FieldError::custom(message));
    }

    /// A shorthand to reject one field with a custom message.
    pub fn reject_field(&mut self, field: impl Into<Cow<'static, str>>, message: impl Into<Text>) {
        self.push(field, FieldError::custom(message));
    }

    /// The errors on one field, in the order the crate found them.
    pub fn field(&self, name: &str) -> impl Iterator<Item = &FieldError> {
        self.fields
            .iter()
            .filter(move |(k, _)| k == name)
            .map(|(_, e)| e)
    }

    /// Whether one named field failed.
    pub fn has_field(&self, name: &str) -> bool {
        self.fields.iter().any(|(k, _)| k == name)
    }

    /// Form-level errors.
    pub fn form_errors(&self) -> &[FieldError] {
        &self.form
    }

    /// Every `(field name, error)` pair.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &FieldError)> {
        self.fields.iter().map(|(k, e)| (k.as_ref(), e))
    }

    /// Add every error in `other` to `self`.
    pub fn merge(&mut self, other: FormErrors) {
        self.form.extend(other.form);
        self.fields.extend(other.fields);
    }

    /// Add every error in `other`, and put the prefix on each of its field
    /// names. The crate uses this when it validates a sub-form on its own and
    /// then puts the sub-form into a parent.
    ///
    /// An empty prefix keeps every key exactly as it was, so the common case
    /// builds nothing.
    pub fn merge_prefixed(&mut self, prefix: &str, other: FormErrors) {
        self.form.extend(other.form);
        if prefix.is_empty() {
            self.fields.extend(other.fields);
            return;
        }
        self.fields.extend(other.fields.into_iter().map(|(k, e)| {
            let mut key = String::with_capacity(prefix.len() + k.len());
            key.push_str(prefix);
            key.push_str(&k);
            (Cow::Owned(key), e)
        }));
    }

    /// Every error as `(field name, message)`. Form-level errors use `None`.
    ///
    /// A message that is still an i18n key comes back as the key. That is also
    /// what it renders as until something resolves it.
    pub fn messages(&self) -> Vec<(Option<&str>, &str)> {
        self.form
            .iter()
            .map(|e| (None, e.message.as_str()))
            .chain(
                self.fields
                    .iter()
                    .map(|(k, e)| (Some(k.as_ref()), e.message.as_str())),
            )
            .collect()
    }
}

impl From<FieldError> for FormErrors {
    /// A single form-level error.
    fn from(error: FieldError) -> Self {
        let mut errors = FormErrors::new();
        errors.push_form(error);
        errors
    }
}

impl From<String> for FormErrors {
    /// A shorthand that lets a `#[form(validate = ...)]` function return a
    /// message.
    fn from(message: String) -> Self {
        FieldError::custom(message).into()
    }
}

impl From<&'static str> for FormErrors {
    fn from(message: &'static str) -> Self {
        FieldError::custom(message).into()
    }
}

impl From<Cow<'static, str>> for FormErrors {
    fn from(message: Cow<'static, str>) -> Self {
        FieldError::custom(message).into()
    }
}

impl From<Text> for FormErrors {
    /// A form-level message that may be an i18n key: `Err(t("signup.mismatch"))`.
    fn from(message: Text) -> Self {
        FieldError::custom(message).into()
    }
}

impl<F: Into<Cow<'static, str>>, M: Into<Text>> From<(F, M)> for FormErrors {
    /// A shorthand to reject one named field: `Err(("password", "…"))`.
    fn from((field, message): (F, M)) -> Self {
        let mut errors = FormErrors::new();
        errors.reject_field(field, message);
        errors
    }
}

impl fmt::Display for FormErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (field, message) in self.messages() {
            if !first {
                f.write_str("; ")?;
            }
            first = false;
            match field {
                Some(name) => write!(f, "{name}: {message}")?,
                None => f.write_str(message)?,
            }
        }
        if first {
            f.write_str("no errors")?;
        }
        Ok(())
    }
}

impl std::error::Error for FormErrors {}

impl Serialize for FormErrors {
    /// Serializes as `{"form": [...], "fields": {"name": [...]}}`, so a
    /// template can find a message by field name.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Repr<'a> {
            form: Vec<&'a str>,
            fields: BTreeMap<&'a str, Vec<&'a str>>,
        }

        let mut fields: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (name, error) in &self.fields {
            fields
                .entry(name.as_ref())
                .or_default()
                .push(error.message.as_str());
        }
        Repr {
            form: self.form.iter().map(|e| e.message.as_str()).collect(),
            fields,
        }
        .serialize(serializer)
    }
}

/// A value the crate could not convert to its Rust type.
///
/// It becomes an [`ErrorKind::Invalid`], where `expected` says what the parser
/// wanted, such as `"a whole number"`.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueError {
    pub expected: Cow<'static, str>,
}

impl ValueError {
    pub fn new(expected: impl Into<Cow<'static, str>>) -> Self {
        Self {
            expected: expected.into(),
        }
    }
}

impl From<ValueError> for FieldError {
    fn from(e: ValueError) -> Self {
        FieldError::new(ErrorKind::Invalid {
            expected: e.expected,
        })
    }
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "expected {}", self.expected)
    }
}

impl std::error::Error for ValueError {}
