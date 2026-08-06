//! Converting between Rust values and the strings a browser submits.

use std::borrow::Cow;

use crate::error::{FieldError, ValueError};
use crate::spec::{Bounds, Control, NumberControl};

/// A type that can appear as the value of a single form control.
///
/// Implement it for your own scalars to use them as form fields; `Option<T>`,
/// `Vec<T>` and `bool` are handled by the derive macro and need no impl.
///
/// Everything a field of this type would otherwise have to repeat lives here,
/// so that a form declaring one says only where it goes:
///
/// * [`CONTROL`](FormValue::CONTROL) — the control it renders as, carrying the
///   constraints the type itself implies, which is why an integer cannot
///   promise a `step` and a text format at the same time;
/// * [`DEFAULT`](FormValue::DEFAULT) — what a blank form shows for it;
/// * [`validate_form_value`](FormValue::validate_form_value) — the check the
///   type makes of every value of it, wherever it is used.
///
/// A field may still override the first two; the check is the type's own and
/// always runs.
///
/// ```
/// use std::borrow::Cow;
/// use web_form::{Control, FormValue, ValueError};
///
/// struct Slug(String);
///
/// impl FormValue for Slug {
///     const CONTROL: Control = Control::TEXT;
///
///     fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
///         if raw.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
///             Ok(Slug(raw.to_owned()))
///         } else {
///             Err(ValueError::new("lowercase letters and dashes"))
///         }
///     }
///
///     fn to_form_value(&self) -> Cow<'_, str> {
///         Cow::Borrowed(&self.0)
///     }
/// }
/// ```
///
/// A type that wraps one that is already a `FormValue` needs none of it written
/// out — `#[derive(FormValue)]` writes the conversion, and `#[value(...)]` says
/// what the wrapper adds:
///
/// ```
/// use web_form::{FormValue, WebForm};
///
/// #[derive(FormValue)]
/// #[value(type = "email", maxlength = 254, validate = is_company_address)]
/// struct WorkEmail(String);
///
/// fn is_company_address(email: &WorkEmail) -> bool {
///     email.0.ends_with("@example.com")
/// }
///
/// #[derive(WebForm)]
/// struct Invite {
///     // Nothing here repeats what the type already knows.
///     colleague: WorkEmail,
/// }
///
/// assert!(Invite::render().to_html().contains(r#"type="email""#));
/// assert!(Invite::from_urlencoded("colleague=ada@example.com").is_ok());
/// assert!(Invite::from_urlencoded("colleague=ada@example.org").is_err());
/// ```
pub trait FormValue: Sized {
    /// The control rendered for this type unless overridden, together with the
    /// constraints the type itself implies.
    const CONTROL: Control;

    /// What a field of this type shows on a blank form, and falls back to when
    /// the name is absent from a submission entirely.
    ///
    /// `#[field(default = "…")]` overrides it, exactly as it overrides
    /// [`CONTROL`](FormValue::CONTROL); the rules are the ones in
    /// [`FieldSpec::default`](crate::FieldSpec::default), since this *is* that
    /// default once the spec is built.
    const DEFAULT: Option<&'static str> = None;

    /// Convert one submitted string into the Rust value.
    fn parse_form_value(raw: &str) -> Result<Self, ValueError>;

    /// Render the value back into the string form a browser would submit, so
    /// an existing record can be shown in the form it came from.
    fn to_form_value(&self) -> Cow<'_, str>;

    /// The check the *type* makes of a value, beyond deciding whether the
    /// submitted string could be converted at all.
    ///
    /// Separate from [`parse_form_value`](FormValue::parse_form_value) because
    /// the two answer different questions and say so differently: a conversion
    /// that fails can only report what it *expected*, where a check that fails
    /// returns a whole [`FieldError`] — a message, an i18n key, a
    /// [code](crate::ErrorKind::Custom) a caller can match on. Splitting them
    /// is also what lets a type be converted where no form is involved and
    /// checked where one is.
    ///
    /// It is run once per submitted value, after the conversion and after the
    /// constraints in the spec, and its error joins the rest of the pass rather
    /// than ending it. A blank value for a field that is not required never
    /// reaches it: not filling something in is not something a type gets a say
    /// about.
    ///
    /// Nothing is handed to it but the value — a `FormValue` belongs to no
    /// form, so there is no context to reach. A check that needs one belongs on
    /// the field, as `#[field(validate = ...)]`.
    fn validate_form_value(&self) -> Result<(), FieldError> {
        Ok(())
    }
}

impl FormValue for String {
    const CONTROL: Control = Control::TEXT;

    fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
        Ok(raw.to_owned())
    }

    fn to_form_value(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

impl FormValue for char {
    const CONTROL: Control = Control::TEXT;

    fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
        let mut chars = raw.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Ok(c),
            _ => Err(ValueError::new("a single character")),
        }
    }

    fn to_form_value(&self) -> Cow<'_, str> {
        Cow::Owned(self.to_string())
    }
}

impl FormValue for bool {
    const CONTROL: Control = Control::Checkbox;

    /// Accepts everything a browser or an API client is likely to send. An
    /// empty value counts as *checked*, because that is what a checkbox with no
    /// `value` attribute submits in some clients.
    fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "on" | "true" | "1" | "yes" | "y" | "checked" => Ok(true),
            "off" | "false" | "0" | "no" | "n" => Ok(false),
            _ => Err(ValueError::new("a yes/no value")),
        }
    }

    fn to_form_value(&self) -> Cow<'_, str> {
        Cow::Borrowed(if *self { "true" } else { "false" })
    }
}

macro_rules! impl_int {
    ($($t:ty => $min:expr),* $(,)?) => {$(
        impl FormValue for $t {
            const CONTROL: Control = Control::Number(NumberControl {
                bounds: Bounds { min: $min, step: Some("1"), max: None },
                ..NumberControl::DEFAULT
            });

            fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
                let trimmed = raw.trim();
                trimmed.parse::<$t>().map_err(|_| {
                    // Distinguish "not a number at all" from "out of range",
                    // which is a much more useful message to show a user.
                    if trimmed.parse::<i128>().is_ok() || trimmed.parse::<u128>().is_ok() {
                        ValueError::new(format!(
                            "a whole number between {} and {}",
                            <$t>::MIN,
                            <$t>::MAX
                        ))
                    } else {
                        ValueError::new("a whole number")
                    }
                })
            }

            fn to_form_value(&self) -> Cow<'_, str> {
                Cow::Owned(self.to_string())
            }
        }
    )*};
}

impl_int! {
    i8 => None,
    i16 => None,
    i32 => None,
    i64 => None,
    i128 => None,
    isize => None,
    u8 => Some("0"),
    u16 => Some("0"),
    u32 => Some("0"),
    u64 => Some("0"),
    u128 => Some("0"),
    usize => Some("0"),
}

macro_rules! impl_float {
    ($($t:ty),* $(,)?) => {$(
        impl FormValue for $t {
            const CONTROL: Control = Control::Number(NumberControl {
                bounds: Bounds { step: Some("any"), min: None, max: None },
                ..NumberControl::DEFAULT
            });

            fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
                let value: $t = raw
                    .trim()
                    .parse()
                    .map_err(|_| ValueError::new("a number"))?;
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(ValueError::new("a finite number"))
                }
            }

            fn to_form_value(&self) -> Cow<'_, str> {
                Cow::Owned(self.to_string())
            }
        }
    )*};
}

impl_float!(f32, f64);
