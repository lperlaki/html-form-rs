//! The parsing machinery shared by hand-written and derived [`Form`] impls.
//!
//! [`ParseCtx`] threads four things through a parse: the raw submission, the
//! context the caller handed in, the flatten prefix currently in scope, and the
//! growing error list. Nothing in here ever short-circuits — a failed field
//! records its error and returns `None`, and the parse carries on to the next
//! field.

use std::borrow::Cow;

use crate::Form;
use crate::context::Provides;
use crate::error::{FieldError, FormErrors};
use crate::spec::{FieldSpec, Flattened};
use crate::validate::{self, FieldValidator, FormValidator};
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
///
/// Only the *literal* default is a fallback. A generated one — `default =
/// some_fn` — belongs to rendering alone: standing it in here would let a form
/// answer its own question, and a submission that left the CSRF token out would
/// arrive carrying a freshly minted one.
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
///
/// `C` is the form's [`Context`](crate::Form::Context) — whatever its own
/// functions were promised they would be handed. A form that declares none
/// parses with `C = ()`, which is what the parameter defaults to.
pub struct ParseCtx<'a, C = ()> {
    values: &'a Values,
    context: &'a C,
    prefix: String,
    errors: FormErrors,
}

impl<'a, C> ParseCtx<'a, C> {
    pub fn new(values: &'a Values, context: &'a C) -> Self {
        Self {
            values,
            context,
            prefix: String::new(),
            errors: FormErrors::new(),
        }
    }

