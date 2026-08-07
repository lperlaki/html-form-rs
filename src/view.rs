//! The render format: a flat, fully resolved, serializable description of a
//! form *as it should appear right now*. It holds the values the user typed and
//! the errors on each field.
//!
//! It is flat and stringly typed on purpose. It has to survive a trip through
//! `serde` into a MiniJinja context, and a template author should not have to
//! know about Rust enums. [`crate::Control`] holds the same information in a
//! form the compiler can check.
//!
//! # Localized strings
//!
//! Every string a person reads has a companion `…_key` field. It is `Some` only
//! while the string is still an unresolved i18n key. Until then the string
//! itself holds that key too, so `{{ field.label }}` renders text either way.
//! Resolve the keys with [`FormView::localize`], or let the template resolve
//! them.
//!
//! Error messages are a *list* of strings, so their keys are a list too.
//! `error_keys[i]` belongs to `errors[i]`, and the two lists always have the
//! same length. A template that only prints the messages can ignore this.
//!
//! # Why every string here is a `Cow`
//!
//! Nearly everything in a view comes from a [`FormSpec`], where it is already a
//! `&'static str`, so the view borrows it. Building a view allocates only for
//! what a spec cannot know. That is the values the user submitted, the ids
//! derived from a field's name, and anything a caller supplies at render time.
//! The owned half of the [`Cow`] keeps the view a plain owned value that a
//! handler can return. It also lets `set_value`, `set_choices` and `localize`
//! put runtime strings in.
//!
//! Serialization does not change. A `Cow` serializes as the string it holds, so
//! a template sees exactly what it always did.

use std::borrow::Cow;

use serde::Serialize;

use crate::Form;
use crate::error::{FieldError, FormErrors};
use crate::kind::FieldKind;
use crate::spec::{Attr, Choice, FormEncType, FormMethod, ResolvedField, Text, sanitize_id};
use crate::values::Values;

/// Split a spec [`Text`] into the string to render now and the key that still
/// waits for a lookup. Both borrow the spec, unless the spec itself was built
/// at runtime.
fn split(text: Option<&Text>) -> (Option<Cow<'static, str>>, Option<Cow<'static, str>>) {
    match text {
        Some(text) => (
            Some(text.content.clone()),
            text.is_key.then(|| text.content.clone()),
        ),
        None => (None, None),
    }
}

/// A [`Text`] split the same way, by value.
fn parts(text: Text) -> (Cow<'static, str>, Option<Cow<'static, str>>) {
    let key = text.is_key.then(|| text.content.clone());
    (text.content, key)
}

/// The messages of a set of errors, split into what to show now and the keys
/// that still wait for a lookup. A view carries the two lists in step.
fn split_messages<'a>(
    errors: impl Iterator<Item = &'a FieldError>,
) -> (Vec<Cow<'static, str>>, Vec<Option<Cow<'static, str>>>) {
    errors.map(|error| parts(error.message.clone())).unzip()
}

/// Look one key up, and drop the key after the lookup, so that nothing later
/// translates it twice. A key that nothing knows stays in place, key and all.
/// Showing `signup.email.label` is a bug a reader can report.
fn resolve(
    text: &mut Option<Cow<'static, str>>,
    key: &mut Option<Cow<'static, str>>,
    translate: &Translate<'_>,
) {
    let Some(found) = key.as_deref().and_then(translate) else {
        return;
    };
    *text = Some(found);
    *key = None;
}

/// [`resolve`], for a string that is always present.
fn resolve_str(
    text: &mut Cow<'static, str>,
    key: &mut Option<Cow<'static, str>>,
    translate: &Translate<'_>,
) {
    let Some(found) = key.as_deref().and_then(translate) else {
        return;
    };
    *text = found;
    *key = None;
}

