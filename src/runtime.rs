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

/// Read a field's single raw value.
///
/// A default never stands in here. It describes what a *render* starts with,
/// and a parse reads what arrived. A field left out of the submission was left
/// out: if the crate filled it in, a form would answer its own question, and a
/// submission carrying no CSRF token at all would arrive holding a valid one.
/// See [`FieldSpec::default`](crate::FieldSpec::default).
fn read<'r>(values: &'r Values, full_name: &str) -> Raw<'r> {
    match values.get(full_name) {
        Some(value) if !is_blank(value) => Raw::Present(value),
        Some(_) => Raw::Blank,
        None => Raw::Absent,
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
        match read(self.values, &full) {
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
        match read(self.values, &full) {
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
    use std::any::Any;
    use std::borrow::Cow;
    use std::fmt::Display;
    use std::str::FromStr;

    use crate::context::{DefaultSource, Provides};
    use crate::error::{FieldError, ValueError};
    use crate::spec::{
        Bounds, Choice, ChoiceStyle, ChooseControl, Control, FieldDefault, FileControl,
        NumberControl, TextControl, TextFormat, TextareaControl,
    };
    use crate::validate::FieldValidation;
    use crate::value::FormValue;

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

        fn to_form_value(&self) -> Cow<'static, str> {
            Cow::Owned(self.0.to_string())
        }
    }

    // A default is written for the field's own type, and the adapter is what
    // the field goes through. These two are what let a default reach the glue
    // as a `Str<T>`, so a `from_str` field needs no second set of glue to
    // produce one.
    impl<T> From<T> for Str<T> {
        fn from(value: T) -> Self {
            Str(value)
        }
    }

    impl<T: Default> Default for Str<T> {
        fn default() -> Self {
            Str(T::default())
        }
    }

    // ─── The glue behind a default ────────────────────────────────────────────
    //
    // A `FieldDefault` is one function pointer, and the derive writes the
    // function it points at. These are the bodies of those functions: the crate
    // holds the downcasts, and the macro emits nothing but a call. Each one
    // takes the render's context erased as a `&dyn Any`, names the type the
    // form declared back, and gives back the string the field renders. See
    // [`FieldDefault`](crate::FieldDefault).

    /// The glue for a default written into the spec.
    ///
    /// This reads no context, so a literal fits a form of any kind. It is a
    /// function all the same, because a spec holds one slot for a default and a
    /// function pointer cannot close over the text.
    pub fn default_literal(literal: &'static str) -> Cow<'static, str> {
        Cow::Borrowed(literal)
    }

    /// The glue for `#[field(default = path)]`.
    ///
    /// The function may take the context or ignore it, and it may give back the
    /// field's own type or anything that converts into it. [`DefaultSource`]
    /// joins those, so the derive emits the same call whichever one you wrote.
    /// The derive cannot see a signature.
    ///
    /// The value is written out the way the field writes out every other value
    /// it holds, so a default and a filled-in record reach the render as the
    /// same string.
    ///
    /// # Panics
    ///
    /// If `context` is not a `C`. The derive pairs this call with the form
    /// whose context is `C`, and the renderer passes that form's context, so
    /// only a hand-written spec put on the wrong form can reach it.
    pub fn default_from<C: Any, V: FormValue, M, S: DefaultSource<C, V, M>>(
        context: &dyn Any,
        source: S,
    ) -> Cow<'static, str> {
        let context = read_context::<C>(context);
        // The value was made for this call and nothing else holds it, so a type
        // that can hand its string over does.
        source.generate(context).into_form_value()
    }

    /// The glue for a default that a *type* declares, which is a literal the
    /// macro cannot read: [`FormValue::DEFAULT`] is an associated const.
    fn default_of_type<V: FormValue>(_context: &dyn Any) -> Cow<'static, str> {
        default_literal(V::DEFAULT.expect("`or_default` only reaches for a default the type has"))
    }

    /// The glue for `#[field(default)]`, which is the field type's own
    /// `Default::default()`.
    ///
    /// It reads no context. It is a function for the reason a literal is one:
    /// a `const` cannot call `Default::default`, and the spec holds one slot.
    pub fn default_standard<V: Default + FormValue>() -> Cow<'static, str> {
        V::default().into_form_value()
    }

    /// A field's fully qualified name: the flatten prefix it sits under, then
    /// its own name.
    ///
    /// The derive's `fill_in` writes the same names the walk resolves, so both
    /// join them here. It gives a `Cow` because a field under no prefix is
    /// already the `&'static str` in the spec.
    pub fn join(prefix: &str, name: &'static str) -> Cow<'static, str> {
        crate::spec::join(prefix, name)
    }

    /// A *type's* default: the one `#[value(...)]` wrote, or the one the type
    /// it wraps carries. Both are literals, because
    /// [`FormValue::DEFAULT`](crate::FormValue::DEFAULT) is a `const` and a
    /// `const` cannot call a function. A default produced per render belongs to
    /// a field, which is what [`or_default`] merges.
    pub const fn or_literal(
        written: Option<&'static str>,
        inherited: Option<&'static str>,
    ) -> Option<&'static str> {
        or_str(written, inherited)
    }

    /// A field's default: the one written on the field, or, where the field
    /// says nothing, the one its type carries.
    ///
    /// [`control`] makes the same trade for the control, in the same place. The
    /// macro cannot see what
    /// [`FormValue::DEFAULT`](crate::FormValue::DEFAULT) is either.
    pub const fn or_default<V: FormValue>(written: Option<FieldDefault>) -> Option<FieldDefault> {
        match written {
            Some(default) => Some(default),
            None => match V::DEFAULT {
                // This glue reads no context, so it holds for a form of any
                // kind.
                Some(literal) => Some(FieldDefault::new(default_of_type::<V>, Some(literal))),
                None => None,
            },
        }
    }

    /// Whether a field shows its default on every render.
    ///
    /// `#[field(reset)]` settles it outright, and `reset = false` settles it
    /// the other way. The macro emits what you wrote in that case and never
    /// calls this. Where the field says nothing, one kind of field resets on
    /// its own: a hidden control whose default the form *produces*. Nobody
    /// typed such a field, it can hold nothing a caller would miss, and a
    /// rejected token sent back would make the retry fail exactly as the first
    /// attempt did.
    ///
    /// This is a `const fn` because the macro cannot see either half of that
    /// rule. A control can come from the field's Rust type, and so can a
    /// default. [`FieldSpec::reset`](crate::FieldSpec::reset) is what the
    /// renderer reads, and it is the whole answer by the time a spec exists.
    pub const fn or_reset(control: &Control, default: &Option<FieldDefault>) -> bool {
        match default {
            Some(default) => {
                default.is_generated() && matches!(control.kind(), crate::FieldKind::Hidden)
            }
            None => false,
        }
    }

    /// The bridge a `#[field(flatten)]` puts in its [`Flattened`][crate::Flattened], which turns
    /// the enclosing form's context into the one the sub-form's own defaults
    /// read. It is the rendering half of
    /// [`ParseCtx::nested`](super::ParseCtx::nested), and it follows the same
    /// rule: the sub-form receives whatever this form's context [`Provides`] in
    /// place of its own.
    ///
    /// # Panics
    ///
    /// If `context` is not a `C`, for the reason [`default_from`] does.
    pub fn provide<C: Provides<I> + Any, I: Any>(context: &dyn Any) -> &dyn Any {
        Provides::provide(read_context::<C>(context))
    }

    /// Name an erased context back as the concrete type the glue was written
    /// for. The two functions above each did this separately; this is the one
    /// place the downcast happens, and the one place that names what went wrong
    /// when it fails.
    fn read_context<C: Any>(context: &dyn Any) -> &C {
        context.downcast_ref::<C>().unwrap_or_else(|| {
            panic!(
                "a form's own glue was handed a context that is not its `{}` — the spec of one \
                 form was put on another",
                std::any::type_name::<C>(),
            )
        })
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

    /// The merge runs in `const` position, where nothing observes it. These
    /// call it at run time instead, so each rule can be stated on its own.
    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::spec::{Generate, NumberFormat, TemporalControl};

        const CHOICES: &[Choice] = &[Choice::new("de", "Germany"), Choice::new("ch", "Swiss")];

        fn merged(implied: Control, explicit: Option<Control>, o: Overrides) -> Control {
            control(implied, explicit, None, o)
        }

        fn plain(implied: Control) -> Control {
            merged(implied, None, Overrides::NONE)
        }

        fn choose(style: ChoiceStyle, multiple: bool) -> Control {
            Control::Choose(ChooseControl {
                style,
                multiple,
                choices: CHOICES,
            })
        }

        fn text(control: TextControl) -> Control {
            Control::Text(control)
        }

        // ─── Which control this is ────────────────────────────────────────────

        /// A field that names no control takes the one its Rust type implies.
        #[test]
        fn the_rust_type_settles_the_control_where_the_field_says_nothing() {
            assert_eq!(plain(Control::TEXT), Control::TEXT);
            assert_eq!(plain(Control::NUMBER), Control::NUMBER);
            assert_eq!(plain(Control::Checkbox), Control::Checkbox);
        }

        /// And where it names one, that one wins outright — the two have
        /// nothing in common to carry over.
        #[test]
        fn a_named_control_replaces_the_implied_one() {
            assert_eq!(
                merged(Control::TEXT, Some(Control::Hidden), Overrides::NONE),
                Control::Hidden
            );
            assert_eq!(
                merged(Control::NUMBER, Some(Control::Color), Overrides::NONE),
                Control::Color
            );
        }

        /// Declared options alone mean "this is a chooser", whatever the Rust
        /// type would otherwise say.
        #[test]
        fn declared_options_make_a_chooser_out_of_any_type() {
            let built = control(Control::TEXT, None, Some(CHOICES), Overrides::NONE);
            assert_eq!(built, choose(ChoiceStyle::Select, false));
        }

        #[test]
        fn options_take_the_style_the_field_named() {
            let radio = control(
                Control::TEXT,
                Some(Control::Choose(ChooseControl {
                    style: ChoiceStyle::Radio,
                    ..ChooseControl::DEFAULT
                })),
                Some(CHOICES),
                Overrides::NONE,
            );
            assert_eq!(radio, choose(ChoiceStyle::Radio, false));
        }

        /// `type = "checkbox"` next to options means one box per option, not
        /// the single boolean control.
        #[test]
        fn a_checkbox_next_to_options_is_a_group_of_boxes() {
            let group = control(
                Control::TEXT,
                Some(Control::Checkbox),
                Some(CHOICES),
                Overrides::NONE,
            );
            // A checkbox group carries many values whatever the field type is.
            assert_eq!(group, choose(ChoiceStyle::Checkbox, true));
        }

        /// Restyling a chooser must keep the variants it chooses between, which
        /// the enum carries and the attribute cannot name.
        #[test]
        fn restyling_a_chooser_keeps_the_options_the_type_carries() {
            let implied = choose(ChoiceStyle::Select, true);
            let restyled = merged(
                implied,
                Some(Control::Choose(ChooseControl {
                    style: ChoiceStyle::Radio,
                    ..ChooseControl::DEFAULT
                })),
                Overrides::NONE,
            );
            let Control::Choose(choose) = restyled else {
                panic!("still a chooser");
            };
            assert_eq!(choose.style, ChoiceStyle::Radio);
            assert_eq!(choose.choices.len(), 2);
            // A radio group carries one value however many the type holds.
            assert!(!choose.multiple);
        }

        /// `type = "checkbox"` on something that already chooses between
        /// variants is a checkbox *group*, not a single boolean box.
        #[test]
        fn a_checkbox_on_a_chooser_is_a_group_and_not_a_boolean_box() {
            let group = merged(
                choose(ChoiceStyle::Select, false),
                Some(Control::Checkbox),
                Overrides::NONE,
            );
            assert_eq!(group, choose(ChoiceStyle::Checkbox, true));
        }

        /// In the same way, `type = "range"` on a `u32` keeps the bounds the
        /// integer type implies.
        #[test]
        fn restyling_a_number_keeps_the_bounds_the_integer_type_implies() {
            let implied = Control::Number(NumberControl {
                format: NumberFormat::Number,
                bounds: Bounds {
                    min: Some("0"),
                    max: None,
                    step: Some("1"),
                },
            });
            let range = merged(
                implied,
                Some(Control::Number(NumberControl {
                    format: NumberFormat::Range,
                    bounds: Bounds::DEFAULT,
                })),
                Overrides::NONE,
            );
            assert_eq!(
                range,
                Control::Number(NumberControl {
                    format: NumberFormat::Range,
                    bounds: Bounds {
                        min: Some("0"),
                        max: None,
                        step: Some("1"),
                    },
                })
            );
        }

        // ─── What each attribute narrows ──────────────────────────────────────

        /// An attribute written on the field wins over what the type implied,
        /// and where the field says nothing the type's own value stands.
        #[test]
        fn an_attribute_on_the_field_narrows_what_the_type_implied() {
            let implied = text(TextControl {
                pattern: Some("[a-z]+"),
                minlength: Some(2),
                maxlength: Some(254),
                ..TextControl::DEFAULT
            });
            let narrowed = merged(
                implied,
                None,
                Overrides {
                    maxlength: Some(40),
                    ..Overrides::NONE
                },
            );
            assert_eq!(
                narrowed,
                text(TextControl {
                    pattern: Some("[a-z]+"),
                    minlength: Some(2),
                    maxlength: Some(40),
                    ..TextControl::DEFAULT
                })
            );
        }

        #[test]
        fn a_text_input_takes_a_pattern_and_a_length() {
            let built = merged(
                Control::TEXT,
                None,
                Overrides {
                    pattern: Some("[0-9]{4}"),
                    minlength: Some(4),
                    maxlength: Some(4),
                    ..Overrides::NONE
                },
            );
            assert_eq!(built.pattern(), Some("[0-9]{4}"));
            assert_eq!(built.minlength(), Some(4));
            assert_eq!(built.maxlength(), Some(4));
        }

        /// `multiple` on an email input means a comma-separated list, and it is
        /// the one thing `multiple` means on any text control.
        #[test]
        fn multiple_on_an_email_input_accepts_a_list() {
            let built = merged(
                text(TextControl {
                    format: TextFormat::Email { multiple: false },
                    ..TextControl::DEFAULT
                }),
                None,
                Overrides {
                    multiple: true,
                    ..Overrides::NONE
                },
            );
            assert!(built.multiple());

            // On any other text format it changes nothing.
            let plain = merged(
                Control::TEXT,
                None,
                Overrides {
                    multiple: true,
                    ..Overrides::NONE
                },
            );
            assert!(!plain.multiple());
        }

        #[test]
        fn a_textarea_takes_a_length_and_a_size() {
            let built = merged(
                Control::Textarea(TextareaControl::DEFAULT),
                None,
                Overrides {
                    minlength: Some(10),
                    maxlength: Some(500),
                    rows: Some(8),
                    cols: Some(60),
                    ..Overrides::NONE
                },
            );
            assert_eq!(
                built,
                Control::Textarea(TextareaControl {
                    minlength: Some(10),
                    maxlength: Some(500),
                    rows: Some(8),
                    cols: Some(60),
                })
            );
        }

        #[test]
        fn a_number_takes_the_three_bounds() {
            let built = merged(
                Control::NUMBER,
                None,
                Overrides {
                    min: Some("1"),
                    max: Some("10"),
                    step: Some("0.5"),
                    ..Overrides::NONE
                },
            );
            assert_eq!(
                built.bounds(),
                Some(&Bounds {
                    min: Some("1"),
                    max: Some("10"),
                    step: Some("0.5"),
                })
            );
        }

        #[test]
        fn a_date_takes_the_same_bounds_written_as_dates() {
            let built = merged(
                Control::Temporal(TemporalControl::DEFAULT),
                None,
                Overrides {
                    min: Some("2026-01-01"),
                    max: Some("2026-12-31"),
                    ..Overrides::NONE
                },
            );
            assert_eq!(
                built.bounds(),
                Some(&Bounds {
                    min: Some("2026-01-01"),
                    max: Some("2026-12-31"),
                    step: None,
                })
            );
        }

        #[test]
        fn a_file_input_takes_what_it_accepts_and_how_many() {
            let built = merged(
                Control::File(FileControl::DEFAULT),
                None,
                Overrides {
                    accept: Some("image/*"),
                    multiple: true,
                    ..Overrides::NONE
                },
            );
            assert_eq!(built.accept(), Some("image/*"));
            assert!(built.multiple());
        }

        /// A `<select>` takes as many values as the field type holds. The two
        /// groups decide for themselves: a radio group carries one value, and
        /// a checkbox group carries many.
        #[test]
        fn only_a_select_reads_multiple_off_the_field() {
            fn many() -> Overrides {
                Overrides {
                    multiple: true,
                    ..Overrides::NONE
                }
            }
            assert!(merged(choose(ChoiceStyle::Select, false), None, many()).multiple());
            assert!(!merged(choose(ChoiceStyle::Radio, true), None, many()).multiple());
            assert!(merged(choose(ChoiceStyle::Checkbox, false), None, many()).multiple());
            assert!(!merged(choose(ChoiceStyle::Select, false), None, Overrides::NONE).multiple());
        }

        #[test]
        fn the_controls_with_nothing_to_narrow_pass_straight_through() {
            for control in [Control::Checkbox, Control::Color, Control::Hidden] {
                assert_eq!(plain(control), control, "{control:?}");
            }
        }

        // ─── What each control refuses ────────────────────────────────────────

        /// An attribute with nowhere to go is a mistake in the declaration, and
        /// the merge runs in `const` position, so each of these is a compile
        /// error where the form is written.
        #[test]
        fn an_attribute_the_control_cannot_hold_is_refused() {
            // Each case builds its own `Overrides`, which is not `Copy`, so
            // the list holds the calls rather than the arguments.
            type Refusal = (&'static str, fn() -> Control);
            let cases: &[Refusal] = &[
                ("`pattern` applies only to a text input", || {
                    merged(
                        Control::NUMBER,
                        None,
                        Overrides {
                            pattern: Some("[0-9]+"),
                            ..Overrides::NONE
                        },
                    )
                }),
                (
                    "`minlength`/`maxlength` apply only to a text input or a textarea",
                    || {
                        merged(
                            Control::NUMBER,
                            None,
                            Overrides {
                                maxlength: Some(4),
                                ..Overrides::NONE
                            },
                        )
                    },
                ),
                ("`min`/`max`/`step` apply only to a number", || {
                    merged(
                        Control::TEXT,
                        None,
                        Overrides {
                            min: Some("1"),
                            ..Overrides::NONE
                        },
                    )
                }),
                ("`accept` applies only to a file input", || {
                    merged(
                        Control::SELECT,
                        None,
                        Overrides {
                            accept: Some("image/*"),
                            ..Overrides::NONE
                        },
                    )
                }),
                ("`rows`/`cols` apply only to a textarea", || {
                    merged(
                        Control::TEXT,
                        None,
                        Overrides {
                            rows: Some(4),
                            ..Overrides::NONE
                        },
                    )
                }),
            ];

            // The refusals are the point, so the report of each one is noise.
            let hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(|_| {}));
            let refusals: Vec<_> = cases
                .iter()
                .map(|(expected, build)| (expected, std::panic::catch_unwind(build)))
                .collect();
            std::panic::set_hook(hook);

            for (expected, refused) in refusals {
                let panic = refused.expect_err(expected);
                let message = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or_default();
                assert!(message.contains(expected), "got {message:?}");
            }
        }

        /// Every control refuses what it cannot hold, including the three that
        /// hold nothing but themselves.
        #[test]
        #[should_panic(expected = "`pattern` applies only to a text input")]
        fn a_checkbox_holds_none_of_them_either() {
            merged(
                Control::Checkbox,
                None,
                Overrides {
                    pattern: Some("[0-9]+"),
                    ..Overrides::NONE
                },
            );
        }

        #[test]
        #[should_panic(expected = "`min`/`max`/`step`")]
        fn a_file_input_takes_no_bounds() {
            merged(
                Control::File(FileControl::DEFAULT),
                None,
                Overrides {
                    step: Some("1"),
                    ..Overrides::NONE
                },
            );
        }

        #[test]
        #[should_panic(expected = "`rows`/`cols`")]
        fn a_temporal_control_takes_no_size() {
            merged(
                Control::Temporal(TemporalControl::DEFAULT),
                None,
                Overrides {
                    cols: Some(4),
                    ..Overrides::NONE
                },
            );
        }

        #[test]
        #[should_panic(expected = "`minlength`/`maxlength`")]
        fn a_chooser_takes_no_length() {
            merged(
                Control::SELECT,
                None,
                Overrides {
                    minlength: Some(4),
                    ..Overrides::NONE
                },
            );
        }

        #[test]
        #[should_panic(expected = "`accept` applies only to a file input")]
        fn a_textarea_accepts_no_media_type() {
            merged(
                Control::Textarea(TextareaControl::DEFAULT),
                None,
                Overrides {
                    accept: Some("image/*"),
                    ..Overrides::NONE
                },
            );
        }

        // ─── The two helpers the derive calls alongside it ────────────────────

        /// A type whose values a blank form starts with, which is what only an
        /// associated const can say and only the compiler can read.
        struct Preset(String);

        impl FormValue for Preset {
            const CONTROL: Control = Control::TEXT;
            const DEFAULT: Option<&'static str> = Some("from the type");

            fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
                Ok(Preset(raw.to_owned()))
            }

            fn to_form_value(&self) -> Cow<'static, str> {
                Cow::Owned(self.0.clone())
            }
        }

        /// The glue a derived form writes for `default = "from the field"`.
        const FROM_THE_FIELD: Generate = |_context| default_literal("from the field");
        const WRITTEN: FieldDefault = FieldDefault::new(FROM_THE_FIELD, Some("from the field"));

        /// Run a default the way a render does, with a context it ignores.
        fn produced(default: FieldDefault) -> Cow<'static, str> {
            default.value(&())
        }

        /// The macro cannot see what a type's `DEFAULT` is, so the same trade
        /// the control makes is made here for the value a blank form starts
        /// with.
        #[test]
        fn a_field_default_falls_back_to_the_one_its_type_carries() {
            let written = or_default::<Preset>(Some(WRITTEN)).expect("the field wrote one");
            assert_eq!(written.literal(), Some("from the field"));
            assert_eq!(produced(written), "from the field");

            let implied = or_default::<Preset>(None).expect("the type carries one");
            assert_eq!(implied.literal(), Some("from the type"));
            assert_eq!(produced(implied), "from the type");

            // A field that says nothing, of a type that carries nothing.
            assert_eq!(or_default::<String>(None), None);
            assert_eq!(
                or_default::<String>(Some(WRITTEN)).map(produced),
                Some(Cow::Borrowed("from the field"))
            );
        }

        /// The other half: a default the form runs. Either arity works, and the
        /// value comes back written the way the field writes every other one.
        #[test]
        fn a_generated_default_reaches_the_render_through_the_glue_the_macro_writes() {
            struct Session {
                seats: u32,
            }
            fn booked(session: &Session) -> u32 {
                session.seats
            }
            fn free() -> u8 {
                7
            }

            // What the derive emits for `#[field(default = booked)]` on a `u32`
            // field of a form whose context is `Session`.
            const BOOKED: Generate = |context| default_from::<Session, u32, _, _>(context, booked);
            const TAKEN: FieldDefault = FieldDefault::new(BOOKED, None);
            // And for a function that ignores the context and hands back
            // something the field's type is made from.
            const FREE: Generate = |context| default_from::<Session, u32, _, _>(context, free);
            const SPARE: FieldDefault = FieldDefault::new(FREE, None);

            assert!(TAKEN.is_generated());
            assert_eq!(TAKEN.literal(), None);
            assert_eq!(format!("{TAKEN:?}"), "<generated>");

            let session = Session { seats: 12 };
            assert_eq!(TAKEN.value(&session), "12");
            assert_eq!(SPARE.value(&session), "7");
        }

        /// The downcast is what keeps the erasure honest. Handing a default the
        /// context of some *other* form is a panic that names the type the glue
        /// wanted, and never a value read out of something that is not one.
        #[test]
        #[should_panic(expected = "is not its ")]
        fn glue_handed_the_wrong_context_says_so_rather_than_misreading_it() {
            struct Session;
            struct Elsewhere;

            fn nothing(_session: &Session) -> String {
                String::new()
            }

            const GLUE: Generate =
                |context| default_from::<Session, String, _, _>(context, nothing);
            FieldDefault::new(GLUE, None).value(&Elsewhere);
        }

        /// A `#[field(from_str)]` field writes its values out with `Display`,
        /// and its default takes the same road: the `Str` adapter, which is
        /// the road every other value of such a field takes.
        #[test]
        fn a_default_of_a_foreign_type_is_written_out_the_way_the_field_is() {
            fn loopback() -> std::net::Ipv4Addr {
                std::net::Ipv4Addr::LOCALHOST
            }
            const LOOPBACK: Generate =
                |context| default_from::<(), Str<std::net::Ipv4Addr>, _, _>(context, loopback);
            const HOST: FieldDefault = FieldDefault::new(LOOPBACK, None);
            assert_eq!(produced(HOST), "127.0.0.1");
        }

        /// A `#[value(validate = ...)]` function takes no context, but returns
        /// everything a field's validator may return.
        #[test]
        fn a_types_own_check_returns_what_a_fields_check_does() {
            assert_eq!(check_value(&4u32, |n| n % 2 == 0), Ok(()));
            assert!(check_value(&3u32, |n| n % 2 == 0).is_err());

            let keyed = check_value(&3u32, |_| Err::<(), _>(crate::Text::key("seats.odd")));
            assert_eq!(keyed.unwrap_err().code(), Some("seats.odd"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Control, FieldDefault, Generate, TextControl};

    fn spec(name: &'static str) -> FieldSpec {
        FieldSpec {
            name,
            ..FieldSpec::DEFAULT
        }
    }

    fn required(name: &'static str) -> FieldSpec {
        FieldSpec {
            required: true,
            ..spec(name)
        }
    }

    // ─── Reading one raw value ────────────────────────────────────────────────

    /// A parse reads what arrived and nothing else. A name that came back empty
    /// is a field the user cleared, and a name that did not come back at all is
    /// a field the submission does not carry.
    #[test]
    fn a_value_is_present_blank_or_absent_and_nothing_else() {
        assert!(matches!(
            read(&Values::parse("name=grace"), "name"),
            Raw::Present("grace")
        ));
        assert!(matches!(read(&Values::parse("name="), "name"), Raw::Blank));
        assert!(matches!(read(&Values::new(), "name"), Raw::Absent));
    }

    /// Whitespace alone is not an answer, so it reads as blank rather than as
    /// a value that happens to be spaces.
    #[test]
    fn a_whitespace_only_value_reads_as_blank() {
        assert!(matches!(
            read(&Values::parse("name=+++"), "name"),
            Raw::Blank
        ));
    }

    // ─── The prefix in scope ──────────────────────────────────────────────────

    /// Outside a flatten there is no prefix to add, so the name in the spec is
    /// what the parse looks for, borrowed as it stands.
    #[test]
    fn a_name_outside_a_flatten_is_borrowed_rather_than_built() {
        let values = Values::new();
        let ctx: ParseCtx<'_, ()> = ParseCtx::new(&values, &());

        assert_eq!(ctx.prefix(), "");
        assert_eq!(ctx.full_name("street"), "street");
        assert!(matches!(ctx.full_name("street"), Cow::Borrowed(_)));
    }

    /// A hand-written impl reaches the submission and the caller's context
    /// through the same accessors the derive uses.
    #[test]
    fn a_parse_carries_the_submission_and_the_context_it_was_given() {
        let values = Values::parse("name=ada");
        let context = "the context";
        let ctx = ParseCtx::new(&values, &context);

        assert_eq!(ctx.values().get("name"), Some("ada"));
        assert_eq!(*ctx.context(), "the context");
        assert!(ctx.errors().is_empty());
    }

    // ─── Collecting errors ────────────────────────────────────────────────────

    /// Nothing stops early. A failed field records its error and the parse goes
    /// on to the next one.
    #[test]
    fn an_error_is_recorded_against_the_field_that_caused_it() {
        let values = Values::new();
        let mut ctx: ParseCtx<'_, ()> = ParseCtx::new(&values, &());

        ctx.push_error("email", FieldError::new(crate::ErrorKind::Required));
        ctx.push_form_error(FieldError::custom("And the form as a whole is wrong."));
        assert_eq!(ctx.errors().len(), 2);

        // The same list, reachable for a caller that builds errors its own way.
        ctx.errors_mut().reject_field("age", "Too young.");

        let errors = ctx.into_errors();
        assert!(errors.has_field("email"));
        assert!(errors.has_field("age"));
        assert_eq!(errors.form_errors().len(), 1);
    }

    /// A `#[form(validate = ...)]` function knows its fields by their bare
    /// names. Merging is what puts them under the prefix in scope.
    #[test]
    fn merged_errors_take_the_prefix_in_scope() {
        let values = Values::new();
        let mut ctx: ParseCtx<'_, ()> = ParseCtx::new(&values, &());
        ctx.prefix.push_str("billing_");

        let mut incoming = FormErrors::new();
        incoming.push("street", FieldError::new(crate::ErrorKind::Required));
        ctx.merge_errors(incoming);

        // The prefix is lent out and put back, so the parse can go on using it.
        assert_eq!(ctx.prefix(), "billing_");
        assert!(ctx.into_errors().has_field("billing_street"));
    }

    // ─── The four field shapes ────────────────────────────────────────────────

    fn parse<T>(body: &str, take: impl FnOnce(&mut ParseCtx<'_, ()>) -> T) -> (T, FormErrors) {
        let values = Values::parse(body);
        let mut ctx: ParseCtx<'_, ()> = ParseCtx::new(&values, &());
        let parsed = take(&mut ctx);
        (parsed, ctx.into_errors())
    }

    #[test]
    fn a_scalar_field_converts_what_arrived() {
        let (parsed, errors) = parse("age=36", |ctx| ctx.field::<u32>(&spec("age")));
        assert_eq!(parsed, Some(36));
        assert!(errors.is_empty());
    }

    /// A blank value for a field that is *not* required still has to produce
    /// some `T`. A `String` can hold nothing, and a `u32` cannot.
    #[test]
    fn a_blank_scalar_is_empty_where_the_type_can_hold_nothing() {
        let (parsed, errors) = parse("name=", |ctx| ctx.field::<String>(&spec("name")));
        assert_eq!(parsed, Some(String::new()));
        assert!(errors.is_empty());

        // A `u32` cannot, so the crate reports the field as required.
        let (parsed, errors) = parse("age=", |ctx| ctx.field::<u32>(&spec("age")));
        assert_eq!(parsed, None);
        assert_eq!(
            errors.field("age").next().unwrap().kind,
            crate::ErrorKind::Required
        );
    }

    /// A default describes what a render starts with. A parse that reached for
    /// one would let a form answer its own question.
    #[test]
    fn a_default_never_stands_in_for_a_value_that_did_not_arrive() {
        // The glue a derived form writes for `default = "ada"`.
        const ADA_GLUE: Generate = |_context| __private::default_literal("ada");
        const ADA: FieldDefault = FieldDefault::new(ADA_GLUE, Some("ada"));
        let with_default = FieldSpec {
            default: Some(ADA),
            ..spec("name")
        };

        let (parsed, errors) = parse("", |ctx| ctx.field::<String>(&with_default));
        assert_eq!(parsed, Some(String::new()), "absent, so nothing arrived");
        assert!(errors.is_empty());

        // And a required one is missing, rather than filled in for the user.
        let required = FieldSpec {
            required: true,
            ..with_default
        };
        let (_, errors) = parse("", |ctx| ctx.field::<String>(&required));
        assert_eq!(
            errors.field("name").next().unwrap().kind,
            crate::ErrorKind::Required
        );
    }

    #[test]
    fn a_required_scalar_that_did_not_arrive_says_so() {
        let (parsed, errors) = parse("", |ctx| ctx.field::<String>(&required("name")));
        assert_eq!(parsed, None);
        assert_eq!(
            errors.field("name").next().unwrap().kind,
            crate::ErrorKind::Required
        );
    }

    #[test]
    fn a_blank_or_absent_option_is_none_either_way() {
        for body in ["", "age="] {
            let (parsed, errors) = parse(body, |ctx| ctx.optional::<u32>(&spec("age")));
            assert_eq!(parsed, Some(None), "{body:?}");
            assert!(errors.is_empty());
        }

        let (parsed, _) = parse("age=36", |ctx| ctx.optional::<u32>(&spec("age")));
        assert_eq!(parsed, Some(Some(36)));
    }

    /// An `Option` that is also required is a contradiction the caller wrote,
    /// so the error stands and the field still parses as `None`.
    #[test]
    fn a_required_option_still_reports_the_missing_value() {
        let (parsed, errors) = parse("", |ctx| ctx.optional::<u32>(&required("age")));
        assert_eq!(parsed, Some(None));
        assert!(errors.has_field("age"));
    }

    #[test]
    fn a_many_field_collects_every_value_submitted_under_the_name() {
        let (parsed, errors) = parse("tag=x&other=1&tag=y", |ctx| {
            ctx.many::<String>(&spec("tag"))
        });
        assert_eq!(parsed, Some(vec!["x".to_owned(), "y".to_owned()]));
        assert!(errors.is_empty());
    }

    /// An unselected `<select multiple>` and the hidden empty option of a
    /// checkbox group both submit a blank, which is not a value.
    #[test]
    fn a_many_field_drops_the_blanks_a_group_submits() {
        let (parsed, _) = parse("tag=&tag=x&tag=+", |ctx| ctx.many::<String>(&spec("tag")));
        assert_eq!(parsed, Some(vec!["x".to_owned()]));

        let (parsed, errors) = parse("tag=", |ctx| ctx.many::<String>(&spec("tag")));
        assert_eq!(parsed, Some(Vec::new()));
        assert!(errors.is_empty());
    }

    #[test]
    fn a_required_many_field_needs_at_least_one_value() {
        let (parsed, errors) = parse("", |ctx| ctx.many::<String>(&required("tag")));
        assert_eq!(parsed, None);
        assert_eq!(
            errors.field("tag").next().unwrap().kind,
            crate::ErrorKind::Required
        );
    }

    /// One bad entry fails the field, and the good ones are still converted so
    /// that every error in the list is reported at once.
    #[test]
    fn a_many_field_reports_every_entry_it_could_not_convert() {
        let (parsed, errors) = parse("n=1&n=x&n=y", |ctx| ctx.many::<u32>(&spec("n")));
        assert_eq!(parsed, None);
        assert_eq!(errors.field("n").count(), 2);
        // And an empty result is not mistaken for "nothing was submitted".
        assert!(
            !errors
                .field("n")
                .any(|e| e.kind == crate::ErrorKind::Required)
        );
    }

    #[test]
    fn an_absent_box_is_false_rather_than_missing() {
        let (parsed, errors) = parse("", |ctx| ctx.flag(&spec("agree")));
        assert_eq!(parsed, Some(false));
        assert!(errors.is_empty());

        let (parsed, _) = parse("agree=on", |ctx| ctx.flag(&spec("agree")));
        assert_eq!(parsed, Some(true));
    }

    /// A user must check a *required* box. That is the HTML rule, and it is
    /// useful for "I accept the terms".
    #[test]
    fn a_required_box_has_to_be_checked() {
        let (parsed, errors) = parse("", |ctx| ctx.flag(&required("agree")));
        assert_eq!(parsed, None);
        assert_eq!(
            errors.field("agree").next().unwrap().kind,
            crate::ErrorKind::Required
        );

        let (parsed, _) = parse("agree=off", |ctx| ctx.flag(&required("agree")));
        assert_eq!(parsed, None, "an explicit `off` is not checked either");

        let (parsed, errors) = parse("agree=true", |ctx| ctx.flag(&required("agree")));
        assert_eq!(parsed, Some(true));
        assert!(errors.is_empty());
    }

    #[test]
    fn a_box_with_a_value_nothing_can_read_is_reported() {
        let (parsed, errors) = parse("agree=maybe", |ctx| ctx.flag(&spec("agree")));
        assert_eq!(parsed, None);
        assert!(matches!(
            errors.field("agree").next().unwrap().kind,
            crate::ErrorKind::Invalid { .. }
        ));
    }

    // ─── Converting one value ─────────────────────────────────────────────────

    /// A value that is both malformed *and* out of range reports the broken
    /// constraint, which says more than "not a number".
    #[test]
    fn a_broken_constraint_wins_over_the_conversion_that_also_failed() {
        let too_long = FieldSpec {
            control: Control::Text(TextControl {
                maxlength: Some(2),
                ..TextControl::DEFAULT
            }),
            ..spec("n")
        };
        let (parsed, errors) = parse("n=hello", |ctx| ctx.field::<u32>(&too_long));

        assert_eq!(parsed, None);
        assert_eq!(errors.field("n").count(), 1);
        assert!(matches!(
            errors.field("n").next().unwrap().kind,
            crate::ErrorKind::TooLong { .. }
        ));
    }

    /// With nothing else wrong with it, the conversion is what reports.
    #[test]
    fn otherwise_the_conversion_says_what_it_wanted() {
        let (_, errors) = parse("n=hello", |ctx| ctx.field::<u32>(&spec("n")));
        assert_eq!(
            errors.field("n").next().unwrap().kind,
            crate::ErrorKind::Invalid {
                expected: "a whole number".into()
            }
        );
    }

    /// The type's own check needs a converted value to look at, so it runs
    /// last, and only once one exists.
    #[test]
    fn the_types_own_check_runs_after_the_conversion_and_the_constraints() {
        struct Even(u32);

        impl FormValue for Even {
            const CONTROL: Control = Control::NUMBER;

            fn parse_form_value(raw: &str) -> Result<Self, crate::ValueError> {
                raw.parse()
                    .map(Even)
                    .map_err(|_| crate::ValueError::new("a number"))
            }

            fn to_form_value(&self) -> Cow<'static, str> {
                Cow::Owned(self.0.to_string())
            }

            fn validate_form_value(&self) -> Result<(), FieldError> {
                match self.0 % 2 {
                    0 => Ok(()),
                    _ => Err(FieldError::custom("Enter an even number.")),
                }
            }
        }

        let (parsed, errors) = parse("n=4", |ctx| ctx.field::<Even>(&spec("n")));
        assert!(parsed.is_some());
        assert!(errors.is_empty());

        let (parsed, errors) = parse("n=3", |ctx| ctx.field::<Even>(&spec("n")));
        assert!(parsed.is_none());
        assert_eq!(
            errors.field("n").next().unwrap().message.as_str(),
            "Enter an even number."
        );
    }

    // ─── The two `validate = ...` hooks ───────────────────────────────────────

    #[test]
    fn a_field_check_names_the_field_it_rejected() {
        let values = Values::new();
        let mut ctx: ParseCtx<'_, ()> = ParseCtx::new(&values, &());
        ctx.check_custom(&spec("seats"), &3u32, |n: &u32| n.is_multiple_of(2));
        ctx.check_custom(&spec("rows"), &4u32, |n: &u32| n.is_multiple_of(2));

        let errors = ctx.into_errors();
        assert!(errors.has_field("seats"));
        assert!(!errors.has_field("rows"));
    }

    #[test]
    fn a_form_check_may_reject_the_form_or_one_of_its_fields() {
        let values = Values::new();
        let mut ctx: ParseCtx<'_, ()> = ParseCtx::new(&values, &());
        ctx.check_form(&(), |_: &()| Err::<(), _>(("confirm", "They differ.")));
        ctx.check_form(&(), |_: &()| false);

        let errors = ctx.into_errors();
        assert!(errors.has_field("confirm"));
        assert_eq!(errors.form_errors().len(), 1);
    }

    // ─── A value converted by its own `FromStr` ───────────────────────────────

    /// `#[field(from_str)]` wraps the field's type for the length of the parse,
    /// so a foreign type needs no impl of its own.
    #[test]
    fn a_foreign_type_converts_through_its_own_from_str_and_display() {
        use crate::runtime::__private::Str;

        let Str(parsed) = <Str<std::net::Ipv4Addr>>::parse_form_value("10.0.0.1").unwrap();
        assert_eq!(parsed, std::net::Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(Str(parsed).to_form_value(), "10.0.0.1");

        // A plain text input, because a foreign type has no `CONTROL` to give.
        assert_eq!(<Str<std::net::Ipv4Addr>>::CONTROL, Control::TEXT);
    }

    /// The whitespace around a pasted value is a user's slip, not their answer.
    #[test]
    fn the_whitespace_around_a_pasted_value_goes_first() {
        use crate::runtime::__private::Str;

        let Str(parsed) = <Str<u16>>::parse_form_value("  8080  ").unwrap();
        assert_eq!(parsed, 8080);
    }

    /// The message deliberately leaves out what the `FromStr` said. A parse
    /// error speaks to whoever wrote the call, not to whoever filled the form.
    #[test]
    fn a_conversion_that_fails_says_nothing_of_what_the_type_complained() {
        use crate::runtime::__private::Str;

        let error = <Str<u16>>::parse_form_value("nope")
            .err()
            .expect("`nope` is not a number");
        assert_eq!(error.expected, "a valid value");
    }
}
