//! The parsing machinery shared by hand-written and derived [`WebForm`] impls.
//!
//! [`ParseCtx`] threads three things through a parse: the raw submission, the
//! flatten prefix currently in scope, and the growing error list. Nothing in
//! here ever short-circuits — a failed field records its error and returns
//! `None`, and the parse carries on to the next field.

use std::borrow::Cow;

use crate::WebForm;
use crate::error::{FieldError, FormErrors};
use crate::spec::{FieldSpec, Flattened};
use crate::validate;
use crate::value::FormValue;
use crate::values::{Values, is_blank};

/// What arrived for one field.
enum Raw<'a> {
    /// The name did not appear in the submission at all.
    Absent,
    /// The name appeared, but with an empty (or whitespace-only) value.
    Blank,
    Present(&'a str),
}

/// Read a field's single raw value, falling back to its declared default when
/// the name is absent entirely.
///
/// A name that *is* present but empty never falls back, so that clearing a
/// field keeps working even when the field has a default.
fn read<'r>(values: &'r Values, spec: &'r FieldSpec, full_name: &str) -> Raw<'r> {
    match values.get(full_name) {
        Some(value) if !is_blank(value) => Raw::Present(value),
        Some(_) => Raw::Blank,
        None => match spec.default {
            // A checkbox default describes the blank form only: an absent
            // checkbox means "unchecked", never "fall back to checked".
            Some(default) if !spec.control.is_checkable() => Raw::Present(default),
            _ => Raw::Absent,
        },
    }
}

/// State carried through one parse of one submission.
pub struct ParseCtx<'a> {
    values: &'a Values,
    prefix: String,
    errors: FormErrors,
}

impl<'a> ParseCtx<'a> {
    pub fn new(values: &'a Values) -> Self {
        Self {
            values,
            prefix: String::new(),
            errors: FormErrors::new(),
        }
    }