/// Resolve a list of messages against its list of keys, in step.
fn resolve_messages(
    messages: &mut [Cow<'static, str>],
    keys: &mut [Option<Cow<'static, str>>],
    translate: &Translate<'_>,
) {
    for (message, key) in messages.iter_mut().zip(keys) {
        resolve_str(message, key, translate);
    }
}

/// The erased form of the translation function. It keeps the recursive walk
/// down to each choice from being generic over the whole tree.
type Translate<'a> = dyn Fn(&str) -> Option<Cow<'static, str>> + 'a;

/// A whole form, ready to render.
#[derive(Debug, Clone, Serialize)]
pub struct FormView {
    pub id: Option<Cow<'static, str>>,
    pub name: Option<Cow<'static, str>>,
    pub action: Option<Cow<'static, str>>,
    /// `"post"`, `"get"` or `"dialog"`. Defaults to `"post"`.
    pub method: &'static str,
    pub enctype: Option<&'static str>,
    pub novalidate: bool,
    pub class: Option<Cow<'static, str>>,
    /// The caption of the submit button the built-in renderer emits.
    pub submit_label: Cow<'static, str>,
    /// Set while [`FormView::submit_label`] is still an unresolved i18n key.
    pub submit_label_key: Option<Cow<'static, str>>,
    /// Attributes the crate has no opinion about, in declaration order.
    pub attrs: Vec<AttrView>,
    /// Errors that belong to the whole form rather than to one field.
    pub errors: Vec<Cow<'static, str>>,
    /// One entry per [`FormView::errors`] message, in the same order and of the
    /// same length. An entry is set while its message is still an unresolved
    /// i18n key.
    pub error_keys: Vec<Option<Cow<'static, str>>>,
    /// True when the form, or any of its fields, has an error.
    pub has_errors: bool,
    pub fields: Vec<FieldView>,
}

/// One control, ready to render.
#[derive(Debug, Clone, Serialize)]
pub struct FieldView {
    /// The full submitted name, with every flatten prefix.
    pub name: Cow<'static, str>,
    /// The DOM id of the control.
    pub id: Cow<'static, str>,
    pub label: Option<Cow<'static, str>>,
    /// Set while [`FieldView::label`] is still an unresolved i18n key.
    pub label_key: Option<Cow<'static, str>>,
    /// `"text"`, `"email"`, `"select"`, …
    pub kind: FieldKind,
    /// `"input"`, `"select"` or `"textarea"`.
    pub element: &'static str,
    /// The `type` attribute. A `<select>` and a `<textarea>` have none.
    pub input_type: Option<&'static str>,
    /// The current value: what the user submitted, or, on a blank form, the
    /// field's default.
    pub value: Option<Cow<'static, str>>,
    /// Every current value, for controls that accept more than one.
    pub values: Vec<Cow<'static, str>>,
    /// The current state of a checkbox.
    pub checked: bool,
    pub required: bool,
    pub multiple: bool,
    pub disabled: bool,
    pub readonly: bool,
    pub autofocus: bool,
    pub placeholder: Option<Cow<'static, str>>,
    /// Set while [`FieldView::placeholder`] is still an unresolved i18n key.
    pub placeholder_key: Option<Cow<'static, str>>,
    pub autocomplete: Option<Cow<'static, str>>,
    pub help: Option<Cow<'static, str>>,
    /// Set while [`FieldView::help`] is still an unresolved i18n key.
    pub help_key: Option<Cow<'static, str>>,
    pub pattern: Option<Cow<'static, str>>,
    pub minlength: Option<usize>,
    pub maxlength: Option<usize>,
    pub min: Option<Cow<'static, str>>,
    pub max: Option<Cow<'static, str>>,
    pub step: Option<Cow<'static, str>>,
    pub accept: Option<Cow<'static, str>>,
    pub rows: Option<u32>,
    pub cols: Option<u32>,
    pub class: Option<Cow<'static, str>>,
    /// Attributes the crate has no opinion about, in declaration order.
    pub attrs: Vec<AttrView>,
    /// The options of a `<select>`, a radio group or a checkbox group.
    pub choices: Vec<ChoiceView>,
    /// Messages to show under the control.
    pub errors: Vec<Cow<'static, str>>,
    /// One entry per [`FieldView::errors`] message, in the same order and of
    /// the same length. An entry is set while its message is still an
    /// unresolved i18n key.
    pub error_keys: Vec<Option<Cow<'static, str>>>,
    pub has_errors: bool,
    /// The legend of the flattened group that holds this field, if there is one.
    pub group: Option<Cow<'static, str>>,
    /// Set while [`FieldView::group`] is still an unresolved i18n key.
    pub group_key: Option<Cow<'static, str>>,
    /// The id of the element that lists this field's errors.
    pub error_id: Cow<'static, str>,
    /// The id of the element that carries this field's help text.
    pub help_id: Cow<'static, str>,
    /// The id of the caption that names a radio or checkbox group.
    pub label_id: Cow<'static, str>,
}

/// One custom attribute, ready to render.
#[derive(Debug, Clone, Serialize)]
pub struct AttrView {
    pub name: Cow<'static, str>,
    /// `None` renders a bare boolean attribute.
    pub value: Option<Cow<'static, str>>,
}

impl AttrView {
    fn build(attrs: &'static [Attr]) -> Vec<AttrView> {
        attrs
            .iter()
            .map(|attr| AttrView {
                name: Cow::Borrowed(attr.name),
                value: attr.value.map(Cow::Borrowed),
            })
            .collect()
    }
}

/// One option of a `<select>`, a radio group or a checkbox group.
#[derive(Debug, Clone, Serialize)]
pub struct ChoiceView {
    pub value: Cow<'static, str>,
    pub label: Cow<'static, str>,
    /// Set while [`ChoiceView::label`] is still an unresolved i18n key.
    pub label_key: Option<Cow<'static, str>>,
    pub disabled: bool,
    pub selected: bool,
    pub group: Option<Cow<'static, str>>,
    /// Set while [`ChoiceView::group`] is still an unresolved i18n key.
    pub group_key: Option<Cow<'static, str>>,
}

impl ChoiceView {
    /// Resolve the i18n keys of this option. See [`FormView::localize`].
    pub fn localize<S, F>(&mut self, translate: F)
    where
        F: Fn(&str) -> Option<S>,
        S: Into<Cow<'static, str>>,
    {
        self.localize_in(&|key| translate(key).map(Into::into));
    }

    fn localize_in(&mut self, translate: &Translate<'_>) {
        resolve_str(&mut self.label, &mut self.label_key, translate);
        resolve(&mut self.group, &mut self.group_key, translate);
    }
}

fn method_str(method: Option<FormMethod>) -> &'static str {
    match method {
        Some(FormMethod::Get) => "get",
        Some(FormMethod::Dialog) => "dialog",
        _ => "post",
    }
}

fn enctype_str(enctype: Option<FormEncType>) -> Option<&'static str> {
    Some(match enctype? {
        FormEncType::UrlEncoded => "application/x-www-form-urlencoded",
        FormEncType::MultipartFormData => "multipart/form-data",
        FormEncType::TextPlain => "text/plain",
    })
}

