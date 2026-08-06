//! The render format: a flat, fully-resolved, serialisable description of a
//! form *as it should appear right now* — including the values the user already
//! typed and the errors attached to each field.
//!
//! This is deliberately flat and stringly-typed: it has to survive a trip
//! through `serde` into a MiniJinja context, and template authors should not
//! have to know about Rust enums. [`crate::Control`] is where the same
//! information lives in a form the compiler can check.
//!
//! # Localised strings
//!
//! Every string a person reads comes with a companion `…_key` field. It is
//! `Some` only while the string is still an unresolved i18n key — which is also
//! what the string itself holds until then, so `{{ field.label }}` renders
//! something either way. Resolve them with [`FormView::localize`], or let the
//! template do it from the key.
//!
//! Error messages are a *list* of strings, so their keys are a list too:
//! `error_keys[i]` belongs to `errors[i]`, and the two are always the same
//! length. A template that just prints the messages needs to know nothing
//! about it.
//!
//! # Why every string here is a `Cow`
//!
//! Nearly everything in a view was copied out of a [`FormSpec`], where it is
//! already `&'static str`, so the view borrows it: building one allocates only
//! for what a spec cannot know — the values the user submitted, the ids derived
//! from a field's name, and anything a caller supplies at render time. The
//! owned half of the [`Cow`] is what keeps the view a plain owned value that a
//! handler can return, and what lets `set_value`, `set_choices` and `localize`
//! put runtime strings in.
//!
//! Serialisation is unaffected: a `Cow` serialises as the string it holds, so a
//! template sees exactly what it always did.

use std::borrow::Cow;

use serde::Serialize;

use crate::error::{FieldError, FormErrors};
use crate::kind::FieldKind;
use crate::spec::{Attr, Choice, FormEncType, FormMethod, FormSpec, Text, sanitize_id};
use crate::values::Values;

/// Split a spec [`Text`] into the string to render now and the key that is
/// still waiting to be resolved. Both borrow the spec unless it was itself
/// built at runtime.
fn split(text: Option<&Text>) -> (Option<Cow<'static, str>>, Option<Cow<'static, str>>) {
    match text {
        Some(text) => (
            Some(text.content.clone()),
            text.is_key.then(|| text.content.clone()),
        ),
        None => (None, None),
    }
}

/// A [`Text`] taken apart the same way, by value.
fn parts(text: Text) -> (Cow<'static, str>, Option<Cow<'static, str>>) {
    let key = text.is_key.then(|| text.content.clone());
    (text.content, key)
}

/// The messages of a set of errors, split into what to show now and the keys
/// still waiting to be resolved — the two index-aligned lists a view carries.
fn split_messages<'a>(
    errors: impl Iterator<Item = &'a FieldError>,
) -> (Vec<Cow<'static, str>>, Vec<Option<Cow<'static, str>>>) {
    errors.map(|error| parts(error.message.clone())).unzip()
}

/// Look one key up, and drop it once it has been resolved so that nothing
/// downstream translates it twice. An unrecognised key is left in place, key and
/// all: showing `signup.email.label` is a bug a reader can report.
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

/// [`resolve`], for a string that is always there to be replaced.
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

/// The erased form of the translation function, so the recursive walk down to
/// each choice is not generic over the whole tree.
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
    /// Caption of the submit button the built-in renderer emits.
    pub submit_label: Cow<'static, str>,
    /// Set while [`FormView::submit_label`] is still an unresolved i18n key.
    pub submit_label_key: Option<Cow<'static, str>>,
    /// Attributes the crate has no opinion about, in declaration order.
    pub attrs: Vec<AttrView>,
    /// Errors that belong to the form as a whole rather than to one field.
    pub errors: Vec<Cow<'static, str>>,
    /// One entry per [`FormView::errors`] message — same length, same order —
    /// set while that message is still an unresolved i18n key.
    pub error_keys: Vec<Option<Cow<'static, str>>>,
    /// True when the form or any of its fields has an error.
    pub has_errors: bool,
    pub fields: Vec<FieldView>,
}

