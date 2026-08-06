//! The parsing machinery that hand-written and derived [`Form`] impls share.
//!
//! [`ParseCtx`] carries four things through a parse. They are the raw
//! submission, the context the caller passed in, the flatten prefix in scope,
//! and the growing error list. Nothing here stops early. A failed field records
//! its error and returns `None`, and the parse goes on to the next field.

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
    /// The name did not appear in the submission.
    Absent,
    /// The name appeared, but with an empty or whitespace-only value.
    Blank,
    Present(&'a str),
}

/// Read a field's single raw value. Use its declared default when the name is
/// absent.
///
/// A name that *is* present but empty never uses the default, so a user can
/// still clear a field that has one.
///
/// Only the *literal* default is a fallback. A generated one, `default =
/// some_fn`, belongs to rendering alone. Using it here would let a form answer
/// its own question. A submission that left the CSRF token out would then
/// arrive with a fresh one.
fn read<'r>(values: &'r Values, spec: &'r FieldSpec, full_name: &str) -> Raw<'r> {
    match values.get(full_name) {
        Some(value) if !is_blank(value) => Raw::Present(value),
        Some(_) => Raw::Blank,
        None => match spec.default {
            // A checkbox default describes the blank form only. An absent
            // checkbox means "unchecked", never "use the checked default".
            Some(default) if !spec.control.is_checkable() => Raw::Present(default),
            _ => Raw::Absent,
        },
    }
}