impl FormView {
    /// Build the view of the form `F`.
    ///
    /// Pass `values` to re-render a submission, usually with the errors it
    /// produced. Pass `None` for a blank form, where each field shows its
    /// declared default.
    ///
    /// `context` is what the form's own defaults receive, and it is `&()` for a
    /// form that asks for none. The form supplies both halves — the spec and
    /// the type of the context its glue reads — so the downcast each piece of
    /// glue makes over the [`&dyn Any`](std::any::Any) a
    /// [`FieldDefault`](crate::FieldDefault) is called with finds the type it
    /// asks for.
    pub fn build<F: Form>(
        values: Option<&Values>,
        errors: &FormErrors,
        context: &F::Context,
    ) -> FormView {
        // The walk goes straight into the view. It does not collect the
        // flattened field list first, so it builds only the `FieldView`s.
        let mut fields = Vec::new();
        // The context is `F::Context` and the spec is `F::SPEC`, which is the
        // pairing `Form` is about: the glue in `SPEC` downcasts to `F::Context`
        // and to nothing else. The derive writes both halves together.
        F::SPEC.walk_with_context(context, |resolved| {
            fields.push(FieldView::build(resolved, values, errors));
        });
        let spec = F::SPEC;

        let (form_errors, form_error_keys) = split_messages(errors.form_errors().iter());

        let (submit_label, submit_label_key) = split(spec.submit_label.as_ref());

        FormView {
            id: spec.id.map(Cow::Borrowed),
            name: spec.name.map(Cow::Borrowed),
            action: spec.action.map(Cow::Borrowed),
            method: method_str(spec.method),
            enctype: enctype_str(spec.enctype),
            novalidate: spec.novalidate,
            class: spec.class.map(Cow::Borrowed),
            submit_label: submit_label.unwrap_or(Cow::Borrowed("Submit")),
            submit_label_key,
            attrs: AttrView::build(spec.attrs),
            has_errors: !form_errors.is_empty() || fields.iter().any(|f| f.has_errors),
            errors: form_errors,
            error_keys: form_error_keys,
            fields,
        }
    }