/// One control, ready to render.
#[derive(Debug, Clone, Serialize)]
pub struct FieldView {
    /// Full submitted name, including any flatten prefixes.
    pub name: Cow<'static, str>,
    /// DOM id of the control.
    pub id: Cow<'static, str>,
    pub label: Option<Cow<'static, str>>,
    /// Set while [`FieldView::label`] is still an unresolved i18n key.
    pub label_key: Option<Cow<'static, str>>,
    /// `"text"`, `"email"`, `"select"`, …
    pub kind: FieldKind,
    /// `"input"`, `"select"` or `"textarea"`.
    pub element: &'static str,
    /// The `type` attribute, absent for `<select>` and `<textarea>`.
    pub input_type: Option<&'static str>,
    /// The current value: what the user submitted, or the field's default on a
    /// blank form.
    pub value: Option<Cow<'static, str>>,
    /// Every current value, for controls that accept more than one.
    pub values: Vec<Cow<'static, str>>,
    /// Current state of a checkbox.
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
    /// Options of a `<select>`, radio group or checkbox group.
    pub choices: Vec<ChoiceView>,
    /// Messages to show under the control.
    pub errors: Vec<Cow<'static, str>>,
    /// One entry per [`FieldView::errors`] message — same length, same order —
    /// set while that message is still an unresolved i18n key.
    pub error_keys: Vec<Option<Cow<'static, str>>>,
    pub has_errors: bool,
    /// Legend of the flattened group this field belongs to, if any.
    pub group: Option<Cow<'static, str>>,
    /// Set while [`FieldView::group`] is still an unresolved i18n key.
    pub group_key: Option<Cow<'static, str>>,
    /// Id of the element listing this field's errors.
    pub error_id: Cow<'static, str>,
    /// Id of the element carrying this field's help text.
    pub help_id: Cow<'static, str>,
    /// Id of the caption naming a radio or checkbox group.
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

/// One option of a `<select>`, radio group or checkbox group.
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
    /// Resolve this option's i18n keys. See [`FormView::localize`].
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
    /// Build the view for `spec`.
    ///
    /// Pass `values` to re-render a submission (typically together with the
    /// errors it produced); pass `None` for a blank form, where each field
    /// falls back to its declared default.
    ///
    /// `generated` is what the form produced for *this* render —
    /// [`WebForm::defaults_with_context`](crate::WebForm::defaults_with_context),
    /// keyed by fully-qualified field name, and `None` for a form that declares
    /// no generated default. It is separate from `values` because the two are
    /// not interchangeable: a value the form minted itself is minted again on a
    /// re-render, where one the user typed is echoed back.
    pub fn build(
        spec: &FormSpec,
        values: Option<&Values>,
        generated: Option<&Values>,
        errors: &FormErrors,
    ) -> FormView {
        // The flattened field list is walked straight into the view rather than
        // collected first: nothing but the `FieldView`s themselves is built.
        let mut fields = Vec::new();
        spec.walk(|resolved| {
            fields.push(FieldView::build(
                resolved.name,
                resolved.spec,
                resolved.group,
                values,
                generated,
                errors,
            ));
        });

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
    /// it, leaving literal text and unrecognised keys alone.
    ///
    /// The crate has no opinion about your i18n stack: `translate` is any
    /// `Fn(&str) -> Option<impl Into<String>>`, so a closure over whatever your
    /// backend calls a bundle is enough.
    ///
    /// ```
    /// # use web_form::WebForm;
    /// # #[derive(WebForm)]
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

    /// [`FormView::localize`], by value, so it can be chained onto `render()`.
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

    /// Look up a field by its full (prefixed) name.
    pub fn field(&self, name: &str) -> Option<&FieldView> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Mutable lookup, for adjusting a field before rendering — filling in
    /// choices loaded from a database, for instance.
    pub fn field_mut(&mut self, name: &str) -> Option<&mut FieldView> {
        self.fields.iter_mut().find(|f| f.name == name)
    }

    /// Attach an error to a field after the fact, e.g. a uniqueness violation
    /// only the database could detect.
    ///
    /// The message may be a [`Text::key`](crate::Text::key), which a later
    /// [`localize`](FormView::localize) resolves like any other.
    ///
    /// Returns `false` if no such field exists.
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

    /// Set a custom attribute on the `<form>`, replacing any of the same name.
    ///
    /// Pass `None` as the value for a bare boolean attribute.
    pub fn set_attr(&mut self, name: impl Into<Cow<'static, str>>, value: Option<&str>) {
        set_attr(&mut self.attrs, name.into(), value);
    }

    /// Attach a form-level error, as text or as an i18n key.
    pub fn add_error(&mut self, message: impl Into<Text>) {
        let (message, key) = parts(message.into());
        self.errors.push(message);
        self.error_keys.push(key);
        self.has_errors = true;
    }
}

impl FieldView {
    fn build(
        name: Cow<'static, str>,
        spec: &'static crate::spec::FieldSpec,
        group: Option<&Text>,
        values: Option<&Values>,
        generated: Option<&Values>,
        errors: &FormErrors,
    ) -> FieldView {
        let control = &spec.control;
        let kind = control.kind();
        let full_name = &name;

        // What the form generated for this render, if anything, stands ahead of
        // the literal in the spec. It was produced once, before the walk: a
        // generator consulted twice would hand out two different tokens.
        let generated = generated
            .and_then(|generated| generated.get(full_name))
            .map(|value| Cow::Owned(value.to_owned()));
        let is_generated = generated.is_some();
        let default = generated.or_else(|| spec.default.map(Cow::Borrowed));

        let checked = if control.is_checkable() {
            match values {
                // A standalone radio is checked when its own value came back;
                // a radio or checkbox *group* tracks selection per choice
                // instead.
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

        // On a blank form the default is the value; on a re-render the
        // submission is, so that an unchecked box or a cleared field stays that
        // way. A literal default comes from the spec and is borrowed; only what
        // the user typed has to be copied out of the submission.
        let current: Vec<Cow<'static, str>> = match values {
            Some(values) => {
                // A *generated* default is what the form itself supplies, so it
                // is minted again wherever the submission has nothing the user
                // would recognise as their own. For a hidden field that is
                // always: nobody typed it, and echoing a rejected token back
                // would leave the retry failing exactly as the first attempt
                // did. A literal default stays out of this — it has been on
                // screen once already, and restoring it would undo a deliberate
                // clearing.
                let regenerate = is_generated
                    && (kind == FieldKind::Hidden
                        || (!values.contains(full_name) && !control.is_checkable()));
                if regenerate {
                    default.into_iter().collect()
                } else {
                    // Decided before anything is copied, so the submission a
                    // regenerated field is about to discard is never collected.
                    values
                        .all(full_name)
                        .map(|value| Cow::Owned(value.to_owned()))
                        .collect()
                }
            }
            // A checkbox default says whether the box starts checked, not what
            // to put in a `value` attribute — a box with no value submits "on".
            None if kind == FieldKind::Checkbox => Vec::new(),
            None => default.into_iter().collect(),
        };

        let choices = control
            .choices()
            .iter()
            .map(|choice| {
                // An option with no label of its own is labelled by its value,
                // which is literal text however the others were declared.
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

    /// Resolve this field's i18n keys — its label, help text, placeholder,
    /// group legend and the label of every option. See [`FormView::localize`].
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

    /// Replace the options, keeping the current selection.
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

    /// Set a custom attribute on this control, replacing any of the same name.
    ///
    /// Pass `None` as the value for a bare boolean attribute.
    pub fn set_attr(&mut self, name: impl Into<Cow<'static, str>>, value: Option<&str>) {
        set_attr(&mut self.attrs, name.into(), value);
    }

    /// Attach another error message to this field, as text or as an i18n key.
    pub fn add_error(&mut self, message: impl Into<Text>) {
        let (message, key) = parts(message.into());
        self.errors.push(message);
        self.error_keys.push(key);
        self.has_errors = true;
    }

    /// The value of `aria-describedby`, linking help text and error list.
    ///
    /// Borrowed from the field itself unless both ids have to be listed.
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

/// `base` with `suffix` appended — the ids the view derives from a field name.
fn suffixed(base: &str, suffix: &str) -> Cow<'static, str> {
    let mut out = String::with_capacity(base.len() + suffix.len());
    out.push_str(base);
    out.push_str(suffix);
    Cow::Owned(out)
}

/// Overwrite an attribute in place, keeping its position, or append it.
fn set_attr(attrs: &mut Vec<AttrView>, name: Cow<'static, str>, value: Option<&str>) {
    let value = value.map(|value| Cow::Owned(value.to_owned()));
    match attrs.iter_mut().find(|a| a.name == name) {
        Some(existing) => existing.value = value,
        None => attrs.push(AttrView { name, value }),
    }
}
