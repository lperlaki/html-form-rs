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
use std::fmt::{self, Write as _};

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
    pub fn build(spec: &FormSpec, values: Option<&Values>, errors: &FormErrors) -> FormView {
        // The flattened field list is walked straight into the view rather than
        // collected first: nothing but the `FieldView`s themselves is built.
        let mut fields = Vec::new();
        spec.walk(|resolved| {
            fields.push(FieldView::build(
                resolved.name,
                resolved.spec,
                resolved.group,
                values,
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

    /// Render the complete `<form>` element.
    ///
    /// The markup is plain and unstyled; every element carries a `web-form__*`
    /// class to hook CSS onto. Reach for a template engine and the serialised
    /// view when you need control over the markup itself.
    pub fn to_html(&self) -> String {
        let mut out = String::with_capacity(1024);
        out.push_str("<form");
        attr_opt(&mut out, "id", self.id.as_deref());
        attr_opt(&mut out, "name", self.name.as_deref());
        attr_opt(&mut out, "action", self.action.as_deref());
        attr(&mut out, "method", self.method);
        attr_opt(&mut out, "enctype", self.enctype);
        out.push_str(" class=\"web-form");
        if let Some(class) = &self.class {
            out.push(' ');
            escape_into(&mut out, class);
        }
        out.push('"');
        flag(&mut out, "novalidate", self.novalidate);
        write_attrs(&mut out, &self.attrs);
        out.push_str(">\n");

        if !self.errors.is_empty() {
            out.push_str("  <ul class=\"web-form__errors\">\n");
            for message in &self.errors {
                out.push_str("    <li>");
                escape_into(&mut out, message);
                out.push_str("</li>\n");
            }
            out.push_str("  </ul>\n");
        }

        // Consecutive fields carrying the same group legend are wrapped in one
        // <fieldset>, which is how a flattened sub-form keeps its identity.
        let mut open_group: Option<&str> = None;
        for field in &self.fields {
            let group = field.group.as_deref();
            if group != open_group {
                if open_group.is_some() {
                    out.push_str("  </fieldset>\n");
                }
                if let Some(legend) = group {
                    out.push_str("  <fieldset class=\"web-form__group\">\n    <legend>");
                    escape_into(&mut out, legend);
                    out.push_str("</legend>\n");
                }
                open_group = group;
            }
            field.write_html(&mut out);
        }
        if open_group.is_some() {
            out.push_str("  </fieldset>\n");
        }

        out.push_str("  <button type=\"submit\" class=\"web-form__submit\">");
        escape_into(&mut out, &self.submit_label);
        out.push_str("</button>\n</form>");
        out
    }
}

impl fmt::Display for FormView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_html())
    }
}

impl FieldView {
    fn build(
        name: Cow<'static, str>,
        spec: &'static crate::spec::FieldSpec,
        group: Option<&Text>,
        values: Option<&Values>,
        errors: &FormErrors,
    ) -> FieldView {
        let control = &spec.control;
        let kind = control.kind();
        let full_name = &name;

        // On a blank form the default is the value; on a re-render the
        // submission is, so that an unchecked box or a cleared field stays that
        // way. A default comes from the spec and is borrowed; only what the user
        // typed has to be copied out of the submission.
        let current: Vec<Cow<'static, str>> = match values {
            Some(values) => values
                .all(full_name)
                .map(|value| Cow::Owned(value.to_owned()))
                .collect(),
            None => spec.default.iter().copied().map(Cow::Borrowed).collect(),
        };

        let checked = if control.is_checkable() {
            match values {
                // A standalone radio is checked when its own value came back;
                // a radio or checkbox *group* tracks selection per choice
                // instead.
                Some(values) => match (kind, values.get(full_name)) {
                    (FieldKind::Radio | FieldKind::CheckboxGroup, Some(submitted)) => {
                        spec.default.is_none_or(|own| own == submitted)
                    }
                    (FieldKind::Radio | FieldKind::CheckboxGroup, None) => false,
                    (_, Some(raw)) => {
                        <bool as crate::value::FormValue>::parse_form_value(raw).unwrap_or(true)
                    }
                    (_, None) => false,
                },
                None => matches!(spec.default, Some("true" | "on" | "1" | "yes")),
            }
        } else {
            false
        };

        // A checkbox default says whether the box starts checked, not what to
        // put in a `value` attribute — a box with no value submits "on".
        let current = match (kind, values) {
            (FieldKind::Checkbox, None) => Vec::new(),
            _ => current,
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

    /// Render just the control, without label, help text or errors.
    pub fn control_html(&self) -> String {
        let mut out = String::with_capacity(self.html_size());
        self.write_control(&mut out);
        out
    }

    /// Render the label, control, help text and error list.
    pub fn to_html(&self) -> String {
        let mut out = String::with_capacity(self.html_size());
        self.write_html(&mut out);
        out
    }

    /// Roughly what this field's markup will come to, so the buffer it is
    /// written into does not have to grow its way there.
    fn html_size(&self) -> usize {
        256 + self.choices.len() * 96 + self.errors.len() * 64
    }

    fn write_html(&self, out: &mut String) {
        // A hidden field has nothing to label and nothing to describe.
        if self.kind == FieldKind::Hidden {
            self.write_control(out);
            out.push('\n');
            return;
        }

        out.push_str("  <div class=\"web-form__field");
        if self.has_errors {
            out.push_str(" web-form__field--invalid");
        }
        out.push_str("\" data-field=\"");
        escape_into(out, &self.name);
        out.push_str("\">\n");

        // A checkbox (or a lone radio) is labelled after the box; a group is
        // captioned before its options.
        let label_after = self.kind == FieldKind::Checkbox
            || (matches!(self.kind, FieldKind::Radio | FieldKind::CheckboxGroup)
                && self.choices.is_empty());
        if !label_after {
            self.write_label(out);
        }
        out.push_str("    ");
        self.write_control(out);
        out.push('\n');
        if label_after {
            self.write_label(out);
        }

        if let Some(help) = &self.help {
            out.push_str("    <p class=\"web-form__help\" id=\"");
            escape_into(out, &self.help_id);
            out.push_str("\">");
            escape_into(out, help);
            out.push_str("</p>\n");
        }

        if self.has_errors {
            out.push_str("    <ul class=\"web-form__errors\" id=\"");
            escape_into(out, &self.error_id);
            out.push_str("\">\n");
            for message in &self.errors {
                out.push_str("      <li>");
                escape_into(out, message);
                out.push_str("</li>\n");
            }
            out.push_str("    </ul>\n");
        }

        out.push_str("  </div>\n");
    }

    fn write_label(&self, out: &mut String) {
        let Some(label) = &self.label else { return };
        // A group labels each option; the group itself gets a plain caption
        // rather than a `for=` that points at only its first control.
        if matches!(self.kind, FieldKind::Radio | FieldKind::CheckboxGroup)
            && !self.choices.is_empty()
        {
            out.push_str("    <span class=\"web-form__label\" id=\"");
            escape_into(out, &self.label_id);
            out.push_str("\">");
            escape_into(out, label);
            if self.required {
                out.push_str(" <span class=\"web-form__required\" aria-hidden=\"true\">*</span>");
            }
            out.push_str("</span>\n");
            return;
        }
        out.push_str("    <label class=\"web-form__label\" for=\"");
        escape_into(out, &self.id);
        out.push_str("\">");
        escape_into(out, label);
        if self.required {
            out.push_str(" <span class=\"web-form__required\" aria-hidden=\"true\">*</span>");
        }
        out.push_str("</label>\n");
    }

    fn write_control(&self, out: &mut String) {
        match self.kind {
            FieldKind::Textarea => self.write_textarea(out),
            FieldKind::Select => self.write_select(out),
            FieldKind::Radio if !self.choices.is_empty() => self.write_radio_group(out),
            FieldKind::CheckboxGroup if !self.choices.is_empty() => self.write_checkbox_group(out),
            _ => self.write_input(out),
        }
    }

    /// Attributes shared by every control.
    fn write_common(&self, out: &mut String, id: &str) {
        attr(out, "name", &self.name);
        attr(out, "id", id);
        // A hidden control is barred from browser validation, and on a checkbox
        // group `required` would mean "tick *this* box" rather than "tick one
        // of them" — the group carries `aria-required` instead. The server
        // requires both regardless.
        flag(
            out,
            "required",
            self.required && !matches!(self.kind, FieldKind::Hidden | FieldKind::CheckboxGroup),
        );
        flag(out, "disabled", self.disabled);
        flag(out, "readonly", self.readonly);
        flag(out, "autofocus", self.autofocus);
        attr_opt(out, "autocomplete", self.autocomplete.as_deref());
        attr_opt(out, "class", self.class.as_deref());
        if self.has_errors {
            attr(out, "aria-invalid", "true");
        }
        if let Some(describedby) = self.described_by() {
            attr(out, "aria-describedby", &describedby);
        }
        // Last, so that a custom attribute can never displace one the crate
        // generated: a repeated attribute is the first one, everywhere.
        write_attrs(out, &self.attrs);
    }

    fn write_input(&self, out: &mut String) {
        out.push_str("<input");
        attr_opt(out, "type", self.input_type);
        self.write_common(out, &self.id);
        attr_opt(out, "placeholder", self.placeholder.as_deref());
        attr_opt(out, "pattern", self.pattern.as_deref());
        attr_opt(out, "min", self.min.as_deref());
        attr_opt(out, "max", self.max.as_deref());
        attr_opt(out, "step", self.step.as_deref());
        attr_opt(out, "accept", self.accept.as_deref());
        attr_num(out, "minlength", self.minlength);
        attr_num(out, "maxlength", self.maxlength);
        flag(out, "multiple", self.multiple);

        if matches!(
            self.kind,
            FieldKind::Checkbox | FieldKind::CheckboxGroup | FieldKind::Radio
        ) {
            flag(out, "checked", self.checked);
            // A checkbox with no value submits "on"; both are understood when
            // the value comes back.
            if matches!(self.kind, FieldKind::Radio | FieldKind::CheckboxGroup) {
                attr_opt(out, "value", self.value.as_deref());
            }
        } else if self.kind != FieldKind::File {
            attr_opt(out, "value", self.value.as_deref());
        }
        out.push('>');
    }

    fn write_textarea(&self, out: &mut String) {
        out.push_str("<textarea");
        self.write_common(out, &self.id);
        attr_opt(out, "placeholder", self.placeholder.as_deref());
        attr_num(out, "rows", self.rows);
        attr_num(out, "cols", self.cols);
        attr_num(out, "minlength", self.minlength);
        attr_num(out, "maxlength", self.maxlength);
        out.push('>');
        if let Some(value) = &self.value {
            escape_into(out, value);
        }
        out.push_str("</textarea>");
    }

    fn write_select(&self, out: &mut String) {
        out.push_str("<select");
        self.write_common(out, &self.id);
        flag(out, "multiple", self.multiple);
        out.push_str(">\n");

        // A single-valued, non-required select needs an empty option so the
        // user can express "nothing".
        if !self.multiple && !self.required {
            out.push_str("      <option value=\"\"");
            flag(out, "selected", self.values.is_empty());
            out.push_str("></option>\n");
        }

        let mut open_group: Option<&str> = None;
        for choice in &self.choices {
            let group = choice.group.as_deref();
            if group != open_group {
                if open_group.is_some() {
                    out.push_str("      </optgroup>\n");
                }
                if let Some(label) = group {
                    out.push_str("      <optgroup label=\"");
                    escape_into(out, label);
                    out.push_str("\">\n");
                }
                open_group = group;
            }
            out.push_str("      <option value=\"");
            escape_into(out, &choice.value);
            out.push('"');
            flag(out, "selected", choice.selected);
            flag(out, "disabled", choice.disabled);
            out.push('>');
            escape_into(out, &choice.label);
            out.push_str("</option>\n");
        }
        if open_group.is_some() {
            out.push_str("      </optgroup>\n");
        }
        out.push_str("    </select>");
    }

    fn write_radio_group(&self, out: &mut String) {
        out.push_str("<div class=\"web-form__radios\" role=\"radiogroup\"");
        if self.label.is_some() {
            attr(out, "aria-labelledby", &self.label_id);
        }
        out.push_str(">\n");
        let mut id = ChoiceId::new(&self.id);
        for (index, choice) in self.choices.iter().enumerate() {
            let id = id.nth(index);
            out.push_str("      <label class=\"web-form__radio\"><input type=\"radio\"");
            self.write_common(out, id);
            attr(out, "value", &choice.value);
            flag(out, "checked", choice.selected);
            flag(out, "disabled", choice.disabled);
            out.push('>');
            escape_into(out, &choice.label);
            out.push_str("</label>\n");
        }
        out.push_str("    </div>");
    }

    fn write_checkbox_group(&self, out: &mut String) {
        out.push_str("<div class=\"web-form__checkboxes\" role=\"group\"");
        if self.label.is_some() {
            attr(out, "aria-labelledby", &self.label_id);
        }
        // The browser cannot enforce "at least one" over a checkbox group, so
        // this only announces the requirement; the server enforces it.
        if self.required {
            attr(out, "aria-required", "true");
        }
        out.push_str(">\n");
        let mut id = ChoiceId::new(&self.id);
        for (index, choice) in self.choices.iter().enumerate() {
            let id = id.nth(index);
            out.push_str("      <label class=\"web-form__checkbox\"><input type=\"checkbox\"");
            self.write_common(out, id);
            attr(out, "value", &choice.value);
            flag(out, "checked", choice.selected);
            flag(out, "disabled", choice.disabled);
            out.push('>');
            escape_into(out, &choice.label);
            out.push_str("</label>\n");
        }
        out.push_str("    </div>");
    }
}

impl fmt::Display for FieldView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_html())
    }
}