    /// Resolve every i18n key in the form to the text `translate` returns for
    /// it. Literal text and unknown keys stay as they are.
    ///
    /// The crate has no opinion about your i18n stack. `translate` is any
    /// `Fn(&str) -> Option<impl Into<String>>`, so a closure over whatever your
    /// backend calls a bundle is enough.
    ///
    /// ```
    /// # use html_form::Form;
    /// # #[derive(Form)]
    /// # struct Signup {
    /// #     #[field(label = t("signup.email"))]
    /// #     email: String,
    /// # }
    /// let mut view = Signup::render();
    /// view.localize(|key| match key {
    ///     "signup.email" => Some("E-Mail-Adresse"),
    ///     _ => None,
    /// });
    /// assert_eq!(view.field("email").unwrap().label.as_deref(), Some("E-Mail-Adresse"));
    /// ```
    pub fn localize<S, F>(&mut self, translate: F)
    where
        F: Fn(&str) -> Option<S>,
        S: Into<Cow<'static, str>>,
    {
        self.localize_in(&|key| translate(key).map(Into::into));
    }

    /// [`FormView::localize`], by value, so you can chain it onto `render()`.
    #[must_use]
    pub fn localized<S, F>(mut self, translate: F) -> Self
    where
        F: Fn(&str) -> Option<S>,
        S: Into<Cow<'static, str>>,
    {
        self.localize(translate);
        self
    }

    fn localize_in(&mut self, translate: &Translate<'_>) {
        resolve_str(
            &mut self.submit_label,
            &mut self.submit_label_key,
            translate,
        );
        resolve_messages(&mut self.errors, &mut self.error_keys, translate);
        for field in &mut self.fields {
            field.localize_in(translate);
        }
    }

    /// Find a field by its full, prefixed name.
    pub fn field(&self, name: &str) -> Option<&FieldView> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// The same lookup, but mutable, to change a field before it renders. Use
    /// it to fill in choices that come from a database, for example.
    pub fn field_mut(&mut self, name: &str) -> Option<&mut FieldView> {
        self.fields.iter_mut().find(|f| f.name == name)
    }

    /// Attach an error to a field later, such as a duplicate value that only
    /// the database could find.
    ///
    /// The message may be a [`Text::key`](crate::Text::key). A later
    /// [`localize`](FormView::localize) resolves it like any other key.
    ///
    /// Returns `false` if the field does not exist.
    pub fn add_field_error(&mut self, name: &str, message: impl Into<Text>) -> bool {
        match self.field_mut(name) {
            Some(field) => {
                field.add_error(message);
                self.has_errors = true;
                true
            }
            None => false,
        }
    }

    /// Set a custom attribute on the `<form>`. It replaces any attribute of the
    /// same name.
    ///
    /// Pass `None` as the value to get a bare boolean attribute.
    ///
    /// A *value* may be anything: the renderer escapes it, and a template
    /// engine escapes it. A **name** is written outside the quotes, where
    /// escaping cannot help, so it has to be a name markup can carry. Build one
    /// from a constant, not from input.
    ///
    /// Returns `false`, and sets nothing, if the name is one
    /// [`is_attr_name`] rejects, or one the `<form>` already carries — `id`,
    /// `name`, `action`, `method`, `enctype`, `class` or `novalidate`. Setting
    /// those would render the attribute twice. `#[form(attr(...))]` rejects the
    /// same names while the crate compiles.
    pub fn set_attr(&mut self, name: impl Into<Cow<'static, str>>, value: Option<&str>) -> bool {
        set_attr(&mut self.attrs, FORM_RESERVED, name.into(), value)
    }

    /// Add a form-level error, as text or as an i18n key.
    pub fn add_error(&mut self, message: impl Into<Text>) {
        let (message, key) = parts(message.into());
        self.errors.push(message);
        self.error_keys.push(key);
        self.has_errors = true;
    }
}

