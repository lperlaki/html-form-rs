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

#[cfg(test)]
mod tests {
    use super::*;

    fn message(kind: ErrorKind) -> String {
        kind.default_message().into_owned()
    }

    #[test]
    fn every_kind_has_a_message_that_names_what_it_wanted() {
        assert_eq!(message(ErrorKind::Required), "This field is required.");
        assert_eq!(
            message(ErrorKind::Invalid {
                expected: "a whole number".into()
            }),
            "Enter a whole number."
        );
        // The pattern itself is a regular expression, which is no use to a
        // user, so the message leaves it out and the kind carries it.
        assert_eq!(
            message(ErrorKind::Pattern {
                pattern: "[a-z]+".into()
            }),
            "This value is not in the expected format."
        );
        assert_eq!(
            message(ErrorKind::Step { step: "5".into() }),
            "Must be a multiple of 5."
        );
        assert_eq!(
            message(ErrorKind::NotAChoice),
            "Select one of the available options."
        );
        assert_eq!(
            message(ErrorKind::Custom { code: None }),
            "This value is not valid."
        );
    }

    #[test]
    fn a_length_message_counts_and_agrees_with_itself() {
        assert_eq!(
            message(ErrorKind::TooShort {
                minlength: 1,
                length: 0
            }),
            "Enter at least 1 character (currently 0).",
            "one character, singular"
        );
        assert_eq!(
            message(ErrorKind::TooShort {
                minlength: 8,
                length: 3
            }),
            "Enter at least 8 characters (currently 3)."
        );
        assert_eq!(
            message(ErrorKind::TooLong {
                maxlength: 1,
                length: 4
            }),
            "Enter at most 1 character (currently 4)."
        );
        assert_eq!(
            message(ErrorKind::TooLong {
                maxlength: 10,
                length: 42
            }),
            "Enter at most 10 characters (currently 42)."
        );
        // Zero is plural, as English has it.
        assert_eq!(
            message(ErrorKind::TooLong {
                maxlength: 0,
                length: 1
            }),
            "Enter at most 0 characters (currently 1)."
        );
    }

    /// A bound on a date reads as a point in time, and a bound on a number as
    /// a quantity. The crate keeps both as strings, so the message decides
    /// which it is by looking at the bound.
    #[test]
    fn a_bound_reads_as_a_quantity_or_as_a_point_in_time() {
        assert_eq!(
            message(ErrorKind::TooSmall { min: "18".into() }),
            "Must be 18 or more."
        );
        assert_eq!(
            message(ErrorKind::TooLarge { max: "1.5".into() }),
            "Must be 1.5 or less."
        );
        assert_eq!(
            message(ErrorKind::TooSmall {
                min: "2026-01-01".into()
            }),
            "Must be 2026-01-01 or later."
        );
        assert_eq!(
            message(ErrorKind::TooLarge {
                max: "12:30".into()
            }),
            "Must be 12:30 or earlier."
        );
    }

    #[test]
    fn an_error_starts_from_the_built_in_message_of_its_kind() {
        let error = FieldError::new(ErrorKind::Required);
        assert_eq!(error.message.as_str(), "This field is required.");
        assert_eq!(FieldError::from(ErrorKind::Required), error);
        assert_eq!(error.to_string(), "This field is required.");
    }

    #[test]
    fn a_caller_may_replace_the_message_and_keep_the_kind() {
        let error = FieldError::new(ErrorKind::Required).message("We need this one.");
        assert_eq!(error.kind, ErrorKind::Required);
        assert_eq!(error.message.as_str(), "We need this one.");

        let supplied = FieldError::with_message(ErrorKind::NotAChoice, "Pick one.");
        assert_eq!(supplied.kind, ErrorKind::NotAChoice);
        assert_eq!(supplied.message.as_str(), "Pick one.");
    }

    /// A message written as an i18n key is also the error's code, so one
    /// string both translates and matches.
    #[test]
    fn a_keyed_custom_message_doubles_as_the_code() {
        let keyed = FieldError::custom(Text::key("signup.username.reserved"));
        assert_eq!(keyed.code(), Some("signup.username.reserved"));
        assert_eq!(
            keyed.kind,
            ErrorKind::Custom {
                code: Some("signup.username.reserved".into())
            }
        );

        // Literal text is not a code.
        assert_eq!(FieldError::custom("That name is taken.").code(), None);
    }

