//! The built-in HTML renderer: a [`FormView`] written out as markup.
//!
//! Enabled by the `html` feature, which is on by default. Turning it off drops
//! [`FormView::to_html`], [`FieldView::to_html`], the `Display` impls and
//! [`escape`], and leaves the view as what it always was — a flat, serialisable
//! description a template engine renders. Nothing else in the crate depends on
//! this module: parsing, validation and [`FormView`] itself are unaffected.
//!
//! The markup is plain and unstyled; every element carries a `html-form__*`
//! class to hook CSS onto.

use std::borrow::Cow;
use std::fmt::{self, Write as _};

use crate::kind::FieldKind;
use crate::view::{AttrView, FieldView, FormView};

impl FormView {
    /// Render the complete `<form>` element.
    ///
    /// The markup is plain and unstyled; every element carries a `html-form__*`
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
        out.push_str(" class=\"html-form");
        if let Some(class) = &self.class {
            out.push(' ');
            escape_into(&mut out, class);
        }
        out.push('"');
        flag(&mut out, "novalidate", self.novalidate);
        write_attrs(&mut out, &self.attrs);
        out.push_str(">\n");

        if !self.errors.is_empty() {
            out.push_str("  <ul class=\"html-form__errors\">\n");
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
                    out.push_str("  <fieldset class=\"html-form__group\">\n    <legend>");
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

        out.push_str("  <button type=\"submit\" class=\"html-form__submit\">");
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

        out.push_str("  <div class=\"html-form__field");
        if self.has_errors {
            out.push_str(" html-form__field--invalid");
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
            out.push_str("    <p class=\"html-form__help\" id=\"");
            escape_into(out, &self.help_id);
            out.push_str("\">");
            escape_into(out, help);
            out.push_str("</p>\n");
        }

        if self.has_errors {
            out.push_str("    <ul class=\"html-form__errors\" id=\"");
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
            out.push_str("    <span class=\"html-form__label\" id=\"");
            escape_into(out, &self.label_id);
            out.push_str("\">");
            escape_into(out, label);
            if self.required {
                out.push_str(" <span class=\"html-form__required\" aria-hidden=\"true\">*</span>");
            }
            out.push_str("</span>\n");
            return;
        }
        out.push_str("    <label class=\"html-form__label\" for=\"");
        escape_into(out, &self.id);
        out.push_str("\">");
        escape_into(out, label);
        if self.required {
            out.push_str(" <span class=\"html-form__required\" aria-hidden=\"true\">*</span>");
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
        out.push_str("<div class=\"html-form__radios\" role=\"radiogroup\"");
        if self.label.is_some() {
            attr(out, "aria-labelledby", &self.label_id);
        }
        out.push_str(">\n");
        let mut id = ChoiceId::new(&self.id);
        for (index, choice) in self.choices.iter().enumerate() {
            let id = id.nth(index);
            out.push_str("      <label class=\"html-form__radio\"><input type=\"radio\"");
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
        out.push_str("<div class=\"html-form__checkboxes\" role=\"group\"");
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
            out.push_str("      <label class=\"html-form__checkbox\"><input type=\"checkbox\"");
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