/// The state that one parse of one submission carries.
///
/// `C` is the form's [`Context`](crate::Form::Context), whatever the form's own
/// functions expect to receive. A form that declares none parses with `C = ()`,
/// which is the default for the parameter.
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

    /// The raw submission this parse reads.
    pub fn values(&self) -> &'a Values {
        self.values
    }

    /// What the caller passed in for this parse.
    pub fn context(&self) -> &'a C {
        self.context
    }

    /// The flatten prefix in scope.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// A field's name, qualified by the current prefix.
    ///
    /// Outside a flatten there is no prefix to add, so this gives back the name
    /// in the spec as it stands.
    pub fn full_name(&self, name: &'static str) -> Cow<'static, str> {
        if self.prefix.is_empty() {
            return Cow::Borrowed(name);
        }
        let mut full = String::with_capacity(self.prefix.len() + name.len());
        full.push_str(&self.prefix);
        full.push_str(name);
        Cow::Owned(full)
    }

    /// The errors this parse has found so far.
    pub fn errors(&self) -> &FormErrors {
        &self.errors
    }

    pub fn errors_mut(&mut self) -> &mut FormErrors {
        &mut self.errors
    }

    /// Consume the context and keep the errors.
    pub fn into_errors(self) -> FormErrors {
        self.errors
    }

    /// Record an error against a field of the form this parse is reading. The
    /// name gets the active prefix.
    pub fn push_error(&mut self, name: &'static str, error: FieldError) {
        let full = self.full_name(name);
        self.errors.push(full, error);
    }

    /// Record an error that belongs to the form rather than to a field.
    pub fn push_form_error(&mut self, error: FieldError) {
        self.errors.push_form(error);
    }

    /// Add a whole [`FormErrors`], and put the active prefix on each of its
    /// field names. The errors of a `#[form(validate = ...)]` function come in
    /// this way.
    pub fn merge_errors(&mut self, errors: FormErrors) {
        // Lent out and put back, not copied, so a sub-form's errors cost
        // nothing to qualify.
        let prefix = std::mem::take(&mut self.prefix);
        self.errors.merge_prefixed(&prefix, errors);
        self.prefix = prefix;
    }

    /// Parse a scalar field, which is required by default.
    ///
    /// A blank value for a field that is *not* required still has to produce
    /// some `T`. If the type cannot hold "nothing", the crate reports the field
    /// as required. A `String` can hold nothing, and a `u32` cannot. Write a
    /// truly optional field as an `Option<T>`.
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

    /// Parse an `Option<T>` field. A blank value and an absent one both mean
    /// `None`.
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
    /// This drops blank entries. An unselected `<select multiple>` and the
    /// hidden empty option of a checkbox group both produce one.
    pub fn many<T: FormValue>(&mut self, spec: &FieldSpec) -> Option<Vec<T>> {
        let full = self.full_name(spec.name);
        // The submission outlives the parse, so this walks the values as it
        // converts them, in place of collecting them first.
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

    /// Parse a `bool` field the way a checkbox works: absent means `false`.
    ///
    /// A user must check a *required* checkbox. That is the HTML rule, and it
    /// is useful for "I accept the terms".
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

    /// Parse a flattened sub-form, with its prefix in scope for the duration.
    ///
    /// The sub-form parses with the context this one carries, or with whatever
    /// that context [`Provides`] in its place. That is how a context-free
    /// sub-form goes into a form that has a context.
    ///
    /// This lends the prefix and the errors to the sub-parse and takes them
    /// back. Nesting therefore allocates nothing of its own, and the errors of
    /// both halves reach one list.
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
    /// The function may take the value alone, or the value and the context. It
    /// may return a `bool`, or any `Result` whose error becomes a message. See
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

    /// Run a `#[form(validate = ...)]` function against the whole struct.
    ///
    /// Besides a `bool`, the function may return anything that converts into
    /// [`FormErrors`]: a message, a `(field, message)` pair, or a full error
    /// set. See [`FormValidator`] and
    /// [`FormValidation`](crate::FormValidation).
    pub fn check_form<T, M>(&mut self, value: &T, validator: impl FormValidator<T, C, M>) {
        if let Some(errors) = validator.check(value, self.context) {
            self.merge_errors(errors);
        }
    }

    /// Validate one raw value against the spec, then convert it, then let the
    /// type say what the spec could not.
    ///
    /// The first two always run. A value that is both malformed *and* out of
    /// range reports the broken constraint, which says more than "not a
    /// number". The third step, [`FormValue::validate_form_value`], needs a
    /// converted value to look at, so it runs only once one exists.
    // `full` is a `&Cow`, not a `&str`, because an error takes a copy of it,
    // and copying a name that is already `'static` should cost nothing.
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

/// Helpers the derive macro calls. Not part of the public API.
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

    /// A value converted by its own [`FromStr`] and [`Display`], not by a
    /// [`FormValue`](crate::FormValue) impl. `#[field(from_str)]` wraps a
    /// field's type in this for the length of the parse.
    ///
    /// It is a wrapper, not a second parse path, because one conversion should
    /// not make the crate write everything a field goes through twice. That is
    /// the constraints in the spec, the error collecting, `Option<T>` and
    /// `Vec<T>`. The derive unwraps it as soon as the value parses, so nothing
    /// later ever sees it, a `validate` function included.
    pub struct Str<T>(pub T);

    impl<T: FromStr + Display> FormValue for Str<T> {
        /// A plain text input. A foreign type has no `CONTROL` to give one, so
        /// the field says what it renders as, or it takes this.
        const CONTROL: Control = Control::TEXT;

        /// This drops the whitespace around the value first, as it does for
        /// the numbers this crate parses itself. The types worth using this
        /// for are dates, ids and decimals. There, a pasted value with a space
        /// at the end is a user's slip, not their answer.
        ///
        /// The message deliberately leaves out what the `FromStr` said. A parse
        /// error speaks to whoever wrote the call, and "Enter invalid digit
        /// found in string." is not a sentence to show anybody. A field that
        /// can say more says it with a `type`, whose format check runs first
        /// and describes what it wanted.
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
    /// render, under the field's fully qualified name.
    ///
    /// The function may take the context or ignore it, and it may return
    /// whichever string type suits it. [`DefaultSource`] joins the two, so the
    /// derive emits the same call either way. The derive cannot see a
    /// signature.
    pub fn generate_default<C, M>(
        values: &mut Values,
        prefix: &str,
        name: &'static str,
        source: impl DefaultSource<C, M>,
        context: &C,
    ) {
        values.push(join(prefix, name), source.generate(context));
    }

    /// The defaults of a flattened sub-form, under the prefix the flatten adds.
    /// This is the rendering half of
    /// [`ParseCtx::nested`](super::ParseCtx::nested), and it follows the same
    /// rules. The sub-form receives whatever this form's context [`Provides`]
    /// in place of its own.
    ///
    /// A sub-form that generates nothing gets no walk, and the crate never
    /// builds its prefix. The const decides that per instantiation, so a form
    /// with no generated default anywhere compiles this down to nothing.
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

    /// A field's default: the one written on the field, or, where the field
    /// says nothing, the one its type carries.
    ///
    /// [`control`] makes the same trade for the control, in the same place. The
    /// macro cannot see what
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
    /// This takes no context, unlike a field's validator. A `FormValue` belongs
    /// to no form, so there is no context to reach. It still returns everything
    /// [`FieldValidation`] accepts, so a type's check says as much as a field's
    /// check can.
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
    /// The derive collects them without knowing which control they decorate.
    /// The control can come from the field's Rust type, which a macro cannot
    /// inspect. [`control`] therefore decides where each one goes, and rejects
    /// the ones with nowhere to go.
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

    /// Build a field's control out of what its type implies, what
    /// `type = "..."` named, the options it declared, and its attributes.
    ///
    /// This runs in `const` position, so every `assert!` here is a compile
    /// error where the form is declared.
    pub const fn control(
        implied: Control,
        explicit: Option<Control>,
        choices: Option<&'static [Choice]>,
        o: Overrides,
    ) -> Control {
        apply(base(implied, explicit, choices), o)
    }

    /// Which control this is, before any attribute goes into it.
    const fn base(
        implied: Control,
        explicit: Option<Control>,
        choices: Option<&'static [Choice]>,
    ) -> Control {
        // Declared options alone mean "this is a chooser", whatever the Rust
        // type would otherwise say.
        if let Some(choices) = choices {
            return Control::Choose(ChooseControl {
                style: match explicit {
                    Some(Control::Choose(c)) => c.style,
                    // `type = "checkbox"` next to options means one box per
                    // option, not the single boolean control.
                    Some(Control::Checkbox) => ChoiceStyle::Checkbox,
                    _ => ChoiceStyle::Select,
                },
                multiple: false,
                choices,
            });
        }
        match (explicit, implied) {
            // `type = "radio"` on a `FormChoice` enum changes the style of the
            // control. It must keep the variants the control chooses between.
            (Some(Control::Choose(e)), Control::Choose(i)) => Control::Choose(ChooseControl {
                style: e.style,
                multiple: i.multiple,
                choices: i.choices,
            }),
            // `type = "checkbox"` on something that already chooses between
            // variants is a checkbox *group*, not a single boolean box.
            (Some(Control::Checkbox), Control::Choose(i)) => Control::Choose(ChooseControl {
                style: ChoiceStyle::Checkbox,
                multiple: i.multiple,
                choices: i.choices,
            }),
            // In the same way, `type = "range"` on a `u32` keeps the bounds the
            // integer type implies.
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
                        // A radio group carries one value whatever the field
                        // type is, and a checkbox group carries many.
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

    // An `assert!` message in a `const fn` has to be a literal, so each
    // attribute group gets its own guard in place of one formatted message.

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