    #[test]
    fn a_code_can_be_named_apart_from_the_message() {
        let error = FieldError::coded("username.taken", "That name is taken.");
        assert_eq!(error.code(), Some("username.taken"));
        assert_eq!(error.message.as_str(), "That name is taken.");
    }

    /// Only a custom error has a code. Every other kind says which constraint
    /// it broke, which is what a caller matches on instead.
    #[test]
    fn no_other_kind_carries_a_code() {
        assert_eq!(FieldError::new(ErrorKind::Required).code(), None);
        assert_eq!(FieldError::new(ErrorKind::NotAChoice).code(), None);
    }

    /// Every string shape a `validate = ...` function may return becomes the
    /// same custom error.
    #[test]
    fn a_message_of_any_string_type_becomes_a_custom_error() {
        let expected = FieldError::custom("no");
        assert_eq!(FieldError::from("no"), expected);
        assert_eq!(FieldError::from("no".to_owned()), expected);
        assert_eq!(FieldError::from(Cow::Borrowed("no")), expected);
        assert_eq!(FieldError::from(Text::literal("no")), expected);
    }

    #[test]
    fn a_value_error_says_what_the_conversion_wanted() {
        let value = ValueError::new("a whole number");
        assert_eq!(value.to_string(), "expected a whole number");
        assert_eq!(
            FieldError::from(value).kind,
            ErrorKind::Invalid {
                expected: "a whole number".into()
            }
        );
    }

    // ─── The set ──────────────────────────────────────────────────────────────

    fn errors() -> FormErrors {
        let mut errors = FormErrors::new();
        errors.push("email", FieldError::new(ErrorKind::Required));
        errors.push("email", FieldError::custom("And it is not an address."));
        errors.push("age", FieldError::new(ErrorKind::NotAChoice));
        errors.push_form(FieldError::custom("The two passwords differ."));
        errors
    }

