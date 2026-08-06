//! The kind of control a field is rendered as.

use serde::{Deserialize, Serialize};

/// Which HTML control a field maps to.
///
/// Most variants are `<input type="...">`; [`FieldKind::Textarea`] and
/// [`FieldKind::Select`] are their own elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FieldKind {
    Text,
    Password,
    Email,
    Url,
    Tel,
    Search,
    Number,
    Range,
    Checkbox,
    /// A set of checkboxes sharing one name — the multi-valued counterpart of
    /// [`FieldKind::Radio`].
    CheckboxGroup,
    Radio,
    Date,
    Time,
    DatetimeLocal,
    Month,
    Week,
    Color,
    File,
    Hidden,
    Textarea,
    Select,
}

impl FieldKind {
    /// The value of the `type` attribute, or `None` for `<textarea>` / `<select>`.
    pub const fn input_type(self) -> Option<&'static str> {
        use FieldKind::*;
        Some(match self {
            Text => "text",
            Password => "password",
            Email => "email",
            Url => "url",
            Tel => "tel",
            Search => "search",
            Number => "number",
            Range => "range",
            Checkbox | CheckboxGroup => "checkbox",
            Radio => "radio",
            Date => "date",
            Time => "time",
            DatetimeLocal => "datetime-local",
            Month => "month",
            Week => "week",
            Color => "color",
            File => "file",
            Hidden => "hidden",
            Textarea | Select => return None,
        })
    }

    /// The tag name used to render this control.
    pub const fn element(self) -> &'static str {
        match self {
            FieldKind::Textarea => "textarea",
            FieldKind::Select => "select",
            _ => "input",
        }
    }
}
