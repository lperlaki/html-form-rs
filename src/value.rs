//! How the crate converts between Rust values and the strings a browser
//! submits.

use std::borrow::Cow;

use crate::error::{FieldError, ValueError};
use crate::spec::{Bounds, Control, NumberControl};

/// A type that can appear as the value of a single form control.
///
/// Implement it for your own scalar types to use them as form fields. The
/// derive macro handles `Option<T>`, `Vec<T>` and `bool`, which need no impl.
///
/// It holds everything a field of this type would otherwise repeat, so a form
/// that declares one says only where it goes:
///
/// * [`CONTROL`](FormValue::CONTROL) — the control it renders as, with the
///   constraints the type itself implies. That is why an integer cannot promise
///   a `step` and a text format at the same time.
/// * [`DEFAULT`](FormValue::DEFAULT) — what a blank form shows for it.
/// * [`validate_form_value`](FormValue::validate_form_value) — the check the
///   type makes of every one of its values, wherever a form uses it.
///
/// A field may still override the first two. The check belongs to the type and
/// always runs.
///
/// ```
/// use std::borrow::Cow;
/// use html_form::{Control, FormValue, ValueError};
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
/// A type that wraps a `FormValue` needs none of this written out.
/// `#[derive(FormValue)]` writes the conversion, and `#[value(...)]` says what
/// the wrapper adds:
///
/// ```
/// use html_form::{Form, FormValue};
///
/// #[derive(FormValue)]
/// #[value(type = "email", maxlength = 254, validate = is_company_address)]
/// struct WorkEmail(String);
///
/// fn is_company_address(email: &WorkEmail) -> bool {
///     email.0.ends_with("@example.com")
/// }
///
/// #[derive(Form)]
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
    /// The control this type renders as, unless a field overrides it, with the
    /// constraints the type itself implies.
    const CONTROL: Control;

    /// What a field of this type shows on a blank form. It also applies when
    /// the submission leaves the name out.
    ///
    /// `#[field(default = "…")]` overrides it, exactly as it overrides
    /// [`CONTROL`](FormValue::CONTROL). The rules are the ones in
    /// [`FieldSpec::default`](crate::FieldSpec::default), because this *is*
    /// that default once the crate builds the spec.
    const DEFAULT: Option<&'static str> = None;

    /// Convert one submitted string into the Rust value.
    fn parse_form_value(raw: &str) -> Result<Self, ValueError>;

    /// Render the value back into the string a browser would submit, so you can
    /// show an existing record in the form it came from.
    fn to_form_value(&self) -> Cow<'_, str>;

    /// The check the *type* makes of a value, after it decides whether the
    /// submitted string converts at all.
    ///
    /// It stays apart from [`parse_form_value`](FormValue::parse_form_value)
    /// because the two answer different questions and say so differently. A
    /// failed conversion can report only what it *expected*. A failed check
    /// returns a whole [`FieldError`]: a message, an i18n key, and a
    /// [code](crate::ErrorKind::Custom) a caller can match on. The split also
    /// lets a caller convert a type where no form is involved, and check it
    /// where one is.
    ///
    /// The crate runs it once per submitted value, after the conversion and
    /// after the constraints in the spec. Its error joins the rest of the pass
    /// in place of ending it. A blank value for a field that is not required
    /// never reaches it. An empty answer is not something a type gets a say
    /// about.
    ///
    /// It receives nothing but the value. A `FormValue` belongs to no form, so
    /// there is no context to reach. A check that needs one belongs on the
    /// field, as `#[field(validate = ...)]`.
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

    /// This accepts everything a browser or an API client is likely to send. An
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
                    // Tell "not a number at all" apart from "out of range".
                    // The second is a far more useful message for a user.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::NumberFormat;

    fn expected<T: FormValue>(raw: &str) -> String {
        T::parse_form_value(raw)
            .err()
            .expect("the value was supposed to be rejected")
            .expected
            .into_owned()
    }

    #[test]
    fn a_string_takes_whatever_arrived_and_gives_it_straight_back() {
        assert_eq!(
            String::parse_form_value("  padded  "),
            Ok("  padded  ".to_owned())
        );
        assert_eq!(String::parse_form_value(""), Ok(String::new()));
        assert_eq!(String::from("ada").to_form_value(), "ada");
        // Borrowed, because a string is already what a submission carries.
        assert!(matches!(
            String::from("ada").to_form_value(),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn a_char_is_exactly_one_of_them() {
        assert_eq!(char::parse_form_value("x"), Ok('x'));
        assert_eq!(
            char::parse_form_value("\u{e9}"),
            Ok('\u{e9}'),
            "one character, two bytes"
        );
        assert_eq!(
            char::parse_form_value("\u{1f600}"),
            Ok('\u{1f600}'),
            "or four"
        );

        assert_eq!(expected::<char>(""), "a single character");
        assert_eq!(expected::<char>("xy"), "a single character");
        assert_eq!('x'.to_form_value(), "x");
    }

    /// A checkbox arrives written in whatever way the client that sent it
    /// writes one, so the conversion accepts every spelling in circulation.
    #[test]
    fn a_checkbox_is_read_from_every_spelling_a_client_sends() {
        for raw in ["", "on", "true", "1", "yes", "y", "checked", " ON ", "TRUE"] {
            assert_eq!(bool::parse_form_value(raw), Ok(true), "{raw:?}");
        }
        for raw in ["off", "false", "0", "no", "n", " OFF ", "False"] {
            assert_eq!(bool::parse_form_value(raw), Ok(false), "{raw:?}");
        }
        assert_eq!(expected::<bool>("maybe"), "a yes/no value");
    }

    /// An empty value counts as *checked*, because that is what a checkbox with
    /// no `value` attribute submits in some clients.
    #[test]
    fn an_empty_checkbox_value_still_means_checked() {
        assert_eq!(bool::parse_form_value(""), Ok(true));
    }

    #[test]
    fn a_bool_writes_itself_back_as_a_word() {
        assert_eq!(true.to_form_value(), "true");
        assert_eq!(false.to_form_value(), "false");
    }

    #[test]
    fn an_integer_drops_the_whitespace_around_it() {
        assert_eq!(u32::parse_form_value("  36  "), Ok(36));
        assert_eq!(i64::parse_form_value("-1"), Ok(-1));
        assert_eq!(36u32.to_form_value(), "36");
    }

    /// "Not a number at all" and "out of range" are different mistakes, and
    /// only one of them is worth telling a user the limits for.
    #[test]
    fn an_integer_out_of_range_says_so_rather_than_just_not_a_number() {
        assert_eq!(
            expected::<u8>("300"),
            "a whole number between 0 and 255",
            "over the top"
        );
        assert_eq!(
            expected::<u8>("-1"),
            "a whole number between 0 and 255",
            "under the bottom, which for an unsigned type is any negative"
        );
        assert_eq!(
            expected::<i8>("-200"),
            "a whole number between -128 and 127"
        );
        assert_eq!(expected::<u8>("abc"), "a whole number");
        assert_eq!(
            expected::<u8>("1.5"),
            "a whole number",
            "nor is it a whole one"
        );
        assert_eq!(expected::<u8>(""), "a whole number");
    }

    /// The `i128`/`u128` fallback covers a value too big for *either* of them,
    /// which is no longer a number that was merely out of range.
    #[test]
    fn a_number_past_every_integer_type_is_reported_as_no_number_at_all() {
        let enormous = "1".repeat(64);
        assert_eq!(expected::<u64>(&enormous), "a whole number");
        // But one that still fits an i128 keeps the more useful message.
        assert_eq!(
            expected::<u64>("-9223372036854775808"),
            "a whole number between 0 and 18446744073709551615"
        );
    }

    #[test]
    fn every_integer_width_carries_the_bounds_its_type_implies() {
        assert!(matches!(
            <u32 as FormValue>::CONTROL,
            Control::Number(NumberControl {
                bounds: Bounds {
                    min: Some("0"),
                    step: Some("1"),
                    max: None
                },
                format: NumberFormat::Number,
            })
        ));
        // A signed type has no floor to promise.
        assert!(matches!(
            <i32 as FormValue>::CONTROL,
            Control::Number(NumberControl {
                bounds: Bounds {
                    min: None,
                    step: Some("1"),
                    ..
                },
                ..
            })
        ));
        assert_eq!(<usize as FormValue>::DEFAULT, None);
    }

    #[test]
    fn a_float_takes_a_fraction_and_says_so_in_its_step() {
        assert_eq!(f64::parse_form_value(" 1.5 "), Ok(1.5));
        assert_eq!(f32::parse_form_value("-0.25"), Ok(-0.25));
        assert_eq!(1.5f64.to_form_value(), "1.5");

        assert!(matches!(
            <f64 as FormValue>::CONTROL,
            Control::Number(NumberControl {
                bounds: Bounds {
                    step: Some("any"),
                    min: None,
                    max: None
                },
                ..
            })
        ));
    }

    /// A form field holds a quantity. An infinity or a NaN is neither one a
    /// user can have meant nor one a caller can do arithmetic with.
    #[test]
    fn a_float_that_is_not_finite_is_not_a_quantity() {
        assert_eq!(expected::<f64>("inf"), "a finite number");
        assert_eq!(expected::<f64>("-inf"), "a finite number");
        assert_eq!(expected::<f64>("NaN"), "a finite number");
        assert_eq!(expected::<f32>("infinity"), "a finite number");
        // And what will not parse at all is simply not a number.
        assert_eq!(expected::<f64>("abc"), "a number");
        assert_eq!(expected::<f32>(""), "a number");
    }

    /// The check is the type's own business, and a type that makes none passes
    /// everything the conversion accepted.
    #[test]
    fn the_built_in_types_check_nothing_beyond_converting() {
        assert!(String::new().validate_form_value().is_ok());
        assert!(0u8.validate_form_value().is_ok());
        assert!(f64::MAX.validate_form_value().is_ok());
        assert!(true.validate_form_value().is_ok());
        assert!('x'.validate_form_value().is_ok());
    }
}