    /// The raw submission being parsed.
    pub fn values(&self) -> &'a Values {
        self.values
    }

    /// The flatten prefix currently in scope.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// A field's name qualified by the current prefix.
    pub fn full_name(&self, name: &str) -> String {
        format!("{}{}", self.prefix, name)
    }

    /// Errors found so far.
    pub fn errors(&self) -> &FormErrors {
        &self.errors
    }

    pub fn errors_mut(&mut self) -> &mut FormErrors {
        &mut self.errors
    }

    /// Consume the context, keeping the errors.
    pub fn into_errors(self) -> FormErrors {
        self.errors
    }

    /// Record an error against a field of the form currently being parsed.
    /// The name is qualified with the active prefix.
    pub fn push_error(&mut self, name: &str, error: FieldError) {
        let full = self.full_name(name);
        self.errors.push(full, error);
    }

    /// Record an error that belongs to the form rather than to a field.
    pub fn push_form_error(&mut self, error: FieldError) {
        self.errors.push_form(error);
    }

    /// Fold a whole [`FormErrors`] in, qualifying its field names with the
    /// active prefix. This is what a `#[form(validate = ...)]` function's
    /// errors go through.
    pub fn merge_errors(&mut self, errors: FormErrors) {
        let prefix = self.prefix.clone();
        self.errors.merge_prefixed(&prefix, errors);
    }

    /// Parse a required-by-default scalar field.
    ///
    /// A blank value for a field that is *not* required still has to produce
    /// some `T`; if the type cannot represent "nothing" (as `String` can, but
    /// `u32` cannot) the field is reported as required. Model genuinely
    /// optional fields as `Option<T>`.
    pub fn field<T: FormValue>(&mut self, spec: &FieldSpec) -> Option<T> {
        let full = self.full_name(spec.name);
        match read(self.values, spec, &full) {
            Raw::Present(raw) => self.convert::<T>(spec, &full, raw),
            Raw::Absent | Raw::Blank => {
                if spec.required {
                    self.errors.push(full, validate::required_error(spec));
                    return None;
                }
                match T::parse_form_value("") {
                    Ok(value) => Some(value),
                    Err(_) => {
                        self.errors.push(full, validate::required_error(spec));
                        None
                    }
                }
            }
        }
    }

    /// Parse an `Option<T>` field: blank and absent both mean `None`.
    pub fn optional<T: FormValue>(&mut self, spec: &FieldSpec) -> Option<Option<T>> {
        let full = self.full_name(spec.name);
        match read(self.values, spec, &full) {
            Raw::Present(raw) => self.convert::<T>(spec, &full, raw).map(Some),
            Raw::Absent | Raw::Blank => {
                if spec.required {
                    self.errors.push(full, validate::required_error(spec));
                }
                Some(None)
            }
        }
    }

    /// Parse a `Vec<T>` field from every value submitted under the name.
    ///
    /// Blank entries are dropped, which is what an unselected `<select multiple>`
    /// and the hidden empty option of a checkbox group produce.
    pub fn many<T: FormValue>(&mut self, spec: &FieldSpec) -> Option<Vec<T>> {
        let full = self.full_name(spec.name);
        let raws: Vec<&str> = self
            .values
            .all(&full)
            .filter(|value| !is_blank(value))
            .collect();

        if raws.is_empty() {
            if spec.required {
                self.errors.push(full, validate::required_error(spec));
                return None;
            }
            return Some(Vec::new());
        }

        let mut out = Vec::with_capacity(raws.len());
        let mut ok = true;
        for raw in raws {
            match self.convert::<T>(spec, &full, raw) {
                Some(value) => out.push(value),
                None => ok = false,
            }
        }
        ok.then_some(out)
    }

    /// Parse a `bool` field with checkbox semantics: absent means `false`.
    ///
    /// A *required* checkbox has to be checked — the HTML rule, useful for
    /// "I accept the terms".
    pub fn flag(&mut self, spec: &FieldSpec) -> Option<bool> {
        let full = self.full_name(spec.name);
        let checked = match self.values.get(&full) {
            None => false,
            Some(raw) => match bool::parse_form_value(raw) {
                Ok(value) => value,
                Err(e) => {
                    self.errors.push(full, FieldError::from(e));
                    return None;
                }
            },
        };
        if spec.required && !checked {
            self.errors.push(full, validate::required_error(spec));
            return None;
        }
        Some(checked)
    }

    /// Parse a flattened sub-form, with its prefix pushed for the duration.
    pub fn nested<T: WebForm>(&mut self, flattened: &Flattened) -> Option<T> {
        let restore = self.prefix.len();
        self.prefix.push_str(flattened.prefix);
        let parsed = T::parse_in(self);
        self.prefix.truncate(restore);
        parsed
    }

    /// Run a `#[field(validate = ...)]` function against a parsed value.
    pub fn check_custom<T, E>(
        &mut self,
        spec: &FieldSpec,
        value: &T,
        validator: impl Fn(&T) -> Result<(), E>,
    ) where
        E: Into<Cow<'static, str>>,
    {
        if let Err(message) = validator(value) {
            let full = self.full_name(spec.name);
            self.errors.push(full, FieldError::custom(message));
        }
    }

    /// Run a `#[form(validate = ...)]` function against the assembled struct.
    ///
    /// The function may return anything convertible to [`FormErrors`]: a
    /// message, a `(field, message)` pair, or a full error set.
    pub fn check_form<T, E>(&mut self, value: &T, validator: impl Fn(&T) -> Result<(), E>)
    where
        E: Into<FormErrors>,
    {
        if let Err(errors) = validator(value) {
            self.merge_errors(errors.into());
        }
    }

    /// Validate one raw value against the spec, then convert it.
    ///
    /// Both steps always run: a value that is both malformed *and* out of range
    /// reports the constraint violation, which says more than "not a number".
    fn convert<T: FormValue>(&mut self, spec: &FieldSpec, full: &str, raw: &str) -> Option<T> {
        let violations = validate::check(spec, raw);
        let had_violations = !violations.is_empty();
        for error in violations {
            self.errors.push(full.to_owned(), error);
        }

        match T::parse_form_value(raw) {
            Ok(value) => Some(value),
            Err(e) => {
                if !had_violations {
                    self.errors.push(full.to_owned(), FieldError::from(e));
                }
                None
            }
        }
    }
}