impl FieldView {
    fn build(
        resolved: ResolvedField<'_>,
        values: Option<&Values>,
        errors: &FormErrors,
    ) -> FieldView {
        let spec = resolved.spec;
        let control = &spec.control;
        let kind = control.kind();

        // A field that resets belongs to the form, not to the user. Nobody
        // typed it, and a rejected token sent back would make the retry fail
        // exactly as the first attempt did. So the field renders as it does on
        // a blank form, and drops what came in. Dropping it here, before
        // anything reads it, is also what keeps the crate from copying a
        // submission it is about to throw away.
        //
        // The spec is the whole answer. A hidden field whose default the form
        // produces already resets there, without being told to. See
        // `FieldSpec::reset`.
        let values = if spec.reset { None } else { values };

        // The default is what a blank form starts with, so it is wanted only
        // where there is nothing else to show. Asking for it any later would
        // run a generator whose value the render is about to drop, and a
        // generator may be handing out something the server also records.
        //
        // A checkable control asks all the same: for a radio or a checkbox
        // group the default *is* the value the control carries, and the
        // submission only says whether that value came back.
        let default = match values.is_none() || control.is_checkable() {
            true => resolved.default(),
            false => None,
        };

        // Everything from here on reads the name, which the view keeps.
        let ResolvedField { name, group, .. } = resolved;
        let full_name = &name;

        let checked = if control.is_checkable() {
            match values {
                // A lone radio is checked when its own value came back. A radio
                // or checkbox *group* tracks the selection per choice instead.
                Some(values) => match (kind, values.get(full_name)) {
                    (FieldKind::Radio | FieldKind::CheckboxGroup, Some(submitted)) => {
                        default.as_deref().is_none_or(|own| own == submitted)
                    }
                    (FieldKind::Radio | FieldKind::CheckboxGroup, None) => false,
                    (_, Some(raw)) => {
                        <bool as crate::value::FormValue>::parse_form_value(raw).unwrap_or(true)
                    }
                    (_, None) => false,
                },
                None => matches!(default.as_deref(), Some("true" | "on" | "1" | "yes")),
            }
        } else {
            false
        };

        // On a blank form the default is the value. On a re-render the
        // submission is, so an unchecked box or a cleared field stays that way.
        // A literal default comes from the spec, borrowed. Only what the user
        // typed has to be copied out of the submission.
        let current: Vec<Cow<'static, str>> = match values {
            // Once there are values to show, such as a submission to re-render
            // or a record to edit, an empty field is empty because that is what
            // it holds. A form that filled it in would put a value the caller
            // never had in front of the user, for them to save without
            // noticing.
            Some(values) => values
                .all(full_name)
                .map(|value| Cow::Owned(value.to_owned()))
                .collect(),
            // A checkbox default says whether the box starts checked. It does
            // not say what to put in a `value` attribute, because a box with no
            // value submits "on".
            None if kind == FieldKind::Checkbox => Vec::new(),
            None => default.into_iter().collect(),
        };

        let choices = control
            .choices()
            .iter()
            .map(|choice| {
                // An option with no label of its own uses its value as the
                // label. That value is literal text, however you declared the
                // other labels.
                let (label, label_key) = if choice.label.is_empty() {
                    (choice.value.clone(), None)
                } else {
                    let (label, key) = split(Some(&choice.label));
                    (label.unwrap_or_default(), key)
                };
                let (group, group_key) = split(choice.group.as_ref());
                ChoiceView {
                    selected: current.contains(&choice.value),
                    value: choice.value.clone(),
                    label,
                    label_key,
                    disabled: choice.disabled,
                    group,
                    group_key,
                }
            })
            .collect();

        let (messages, message_keys) = split_messages(errors.field(full_name));

        let base = sanitize_id(full_name);
        let id = spec.id_for(full_name);
        let bounds = control.bounds();
        let (label, label_key) = split(spec.label.as_ref());
        let (help, help_key) = split(spec.help.as_ref());
        let (placeholder, placeholder_key) = split(spec.placeholder.as_ref());
        let (group, group_key) = split(group);

