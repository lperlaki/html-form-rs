//! The kind of control a field renders as.

use serde::{Deserialize, Serialize};

/// Which HTML control a field becomes.
///
/// Most variants are an `<input type="...">`. [`FieldKind::Textarea`] and
/// [`FieldKind::Select`] are elements of their own.
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
    /// A set of checkboxes that share one name. It is the many-valued twin of
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
    /// The value of the `type` attribute. A `<textarea>` and a `<select>` get
    /// `None`.
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

    /// The tag name this control renders as.
    pub const fn element(self) -> &'static str {
        match self {
            FieldKind::Textarea => "textarea",
            FieldKind::Select => "select",
            _ => "input",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FieldKind::*;
    use super::*;

    /// Every variant, so a new one cannot be added without deciding what it
    /// renders as.
    const ALL: &[FieldKind] = &[
        Text,
        Password,
        Email,
        Url,
        Tel,
        Search,
        Number,
        Range,
        Checkbox,
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
    ];

    #[test]
    fn each_input_names_the_type_attribute_it_renders() {
        for (kind, expected) in [
            (Text, "text"),
            (Password, "password"),
            (Email, "email"),
            (Url, "url"),
            (Tel, "tel"),
            (Search, "search"),
            (Number, "number"),
            (Range, "range"),
            (Radio, "radio"),
            (Date, "date"),
            (Time, "time"),
            (DatetimeLocal, "datetime-local"),
            (Month, "month"),
            (Week, "week"),
            (Color, "color"),
            (File, "file"),
            (Hidden, "hidden"),
        ] {
            assert_eq!(kind.input_type(), Some(expected), "{kind:?}");
            assert_eq!(kind.element(), "input", "{kind:?}");
        }
    }

    /// A group is a set of boxes that share one name, so each box is still an
    /// `<input type="checkbox">`. What differs is how many values come back.
    #[test]
    fn both_checkbox_kinds_render_the_same_input() {
        assert_eq!(Checkbox.input_type(), Some("checkbox"));
        assert_eq!(CheckboxGroup.input_type(), Some("checkbox"));
    }

    /// A `<textarea>` and a `<select>` are elements of their own, so there is
    /// no `type` to give.
    #[test]
    fn the_two_kinds_that_are_not_inputs_have_no_type() {
        assert_eq!(Textarea.input_type(), None);
        assert_eq!(Textarea.element(), "textarea");
        assert_eq!(Select.input_type(), None);
        assert_eq!(Select.element(), "select");
    }

    #[test]
    fn every_kind_answers_both_questions() {
        for kind in ALL {
            assert!(!kind.element().is_empty(), "{kind:?}");
            assert!(kind.input_type().is_none_or(|t| !t.is_empty()), "{kind:?}");
        }
    }

    /// The serialized name is what a template matches on, so it is part of the
    /// crate's surface and not an implementation detail.
    #[test]
    fn a_kind_serialises_in_kebab_case() {
        assert_eq!(
            serde_json::to_string(&DatetimeLocal).unwrap(),
            r#""datetime-local""#
        );
        assert_eq!(
            serde_json::to_string(&CheckboxGroup).unwrap(),
            r#""checkbox-group""#
        );
        assert_eq!(
            serde_json::from_str::<FieldKind>(r#""checkbox-group""#).unwrap(),
            CheckboxGroup
        );
    }
}
