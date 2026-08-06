//! Validation and conversion errors.
//!
//! Parsing never stops at the first problem: every field is attempted, every
//! failure is recorded in a [`FormErrors`], and the caller gets the complete
//! picture in one pass.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use serde::{Serialize, Serializer};

/// Why a value was rejected.
///
/// The variants carry the constraint that was violated so callers can render
/// their own (localised) messages instead of the built-in English ones.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorKind {
    /// Required, but missing or blank.
    Required,
    /// Present, but not convertible to the field's Rust type.
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
    /// Produced by a `#[field(validate = ...)]` or `#[form(validate = ...)]`
    /// function.
    Custom,
}

impl ErrorKind {
    /// The built-in English message for this kind.
    pub fn default_message(&self) -> String {
        match self {
            ErrorKind::Required => "This field is required.".into(),
            ErrorKind::Invalid { expected } => format!("Enter {expected}."),
            ErrorKind::Pattern { .. } => "This value is not in the expected format.".into(),
            ErrorKind::TooShort { minlength, length } => format!(
                "Enter at least {minlength} character{} (currently {length}).",
                plural(*minlength)
            ),
            ErrorKind::TooLong { maxlength, length } => format!(
                "Enter at most {maxlength} character{} (currently {length}).",
                plural(*maxlength)
            ),
            // A bound on a date reads as a point in time, one on a number as a
            // quantity.
            ErrorKind::TooSmall { min } if is_number(min) => format!("Must be {min} or more."),
            ErrorKind::TooSmall { min } => format!("Must be {min} or later."),
            ErrorKind::TooLarge { max } if is_number(max) => format!("Must be {max} or less."),
            ErrorKind::TooLarge { max } => format!("Must be {max} or earlier."),
            ErrorKind::Step { step } => format!("Must be a multiple of {step}."),
            ErrorKind::NotAChoice => "Select one of the available options.".into(),
            ErrorKind::Custom => "This value is not valid.".into(),
        }
    }
}

fn is_number(value: &str) -> bool {
    value.parse::<f64>().is_ok()
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// A single rejection, with a ready-to-display message.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldError {
    pub kind: ErrorKind,
    pub message: Cow<'static, str>,
}

impl FieldError {
    /// An error carrying the built-in message for its kind.
    pub fn new(kind: ErrorKind) -> Self {
        let message = kind.default_message().into();
        Self { kind, message }
    }

    /// An error with a caller-supplied message.
    pub fn with_message(kind: ErrorKind, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// The error a `validate = ...` function produces.
    pub fn custom(message: impl Into<Cow<'static, str>>) -> Self {
        Self::with_message(ErrorKind::Custom, message)
    }

    /// Replace the message, keeping the kind.
    pub fn message(mut self, message: impl Into<Cow<'static, str>>) -> Self {
        self.message = message.into();
        self
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Every problem found while parsing one submission.
///
/// Errors are keyed by the *full* field name, so a field inside a flattened
/// sub-form appears under its prefixed name. A field may carry more than one
/// error.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FormErrors {
    form: Vec<FieldError>,
    fields: Vec<(String, FieldError)>,
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

    /// Total number of errors, across all fields plus form-level ones.
    pub fn len(&self) -> usize {
        self.form.len() + self.fields.len()
    }

    /// Record an error against a field.
    pub fn push(&mut self, field: impl Into<String>, error: FieldError) {
        self.fields.push((field.into(), error));
    }

    /// Record an error that belongs to the form as a whole rather than to one
    /// field — cross-field checks, for example.
    pub fn push_form(&mut self, error: FieldError) {
        self.form.push(error);
    }

    /// Convenience for the common cross-field case.
    pub fn reject(&mut self, message: impl Into<Cow<'static, str>>) {
        self.push_form(FieldError::custom(message));
    }

    /// Convenience for rejecting one field with a custom message.
    pub fn reject_field(
        &mut self,
        field: impl Into<String>,
        message: impl Into<Cow<'static, str>>,
    ) {
        self.push(field, FieldError::custom(message));
    }

    /// Errors attached to one field, in the order they were found.
    pub fn field(&self, name: &str) -> impl Iterator<Item = &FieldError> {
        self.fields
            .iter()
            .filter(move |(k, _)| k == name)
            .map(|(_, e)| e)
    }

    /// Whether a specific field failed.
    pub fn has_field(&self, name: &str) -> bool {
        self.fields.iter().any(|(k, _)| k == name)
    }

    /// Form-level errors.
    pub fn form_errors(&self) -> &[FieldError] {
        &self.form
    }

    /// Every `(field name, error)` pair.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &FieldError)> {
        self.fields.iter().map(|(k, e)| (k.as_str(), e))
    }

    /// Fold `other` into `self`.
    pub fn merge(&mut self, other: FormErrors) {
        self.form.extend(other.form);
        self.fields.extend(other.fields);
    }

    /// Fold `other` in, prefixing each of its field names — used when a
    /// sub-form is validated on its own and then spliced into a parent.
    pub fn merge_prefixed(&mut self, prefix: &str, other: FormErrors) {
        self.form.extend(other.form);
        self.fields.extend(
            other
                .fields
                .into_iter()
                .map(|(k, e)| (format!("{prefix}{k}"), e)),
        );
    }

    /// All errors as `(field name, message)`, form-level ones under `None`.
    pub fn messages(&self) -> Vec<(Option<&str>, &str)> {
        self.form
            .iter()
            .map(|e| (None, e.message.as_ref()))
            .chain(
                self.fields
                    .iter()
                    .map(|(k, e)| (Some(k.as_str()), e.message.as_ref())),
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
    /// Shorthand so a `#[form(validate = ...)]` function can return a message.
    fn from(message: String) -> Self {
        FieldError::custom(message).into()
    }
}

impl From<&'static str> for FormErrors {
    fn from(message: &'static str) -> Self {
        FieldError::custom(message).into()
    }
}

impl<F: Into<String>, M: Into<Cow<'static, str>>> From<(F, M)> for FormErrors {
    /// Shorthand for rejecting one named field: `Err(("password", "…"))`.
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
    /// Serialised as `{"form": [...], "fields": {"name": [...]}}` so templates
    /// can look messages up by field name.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Repr<'a> {
            form: Vec<&'a str>,
            fields: BTreeMap<&'a str, Vec<&'a str>>,
        }

        let mut fields: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (name, error) in &self.fields {
            fields
                .entry(name.as_str())
                .or_default()
                .push(error.message.as_ref());
        }
        Repr {
            form: self.form.iter().map(|e| e.message.as_ref()).collect(),
            fields,
        }
        .serialize(serializer)
    }
}

/// A value that could not be converted to its Rust type.
///
/// Turned into [`ErrorKind::Invalid`] with `expected` describing what the
/// parser wanted, e.g. `"a whole number"`.
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