    #[test]
    fn an_empty_set_is_what_a_form_that_passed_produces() {
        let empty = FormErrors::new();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.to_string(), "no errors");
        assert_eq!(empty, FormErrors::default());
        assert!(empty.form_errors().is_empty());
        assert!(!empty.has_field("email"));
        assert_eq!(empty.field("email").count(), 0);
    }

    /// The count is over every field plus the form-level ones, so a field with
    /// two errors counts twice.
    #[test]
    fn a_field_may_carry_more_than_one_error() {
        let errors = errors();
        assert!(!errors.is_empty());
        assert_eq!(errors.len(), 4);
        assert_eq!(errors.field("email").count(), 2);
        assert!(errors.has_field("age"));
        assert!(!errors.has_field("password"));
        assert_eq!(errors.form_errors().len(), 1);
        // In the order the crate found them.
        assert_eq!(
            errors.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            ["email", "email", "age"]
        );
    }

    #[test]
    fn the_shorthands_reject_the_form_or_one_field() {
        let mut errors = FormErrors::new();
        errors.reject("The two passwords differ.");
        errors.reject_field("confirm", Text::key("signup.mismatch"));

        assert_eq!(
            errors.form_errors()[0].message.as_str(),
            "The two passwords differ."
        );
        assert_eq!(
            errors.field("confirm").next().unwrap().code(),
            Some("signup.mismatch"),
            "a keyed message is a code here too"
        );
    }

    #[test]
    fn messages_name_the_field_they_belong_to_and_the_form_names_none() {
        assert_eq!(
            errors().messages(),
            [
                (None, "The two passwords differ."),
                (Some("email"), "This field is required."),
                (Some("email"), "And it is not an address."),
                (Some("age"), "Select one of the available options."),
            ]
        );
    }

    #[test]
    fn display_writes_every_message_in_one_line() {
        assert_eq!(
            errors().to_string(),
            "The two passwords differ.; email: This field is required.; \
             email: And it is not an address.; age: Select one of the available options."
        );
    }

    #[test]
    fn merging_keeps_both_sets_whole() {
        let mut errors = errors();
        let mut other = FormErrors::new();
        other.push("name", FieldError::new(ErrorKind::Required));
        other.push_form(FieldError::custom("And something else."));

        errors.merge(other);
        assert_eq!(errors.len(), 6);
        assert!(errors.has_field("name"));
        assert_eq!(errors.form_errors().len(), 2);
    }

    /// A sub-form validated on its own knows its fields by their bare names.
    /// The prefix is what puts them where the parent submitted them.
    #[test]
    fn merging_under_a_prefix_qualifies_the_field_names() {
        let mut parent = FormErrors::new();
        let mut child = FormErrors::new();
        child.push("street", FieldError::new(ErrorKind::Required));
        child.push_form(FieldError::custom("The address is incomplete."));

        parent.merge_prefixed("billing_", child);
        assert!(parent.has_field("billing_street"));
        assert!(!parent.has_field("street"));
        // A form-level error belongs to no field, so no prefix applies to it.
        assert_eq!(parent.form_errors().len(), 1);
    }

    /// The common case is a form that flattens nothing, where every name is
    /// already the one it was submitted under.
    #[test]
    fn an_empty_prefix_leaves_every_name_exactly_as_it_was() {
        let mut parent = FormErrors::new();
        let mut child = FormErrors::new();
        child.push("street", FieldError::new(ErrorKind::Required));

        parent.merge_prefixed("", child);
        assert!(parent.has_field("street"));
    }

    /// A single message is the shortest thing a `#[form(validate = ...)]`
    /// function can return, and it belongs to the form rather than a field.
    #[test]
    fn a_lone_message_becomes_a_form_level_error() {
        for errors in [
            FormErrors::from("no"),
            FormErrors::from("no".to_owned()),
            FormErrors::from(Cow::Borrowed("no")),
            FormErrors::from(Text::literal("no")),
            FormErrors::from(FieldError::custom("no")),
        ] {
            assert_eq!(errors.form_errors().len(), 1);
            assert_eq!(errors.to_string(), "no");
        }
    }

    #[test]
    fn a_pair_names_the_field_the_user_can_correct() {
        let errors = FormErrors::from(("confirm", "The two passwords do not match."));
        assert!(errors.has_field("confirm"));
        assert!(errors.form_errors().is_empty());

        // Either half may be owned, for a name or a message built at runtime.
        let built = FormErrors::from((format!("item_{}", 3), format!("Row {} is wrong.", 3)));
        assert!(built.has_field("item_3"));
    }

    /// A template finds a message by field name, so the serialized shape
    /// groups by name rather than repeating the list of pairs.
    #[test]
    fn the_set_serialises_grouped_by_field_name() {
        assert_eq!(
            serde_json::to_value(errors()).unwrap(),
            serde_json::json!({
                "form": ["The two passwords differ."],
                "fields": {
                    "age": ["Select one of the available options."],
                    "email": ["This field is required.", "And it is not an address."],
                },
            })
        );
    }

    /// The kind travels with the message, so a JSON client can match on the
    /// constraint rather than on English. It carries the constraint it broke
    /// alongside its own name.
    #[test]
    fn an_error_serialises_with_the_constraint_it_broke() {
        assert_eq!(
            serde_json::to_value(FieldError::new(ErrorKind::TooShort {
                minlength: 8,
                length: 3
            }))
            .unwrap(),
            serde_json::json!({
                "kind": {"kind": "too_short", "minlength": 8, "length": 3},
                "message": "Enter at least 8 characters (currently 3).",
            })
        );

        // A custom error with no code leaves the code out rather than sending
        // a null.
        assert_eq!(
            serde_json::to_value(FieldError::custom("no")).unwrap(),
            serde_json::json!({"kind": {"kind": "custom"}, "message": "no"})
        );
        assert_eq!(
            serde_json::to_value(FieldError::coded("taken", "no")).unwrap(),
            serde_json::json!({
                "kind": {"kind": "custom", "code": "taken"},
                "message": "no",
            })
        );
    }
}