        FieldView {
            error_id: suffixed(&base, "-error"),
            help_id: suffixed(&base, "-help"),
            label_id: suffixed(&base, "-label"),
            id,
            label,
            label_key,
            kind,
            element: kind.element(),
            input_type: kind.input_type(),
            value: current.first().cloned(),
            values: current,
            name,
            checked,
            required: spec.required,
            multiple: control.multiple(),
            disabled: spec.disabled,
            readonly: spec.readonly,
            autofocus: spec.autofocus,
            placeholder,
            placeholder_key,
            autocomplete: spec.autocomplete.map(Cow::Borrowed),
            help,
            help_key,
            pattern: control.pattern().map(Cow::Borrowed),
            minlength: control.minlength(),
            maxlength: control.maxlength(),
            min: bounds.and_then(|b| b.min).map(Cow::Borrowed),
            max: bounds.and_then(|b| b.max).map(Cow::Borrowed),
            step: bounds.and_then(|b| b.step).map(Cow::Borrowed),
            accept: control.accept().map(Cow::Borrowed),
            rows: control.rows(),
            cols: control.cols(),
            class: spec.class.map(Cow::Borrowed),
            attrs: AttrView::build(spec.attrs),
            choices,
            has_errors: !messages.is_empty(),
            errors: messages,
            error_keys: message_keys,
            group,
            group_key,
        }
    }

    /// Resolve the i18n keys of this field: its label, help text, placeholder,
    /// group legend, and the label of every option. See
    /// [`FormView::localize`].
    pub fn localize<S, F>(&mut self, translate: F)
    where
        F: Fn(&str) -> Option<S>,
        S: Into<Cow<'static, str>>,
    {
        self.localize_in(&|key| translate(key).map(Into::into));
    }

    fn localize_in(&mut self, translate: &Translate<'_>) {
        resolve(&mut self.label, &mut self.label_key, translate);
        resolve(&mut self.help, &mut self.help_key, translate);
        resolve(&mut self.placeholder, &mut self.placeholder_key, translate);
        resolve(&mut self.group, &mut self.group_key, translate);
        resolve_messages(&mut self.errors, &mut self.error_keys, translate);
        for choice in &mut self.choices {
            choice.localize_in(translate);
        }
    }

    /// Replace the current value.
    pub fn set_value(&mut self, value: impl Into<Cow<'static, str>>) {
        let value = value.into();
        for choice in &mut self.choices {
            choice.selected = choice.value == value;
        }
        self.checked = matches!(value.as_ref(), "true" | "on" | "1" | "yes");
        self.values = vec![value.clone()];
        self.value = Some(value);
    }

    /// Replace the options and keep the current selection.
    pub fn set_choices<I>(&mut self, choices: I)
    where
        I: IntoIterator<Item = Choice>,
    {
        let current = std::mem::take(&mut self.values);
        self.choices = choices
            .into_iter()
            .map(|choice| {
                let (label, label_key) = split(Some(&choice.label));
                let (group, group_key) = split(choice.group.as_ref());
                ChoiceView {
                    selected: current.contains(&choice.value),
                    value: choice.value,
                    label: label.unwrap_or_default(),
                    label_key,
                    disabled: choice.disabled,
                    group,
                    group_key,
                }
            })
            .collect();
        self.values = current;
    }

    /// Set a custom attribute on this control. It replaces any attribute of the
    /// same name.
    ///
    /// Pass `None` as the value to get a bare boolean attribute.
    ///
    /// A *value* may be anything: the renderer escapes it, and a template
    /// engine escapes it. A **name** is written outside the quotes, where
    /// escaping cannot help, so it has to be a name markup can carry. Build one
    /// from a constant, not from input.
    ///
    /// Returns `false`, and sets nothing, if the name is one
    /// [`is_attr_name`] rejects, or one the control already carries — `name`,
    /// `id`, `type`, `value`, `required` and the rest the crate renders from
    /// the spec. Setting those would render the attribute twice.
    /// `#[field(attr(...))]` rejects the same names while the crate compiles.
    pub fn set_attr(&mut self, name: impl Into<Cow<'static, str>>, value: Option<&str>) -> bool {
        set_attr(&mut self.attrs, FIELD_RESERVED, name.into(), value)
    }

    /// Add another error message to this field, as text or as an i18n key.
    pub fn add_error(&mut self, message: impl Into<Text>) {
        let (message, key) = parts(message.into());
        self.errors.push(message);
        self.error_keys.push(key);
        self.has_errors = true;
    }

    /// The value of `aria-describedby`, which links the help text and the error
    /// list.
    ///
    /// It borrows from the field itself, unless it has to list both ids.
    pub fn described_by(&self) -> Option<Cow<'_, str>> {
        match (self.help.is_some(), self.has_errors) {
            (false, false) => None,
            (true, false) => Some(Cow::Borrowed(self.help_id.as_ref())),
            (false, true) => Some(Cow::Borrowed(self.error_id.as_ref())),
            (true, true) => {
                let mut both = String::with_capacity(self.help_id.len() + 1 + self.error_id.len());
                both.push_str(&self.help_id);
                both.push(' ');
                both.push_str(&self.error_id);
                Some(Cow::Owned(both))
            }
        }
    }
}