/// Helpers the derive macro calls into. Not part of the public API.
#[doc(hidden)]
pub mod __private {
    use crate::spec::{
        Bounds, Choice, ChoiceStyle, ChooseControl, Control, FileControl, NumberControl,
        TextControl, TextFormat, TextareaControl,
    };

    /// The `#[field(...)]` attributes that belong to a control rather than to
    /// the field as a whole.
    ///
    /// The derive collects them without knowing which control it is decorating
    /// — that can come from the field's Rust type, which a macro cannot inspect
    /// — so [`control`] is what finally decides where each one lands, and
    /// rejects the ones that have nowhere to go.
    pub struct Overrides {
        pub pattern: Option<&'static str>,
        pub minlength: Option<usize>,
        pub maxlength: Option<usize>,
        pub min: Option<&'static str>,
        pub max: Option<&'static str>,
        pub step: Option<&'static str>,
        pub accept: Option<&'static str>,
        pub rows: Option<u32>,
        pub cols: Option<u32>,
        pub multiple: bool,
    }

    impl Overrides {
        pub const NONE: Self = Self {
            pattern: None,
            minlength: None,
            maxlength: None,
            min: None,
            max: None,
            step: None,
            accept: None,
            rows: None,
            cols: None,
            multiple: false,
        };
    }

    /// Assemble a field's control out of what its type implies, what
    /// `type = "..."` named, the options it declared, and its attributes.
    ///
    /// Evaluated in `const` position, so every `assert!` here is a compile
    /// error at the point the form is declared.
    pub const fn control(
        implied: Control,
        explicit: Option<Control>,
        choices: Option<&'static [Choice]>,
        o: Overrides,
    ) -> Control {
        apply(base(implied, explicit, choices), o)
    }

    /// Which control this is, before any attribute is placed in it.
    const fn base(
        implied: Control,
        explicit: Option<Control>,
        choices: Option<&'static [Choice]>,
    ) -> Control {
        // Declaring options is enough to mean "this is a chooser", whatever the
        // Rust type would otherwise have said.
        if let Some(choices) = choices {
            return Control::Choose(ChooseControl {
                style: match explicit {
                    Some(Control::Choose(c)) => c.style,
                    _ => ChoiceStyle::Select,
                },
                multiple: false,
                choices,
            });
        }
        match (explicit, implied) {
            // `type = "radio"` on a `FormChoice` enum restyles the control but
            // must not throw away the variants it is choosing between.
            (Some(Control::Choose(e)), Control::Choose(i)) => Control::Choose(ChooseControl {
                style: e.style,
                multiple: i.multiple,
                choices: i.choices,
            }),
            // Likewise `type = "range"` on a `u32` keeps the bounds the integer
            // type implies.
            (Some(Control::Number(e)), Control::Number(i)) => Control::Number(NumberControl {
                format: e.format,
                bounds: i.bounds,
            }),
            (Some(explicit), _) => explicit,
            (None, implied) => implied,
        }
    }