// ─── HTML writing helpers ─────────────────────────────────────────────────────

fn attr(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    escape_into(out, value);
    out.push('"');
}

fn attr_opt(out: &mut String, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        attr(out, name, value);
    }
}

/// A numeric attribute, written straight into the output rather than through a
/// `String` of its own.
fn attr_num(out: &mut String, name: &str, value: Option<impl fmt::Display>) {
    if let Some(value) = value {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        // Writing to a `String` cannot fail, and a number never needs escaping.
        let _ = write!(out, "{value}");
        out.push('"');
    }
}

/// The `id` of the *n*-th option of a radio or checkbox group: one buffer,
/// rewound for each option, rather than one `String` per option.
struct ChoiceId {
    buffer: String,
    base: usize,
}

impl ChoiceId {
    fn new(id: &str) -> Self {
        let mut buffer = String::with_capacity(id.len() + 4);
        buffer.push_str(id);
        Self {
            base: buffer.len(),
            buffer,
        }
    }

    fn nth(&mut self, index: usize) -> &str {
        self.buffer.truncate(self.base);
        let _ = write!(self.buffer, "-{index}");
        &self.buffer
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

fn write_attrs(out: &mut String, attrs: &[AttrView]) {
    for custom in attrs {
        match &custom.value {
            Some(value) => attr(out, &custom.name, value),
            None => flag(out, &custom.name, true),
        }
    }
}

fn flag(out: &mut String, name: &str, on: bool) {
    if on {
        out.push(' ');
        out.push_str(name);
    }
}

/// The entity `byte` has to be written as, if any. Every character that needs
/// escaping is ASCII, so this can look one byte at a time.
const fn entity(byte: u8) -> Option<&'static str> {
    match byte {
        b'&' => Some("&amp;"),
        b'<' => Some("&lt;"),
        b'>' => Some("&gt;"),
        b'"' => Some("&quot;"),
        b'\'' => Some("&#39;"),
        _ => None,
    }
}

/// Escape text for use in element content or a double-quoted attribute.
///
/// Text with nothing to escape — which is most text — is handed back as it is.
pub fn escape(text: &str) -> Cow<'_, str> {
    match text.bytes().position(|byte| entity(byte).is_some()) {
        None => Cow::Borrowed(text),
        Some(first) => {
            let mut out = String::with_capacity(text.len() + 16);
            out.push_str(&text[..first]);
            escape_into(&mut out, &text[first..]);
            Cow::Owned(out)
        }
    }
}

/// Append `text`, escaped, copying the stretches between entities in one go
/// rather than a character at a time.
fn escape_into(out: &mut String, text: &str) {
    let mut plain = 0;
    for (at, byte) in text.bytes().enumerate() {
        let Some(entity) = entity(byte) else { continue };
        out.push_str(&text[plain..at]);
        out.push_str(entity);
        plain = at + 1;
    }
    out.push_str(&text[plain..]);
}