/// `base` with `suffix` added. These are the ids the view derives from a field
/// name.
fn suffixed(base: &str, suffix: &str) -> Cow<'static, str> {
    let mut out = String::with_capacity(base.len() + suffix.len());
    out.push_str(base);
    out.push_str(suffix);
    Cow::Owned(out)
}

/// Whether `name` is a name that may be written out as one.
///
/// A *value* is safe however it was built: the built-in renderer escapes it, and
/// a template engine escapes it. A name is not, because it sits outside the
/// quotes. A space in one would end it and start a second attribute nobody
/// declared — `set_attr("x onfocus=alert(1)", Some("…"))` would otherwise render
/// a live event handler — and escaping does not help, because a space needs no
/// escape.
///
/// So this is the HTML attribute-name production narrowed to what a real one
/// uses: no whitespace, no quote, no `/`, `=`, `<`, `>` or `&`, and no control
/// character. Every `data-*`, `aria-*`, `hx-*` and plain word passes untouched.
///
/// [`FieldView::set_attr`] and [`FormView::set_attr`] refuse a name this
/// rejects, and `#[field(attr(...))]` is a compile error for the same set — the
/// derive checks these very characters, so a name that compiles is a name that
/// renders. It is public because [`AttrView::name`] is a public field: a view
/// built or edited some other way can check a name the same way the crate does.
///
/// ```
/// assert!(html_form::is_attr_name("data-tooltip"));
/// assert!(!html_form::is_attr_name("x onfocus=alert(1)"));
/// assert!(!html_form::is_attr_name(""));
/// ```
pub fn is_attr_name(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().any(|ch| {
            ch.is_whitespace()
                || ch.is_control()
                || matches!(ch, '"' | '\'' | '>' | '<' | '/' | '=' | '&')
        })
}

/// The attribute names the built-in renderer writes on a `<form>` itself.
///
/// Setting one as a *custom* attribute would render it twice, which is invalid
/// markup, and the browser would keep the crate's copy anyway. The derive
/// rejects the same names at compile time, with a message naming the `#[form]`
/// key that sets each one; this is the runtime half, and the two lists are
/// meant to hold the same names.
const FORM_RESERVED: &[&str] = &[
    "id",
    "name",
    "action",
    "method",
    "enctype",
    "class",
    "novalidate",
];

/// The attribute names the built-in renderer writes on a control itself. See
/// [`FORM_RESERVED`].
const FIELD_RESERVED: &[&str] = &[
    "name",
    "id",
    "type",
    "value",
    "class",
    "required",
    "disabled",
    "readonly",
    "autofocus",
    "multiple",
    "placeholder",
    "autocomplete",
    "pattern",
    "min",
    "max",
    "step",
    "accept",
    "minlength",
    "maxlength",
    "rows",
    "cols",
    "checked",
    "selected",
    "aria-invalid",
    "aria-describedby",
];

/// Overwrite an attribute in place and keep its position, or add it at the end.
///
/// Refuses a name markup could not carry, and one the renderer writes itself.
fn set_attr(
    attrs: &mut Vec<AttrView>,
    reserved: &[&str],
    name: Cow<'static, str>,
    value: Option<&str>,
) -> bool {
    if !is_attr_name(&name) || reserved.contains(&name.to_ascii_lowercase().as_str()) {
        return false;
    }
    let value = value.map(|value| Cow::Owned(value.to_owned()));
    match attrs.iter_mut().find(|a| a.name == name) {
        Some(existing) => existing.value = value,
        None => attrs.push(AttrView { name, value }),
    }
    true
}