    const fn apply(base: Control, o: Overrides) -> Control {
        match base {
            Control::Text(text) => {
                deny_bounds(&o);
                deny_accept(&o);
                deny_size(&o);
                Control::Text(TextControl {
                    format: match text.format {
                        TextFormat::Email { multiple } => TextFormat::Email {
                            multiple: multiple || o.multiple,
                        },
                        other => other,
                    },
                    pattern: or_str(o.pattern, text.pattern),
                    minlength: or_usize(o.minlength, text.minlength),
                    maxlength: or_usize(o.maxlength, text.maxlength),
                })
            }
            Control::Textarea(area) => {
                deny_pattern(&o);
                deny_bounds(&o);
                deny_accept(&o);
                Control::Textarea(TextareaControl {
                    minlength: or_usize(o.minlength, area.minlength),
                    maxlength: or_usize(o.maxlength, area.maxlength),
                    rows: or_u32(o.rows, area.rows),
                    cols: or_u32(o.cols, area.cols),
                })
            }
            Control::Number(number) => {
                deny_pattern(&o);
                deny_length(&o);
                deny_accept(&o);
                deny_size(&o);
                Control::Number(NumberControl {
                    format: number.format,
                    bounds: merge_bounds(&o, number.bounds),
                })
            }
            Control::Temporal(temporal) => {
                deny_pattern(&o);
                deny_length(&o);
                deny_accept(&o);
                deny_size(&o);
                Control::Temporal(crate::spec::TemporalControl {
                    format: temporal.format,
                    bounds: merge_bounds(&o, temporal.bounds),
                })
            }
            Control::Choose(choose) => {
                deny_pattern(&o);
                deny_length(&o);
                deny_bounds(&o);
                deny_accept(&o);
                deny_size(&o);
                Control::Choose(ChooseControl {
                    // A radio group is single-valued however the field is typed.
                    multiple: o.multiple && matches!(choose.style, ChoiceStyle::Select),
                    ..choose
                })
            }
            Control::File(file) => {
                deny_pattern(&o);
                deny_length(&o);
                deny_bounds(&o);
                deny_size(&o);
                Control::File(FileControl {
                    accept: or_str(o.accept, file.accept),
                    multiple: o.multiple || file.multiple,
                })
            }
            control @ (Control::Checkbox | Control::Color | Control::Hidden) => {
                deny_pattern(&o);
                deny_length(&o);
                deny_bounds(&o);
                deny_accept(&o);
                deny_size(&o);
                control
            }
        }
    }

    const fn merge_bounds(o: &Overrides, implied: Bounds) -> Bounds {
        Bounds {
            min: or_str(o.min, implied.min),
            max: or_str(o.max, implied.max),
            step: or_str(o.step, implied.step),
        }
    }

    // `assert!` messages in a `const fn` have to be literals, so each attribute
    // group gets its own guard rather than one formatted complaint.

    const fn deny_pattern(o: &Overrides) {
        assert!(
            o.pattern.is_none(),
            "`pattern` applies only to a text input (text, password, email, url, tel, search)"
        );
    }

    const fn deny_length(o: &Overrides) {
        assert!(
            o.minlength.is_none() && o.maxlength.is_none(),
            "`minlength`/`maxlength` apply only to a text input or a textarea"
        );
    }

    const fn deny_bounds(o: &Overrides) {
        assert!(
            o.min.is_none() && o.max.is_none() && o.step.is_none(),
            "`min`/`max`/`step` apply only to a number, range or date/time control"
        );
    }

    const fn deny_accept(o: &Overrides) {
        assert!(o.accept.is_none(), "`accept` applies only to a file input");
    }

    const fn deny_size(o: &Overrides) {
        assert!(
            o.rows.is_none() && o.cols.is_none(),
            "`rows`/`cols` apply only to a textarea"
        );
    }

    const fn or_str(
        over: Option<&'static str>,
        implied: Option<&'static str>,
    ) -> Option<&'static str> {
        match over {
            Some(value) => Some(value),
            None => implied,
        }
    }

    const fn or_usize(over: Option<usize>, implied: Option<usize>) -> Option<usize> {
        match over {
            Some(value) => Some(value),
            None => implied,
        }
    }

    const fn or_u32(over: Option<u32>, implied: Option<u32>) -> Option<u32> {
        match over {
            Some(value) => Some(value),
            None => implied,
        }
    }
}