    /// The raw submission being parsed.
    pub fn values(&self) -> &'a Values {
        self.values
    }

    /// What the caller handed in for this parse.
    pub fn context(&self) -> &'a C {
        self.context
    }

    /// The flatten prefix currently in scope.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// A field's name qualified by the current prefix.
    ///
    /// Outside a flatten there is no prefix to prepend, so the name in the spec
    /// is handed back as it stands.
    pub fn full_name(&self, name: &'static str) -> Cow<'static, str> {
        if self.prefix.is_empty() {
            return Cow::Borrowed(name);
        }
        let mut full = String::with_capacity(self.prefix.len() + name.len());
        full.push_str(&self.prefix);
        full.push_str(name);
        Cow::Owned(full)
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
    pub fn push_error(&mut self, name: &'static str, error: FieldError) {
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
        // Lent out and put back, rather than copied, so that a sub-form's
        // errors cost nothing to qualify.
        let prefix = std::mem::take(&mut self.prefix);
        self.errors.merge_prefixed(&prefix, errors);
        self.prefix = prefix;
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
        // The submission outlives the parse, so the values can be walked as
        // they are converted rather than collected first.
        let values = self.values;
        let mut out = Vec::new();
        let mut ok = true;
        for raw in values.all(&full).filter(|value| !is_blank(value)) {
            match self.convert::<T>(spec, &full, raw) {
                Some(value) => out.push(value),
                None => ok = false,
            }
        }

        if out.is_empty() && ok {
            if spec.required {
                self.errors.push(full, validate::required_error(spec));
                return None;
            }
            return Some(Vec::new());
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
    ///
    /// The sub-form is parsed with the context this one is carrying, or with
    /// whatever that context [`Provides`] in its place — which is how a
    /// context-free sub-form is flattened into a form that has one.
    ///
    /// The prefix and the errors are lent to the sub-parse and taken back, so
    /// nesting costs no allocation of its own and the errors of both halves end
    /// up in one list.
    pub fn nested<T: Form>(&mut self, flattened: &Flattened) -> Option<T>
    where
        C: Provides<T::Context>,
    {
        let restore = self.prefix.len();
        self.prefix.push_str(flattened.prefix);

        let mut nested = ParseCtx {
            values: self.values,
            context: Provides::provide(self.context),
            prefix: std::mem::take(&mut self.prefix),
            errors: std::mem::take(&mut self.errors),
        };
        let parsed = T::parse_in(&mut nested);

        self.prefix = nested.prefix;
        self.errors = nested.errors;
        self.prefix.truncate(restore);
        parsed
    }

    /// Run a `#[field(validate = ...)]` function against a parsed value.
    ///
    /// The function may take the value alone or the value and the context, and
    /// may return a `bool` or any `Result` whose error becomes a message — see
    /// [`FieldValidator`] and [`FieldValidation`](crate::FieldValidation).
    pub fn check_custom<T, M>(
        &mut self,
        spec: &FieldSpec,
        value: &T,
        validator: impl FieldValidator<T, C, M>,
    ) {
        if let Some(error) = validator.check(value, self.context) {
            let full = self.full_name(spec.name);
            self.errors.push(full, error);
        }
    }

    /// Run a `#[form(validate = ...)]` function against the assembled struct.
    ///
    /// As well as a `bool`, the function may return anything convertible to
    /// [`FormErrors`]: a message, a `(field, message)` pair, or a full error
    /// set — see [`FormValidator`] and [`FormValidation`](crate::FormValidation).
    pub fn check_form<T, M>(&mut self, value: &T, validator: impl FormValidator<T, C, M>) {
        if let Some(errors) = validator.check(value, self.context) {
            self.merge_errors(errors);
        }
    }

    /// Validate one raw value against the spec, then convert it, then let the
    /// type say what the spec could not.
    ///
    /// The first two always run: a value that is both malformed *and* out of
    /// range reports the constraint violation, which says more than "not a
    /// number". The third —
    /// [`FormValue::validate_form_value`] — needs a converted value to look at,
    /// so it runs only once there is one.
    // `full` is a `&Cow` rather than a `&str` because an error takes a copy of
    // it, and copying a name that is already `'static` should cost nothing.
    #[allow(clippy::ptr_arg)]
    fn convert<T: FormValue>(
        &mut self,
        spec: &FieldSpec,
        full: &Cow<'static, str>,
        raw: &str,
    ) -> Option<T> {
        let violations = validate::check(spec, raw);
        let had_violations = !violations.is_empty();
        for error in violations {
            self.errors.push(full.clone(), error);
        }

        match T::parse_form_value(raw) {
            Ok(value) => match value.validate_form_value() {
                Ok(()) => Some(value),
                Err(error) => {
                    self.errors.push(full.clone(), error);
                    None
                }
            },
            Err(e) => {
                if !had_violations {
                    self.errors.push(full.clone(), FieldError::from(e));
                }
                None
            }
        }
    }
}

/// Helpers the derive macro calls into. Not part of the public API.
#[doc(hidden)]
pub mod __private {
    use std::borrow::Cow;
    use std::fmt::Display;
    use std::str::FromStr;

    use crate::Form;
    use crate::context::{DefaultSource, Provides};
    use crate::error::{FieldError, ValueError};
    use crate::spec::{
        Bounds, Choice, ChoiceStyle, ChooseControl, Control, FileControl, NumberControl,
        TextControl, TextFormat, TextareaControl, join,
    };
    use crate::validate::FieldValidation;
    use crate::value::FormValue;
    use crate::values::Values;

    /// A value converted by its own [`FromStr`] and [`Display`] rather than by a
    /// [`FormValue`](crate::FormValue) impl — what `#[field(from_str)]` wraps a
    /// field's type in for the duration of the parse.
    ///
    /// It is a wrapper rather than a second parse path because everything a
    /// field goes through — the constraints in the spec, the error collecting,
    /// `Option<T>` and `Vec<T>` — should not have to be written twice for the
    /// sake of one conversion. The derive unwraps it the moment the value is
    /// parsed, so nothing downstream, a `validate` function included, ever sees
    /// it.
    pub struct Str<T>(pub T);

    impl<T: FromStr + Display> FormValue for Str<T> {
        /// A plain text input: a foreign type has no `CONTROL` to be asked for
        /// one, so the field says what it renders as, or takes this.
        const CONTROL: Control = Control::TEXT;

        /// Surrounding whitespace is dropped first, as it is for the numbers
        /// this crate parses itself — the types worth reaching for this over
        /// are dates, ids and decimals, where a pasted value with a space at
        /// the end is a user's slip rather than their answer.
        ///
        /// What the `FromStr` said went wrong is deliberately not the message:
        /// a parse error is written for whoever wrote the call, and "Enter
        /// invalid digit found in string." is not a sentence to show anybody.
        /// A field that can say better says it with a `type`, whose format
        /// check runs first and describes what it wanted.
        fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
            match raw.trim().parse() {
                Ok(value) => Ok(Str(value)),
                Err(_) => Err(ValueError::new("a valid value")),
            }
        }

        fn to_form_value(&self) -> Cow<'_, str> {
            Cow::Owned(self.0.to_string())
        }
    }

    /// Record what a `#[field(default = path)]` function produced for one
    /// render, under the field's fully-qualified name.
    ///
    /// The function may take the context or ignore it, and may return whichever
    /// string type suits it — [`DefaultSource`] is what reconciles the two, so
    /// that the derive, which cannot see a signature, emits the same call
    /// either way.
    pub fn generate_default<C, M>(
        values: &mut Values,
        prefix: &str,
        name: &'static str,
        source: impl DefaultSource<C, M>,
        context: &C,
    ) {
        values.push(join(prefix, name), source.generate(context));
    }

    /// The defaults of a flattened sub-form, under the prefix the flatten adds
    /// — the rendering half of [`ParseCtx::nested`](super::ParseCtx::nested),
    /// and where the same rules live: the sub-form is handed whatever this
    /// form's context [`Provides`] in place of its own.
    ///
    /// A sub-form that generates nothing is not walked, and its prefix is never
    /// built: the const decides that per instantiation, so a form that has no
    /// generated default anywhere in it compiles this down to nothing.
    pub fn nested_defaults<T: Form, C: Provides<T::Context>>(
        values: &mut Values,
        prefix: &str,
        nested: &'static str,
        context: &C,
    ) {
        if T::GENERATES_DEFAULTS {
            T::generate_defaults(values, &join(prefix, nested), Provides::provide(context));
        }
    }

    /// A field's default: the one written on the field, or — where the field
    /// says nothing — the one its type carries.
    ///
    /// The same bargain [`control`] strikes for the control, and made in the
    /// same place, because the macro cannot see what
    /// [`FormValue::DEFAULT`](crate::FormValue::DEFAULT) is either.
    pub const fn or_default(
        explicit: Option<&'static str>,
        implied: Option<&'static str>,
    ) -> Option<&'static str> {
        or_str(explicit, implied)
    }

    /// Run a `#[value(validate = ...)]` function over a value of the type that
    /// declared it.
    ///
    /// Unlike a field's validator this takes no context — a `FormValue` belongs
    /// to no form, so there is none to reach — but it returns everything
    /// [`FieldValidation`] accepts, so a type's check says as much as a field's
    /// can.
    pub fn check_value<T, V: FieldValidation>(
        value: &T,
        validator: impl Fn(&T) -> V,
    ) -> Result<(), FieldError> {
        match validator(value).into_field_error() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

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
                    // `type = "checkbox"` alongside options means one box per
                    // option, not the lone boolean control.
                    Some(Control::Checkbox) => ChoiceStyle::Checkbox,
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
            // `type = "checkbox"` on something already choosing between
            // variants is a checkbox *group*, not a lone boolean box.
            (Some(Control::Checkbox), Control::Choose(i)) => Control::Choose(ChooseControl {
                style: ChoiceStyle::Checkbox,
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
                    multiple: match choose.style {
                        ChoiceStyle::Select => o.multiple,
                        // A radio group is single-valued however the field is
                        // typed, and a checkbox group multi-valued.
                        ChoiceStyle::Radio => false,
                        ChoiceStyle::Checkbox => true,
                    },
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
