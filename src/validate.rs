//! The server-side re-check of the HTML validation attributes, and the shapes a
//! `validate = ...` function may return.
//!
//! This module checks again everything the markup asks the browser to enforce,
//! because a submission can come from anywhere. What the markup *cannot* ask
//! for is a `validate = ...` function. Its return value reaches the parse
//! through [`FieldValidation`] or [`FormValidation`].

use crate::context::{WithContext, WithoutContext};
use crate::error::{ErrorKind, FieldError, FormErrors};
use crate::spec::{Bounds, Control, FieldSpec, TemporalFormat, TextFormat};

/// Check one submitted value that is not blank against a field's constraints.
///
/// It returns every failure, not the first alone. The [`Control`] decides which
/// checks exist at all. No arm gives a `<select>` a `pattern` or a date a
/// `minlength`, because the spec cannot hold those.
pub fn check(spec: &FieldSpec, raw: &str) -> Vec<FieldError> {
    let mut errors = Vec::new();

    match &spec.control {
        Control::Text(text) => {
            // The control's own type is a constraint too. `type="email"` is a
            // promise the browser keeps, and a submission from anywhere else
            // need not keep it. A value of the wrong shape makes the finer
            // checks pointless. Something that is not an address will usually
            // also fail the `pattern` that narrows which addresses count.
            if let Some(expected) = text.format.check(raw) {
                return vec![invalid(expected)];
            }
            check_pattern(text.pattern, raw, &mut errors);
            check_length(text.minlength, text.maxlength, raw, &mut errors);
        }
        Control::Textarea(area) => {
            check_length(area.minlength, area.maxlength, raw, &mut errors);
        }
        Control::Number(number) => check_numeric(&number.bounds, raw, &mut errors),
        Control::Temporal(temporal) => {
            if !temporal.format.matches(raw) {
                return vec![invalid(temporal.format.expected())];
            }
            check_chronological(&temporal.bounds, raw, &mut errors);
        }
        Control::Choose(choose) => {
            // An empty list means the options arrive at render time, so there
            // is nothing to check the value against.
            if !choose.choices.is_empty() && !choose.choices.iter().any(|c| c.value == raw) {
                errors.push(FieldError::new(ErrorKind::NotAChoice));
            }
        }
        Control::Color => {
            if !format::is_color(raw) {
                return vec![invalid("a color as #rrggbb")];
            }
        }
        // The `FormValue` impl of `bool` checks a checkbox value. The bytes of
        // a file never reach this crate. A hidden field carries whatever the
        // form put there.
        Control::Checkbox | Control::File(_) | Control::Hidden => {}
    }

    errors
}

fn invalid(expected: &'static str) -> FieldError {
    FieldError::new(ErrorKind::Invalid {
        expected: expected.into(),
    })
}

fn check_pattern(pattern: Option<&'static str>, raw: &str, errors: &mut Vec<FieldError>) {
    if let Some(pattern) = pattern
        && !matches_pattern(pattern, raw)
    {
        errors.push(FieldError::new(ErrorKind::Pattern {
            pattern: pattern.into(),
        }));
    }
}

/// HTML counts length in UTF-16 code units. A count of scalar values is close
/// enough, and it never surprises a caller who writes plain text.
fn check_length(
    minlength: Option<usize>,
    maxlength: Option<usize>,
    raw: &str,
    errors: &mut Vec<FieldError>,
) {
    let length = raw.chars().count();
    if let Some(minlength) = minlength
        && length < minlength
    {
        errors.push(FieldError::new(ErrorKind::TooShort { minlength, length }));
    }
    if let Some(maxlength) = maxlength
        && length > maxlength
    {
        errors.push(FieldError::new(ErrorKind::TooLong { maxlength, length }));
    }
}

/// `min`, `max` and `step` for `number` and `range`, compared as numbers.
///
/// This leaves a value that is not a number alone. The field's Rust type
/// rejects it with a better message than any bound could give.
fn check_numeric(bounds: &Bounds, raw: &str, errors: &mut Vec<FieldError>) {
    if bounds.is_empty() {
        return;
    }
    let Ok(value) = raw.trim().parse::<f64>() else {
        return;
    };

    let min = bounds.min.and_then(|s| s.parse::<f64>().ok());
    let max = bounds.max.and_then(|s| s.parse::<f64>().ok());

    if let (Some(min), Some(text)) = (min, bounds.min)
        && value < min
    {
        errors.push(FieldError::new(ErrorKind::TooSmall { min: text.into() }));
    }
    if let (Some(max), Some(text)) = (max, bounds.max)
        && value > max
    {
        errors.push(FieldError::new(ErrorKind::TooLarge { max: text.into() }));
    }
    if let Some(text) = bounds.step
        && let Some(step) = parse_step(text)
        && !on_step(value, min.unwrap_or(0.0), step)
    {
        errors.push(FieldError::new(ErrorKind::Step { step: text.into() }));
    }
}

/// `min` and `max` for the date and time controls.
///
/// The crate has already checked the value against its format. For every format
/// HTML uses, string order *is* the order in time, so a string comparison is
/// the whole job. The crate renders `step` for the browser, but `step` means
/// nothing on the server without a calendar.
fn check_chronological(bounds: &Bounds, raw: &str, errors: &mut Vec<FieldError>) {
    if let Some(min) = bounds.min
        && raw < min
    {
        errors.push(FieldError::new(ErrorKind::TooSmall { min: min.into() }));
    }
    if let Some(max) = bounds.max
        && raw > max
    {
        errors.push(FieldError::new(ErrorKind::TooLarge { max: max.into() }));
    }
}

/// `step="any"`, and a step of zero or less, turn the check off.
fn parse_step(step: &str) -> Option<f64> {
    if step.eq_ignore_ascii_case("any") {
        return None;
    }
    match step.parse::<f64>() {
        Ok(s) if s.is_finite() && s > 0.0 => Some(s),
        _ => None,
    }
}

fn on_step(value: f64, base: f64, step: f64) -> bool {
    let steps = (value - base) / step;
    let nearest = steps.round();
    // A relative tolerance: 0.1 + 0.2 must still sit on a 0.1 grid.
    (steps - nearest).abs() <= 1e-9 * steps.abs().max(1.0)
}

/// Whether the whole value matches an HTML `pattern`.
///
/// Without the `pattern` feature, the crate still renders the attribute, so the
/// browser still enforces it. This module then checks nothing.
#[cfg(feature = "pattern")]
fn matches_pattern(pattern: &str, value: &str) -> bool {
    match compiled(pattern) {
        // A pattern that does not compile must not reject everything the user
        // types. The browser is then the only place that enforces it.
        None => true,
        Some(re) => re.is_match(value),
    }
}

#[cfg(not(feature = "pattern"))]
fn matches_pattern(_pattern: &str, _value: &str) -> bool {
    true
}

#[cfg(feature = "pattern")]
fn compiled(pattern: &str) -> Option<std::sync::Arc<regex_lite::Regex>> {
    use std::collections::HashMap;
    use std::sync::{Arc, OnceLock, RwLock};

    type Cache = RwLock<HashMap<String, Option<Arc<regex_lite::Regex>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Cache::default);

    if let Some(hit) = cache.read().ok().and_then(|c| c.get(pattern).cloned()) {
        return hit;
    }

    // HTML anchors `pattern` at both ends and matches the whole value.
    let anchored = format!("^(?:{pattern})$");
    let compiled = regex_lite::Regex::new(&anchored).ok().map(Arc::new);
    if let Ok(mut cache) = cache.write() {
        cache.insert(pattern.to_owned(), compiled.clone());
    }
    compiled
}

impl TextFormat {
    /// What this format expected, if `raw` does not have that shape.
    ///
    /// `Text`, `Password`, `Tel` and `Search` accept anything. HTML asks the
    /// browser for nothing more, so the server asks for nothing more either.
    fn check(self, raw: &str) -> Option<&'static str> {
        match self {
            TextFormat::Text | TextFormat::Password | TextFormat::Tel | TextFormat::Search => None,
            TextFormat::Url => {
                (!format::is_url(raw)).then_some("a full URL, for example https://example.com")
            }
            // `multiple` accepts a comma-separated list.
            TextFormat::Email { multiple: true } => {
                (!raw.split(',').all(|one| format::is_email(one.trim())))
                    .then_some("a comma-separated list of email addresses")
            }
            TextFormat::Email { multiple: false } => {
                (!format::is_email(raw)).then_some("a valid email address")
            }
        }
    }
}

impl TemporalFormat {
    fn matches(self, raw: &str) -> bool {
        match self {
            TemporalFormat::Date => format::is_date(raw),
            TemporalFormat::Time => format::is_time(raw),
            TemporalFormat::DatetimeLocal => format::is_datetime_local(raw),
            TemporalFormat::Month => format::is_month(raw),
            TemporalFormat::Week => format::is_week(raw),
        }
    }

    fn expected(self) -> &'static str {
        match self {
            TemporalFormat::Date => "a date as YYYY-MM-DD",
            TemporalFormat::Time => "a time as HH:MM",
            TemporalFormat::DatetimeLocal => "a date and time as YYYY-MM-DDTHH:MM",
            TemporalFormat::Month => "a month as YYYY-MM",
            TemporalFormat::Week => "a week as YYYY-Www",
        }
    }
}

/// The format checks a control's type implies.
///
/// These use no dependency, and they are no stricter than the HTML
/// specification. The aim is to reject what a browser would refuse to submit,
/// not to decide whether an address can receive mail.
///
/// The numeric controls are missing on purpose. The field's Rust type already
/// rejects what it cannot hold, and it says more than a general "that is not a
/// number".
mod format {
    /// The HTML "valid email address" production, which accepts more than
    /// RFC 5322 does, on purpose.
    pub fn is_email(value: &str) -> bool {
        const LOCAL_PUNCTUATION: &str = ".!#$%&'*+/=?^_`{|}~-";
        let Some((local, domain)) = value.split_once('@') else {
            return false;
        };
        if local.is_empty() || domain.is_empty() {
            return false;
        }
        let local_ok = local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || LOCAL_PUNCTUATION.contains(c));
        // A second `@` lands in the domain, where it is not a legal label
        // character. The label check below therefore rejects it.
        local_ok && domain.split('.').all(is_domain_label)
    }

    fn is_domain_label(label: &str) -> bool {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }

    /// An absolute URL: a scheme, a colon, and something after it.
    pub fn is_url(value: &str) -> bool {
        let Some((scheme, rest)) = value.split_once(':') else {
            return false;
        };
        !rest.is_empty()
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            && !value.chars().any(char::is_whitespace)
    }

    /// Exactly `width` ASCII digits.
    fn digits(text: &str, width: usize) -> Option<u32> {
        (text.len() == width && text.bytes().all(|b| b.is_ascii_digit()))
            .then(|| text.parse().ok())
            .flatten()
    }

    fn is_leap(year: u32) -> bool {
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
    }

    fn days_in_month(year: u32, month: u32) -> u32 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if is_leap(year) => 29,
            2 => 28,
            _ => 0,
        }
    }

    fn parse_month(value: &str) -> Option<(u32, u32)> {
        let (year, month) = value.split_once('-')?;
        let year = digits(year, 4)?;
        let month = digits(month, 2)?;
        (1..=12).contains(&month).then_some((year, month))
    }

    pub fn is_month(value: &str) -> bool {
        parse_month(value).is_some()
    }

    pub fn is_date(value: &str) -> bool {
        let Some((month_part, day)) = value.rsplit_once('-') else {
            return false;
        };
        let Some((year, month)) = parse_month(month_part) else {
            return false;
        };
        match digits(day, 2) {
            // A calendar check, so this rejects 2026-02-31 as a browser does.
            Some(day) => day >= 1 && day <= days_in_month(year, month),
            None => false,
        }
    }

    pub fn is_time(value: &str) -> bool {
        let mut parts = value.split(':');
        let (Some(hour), Some(minute)) = (parts.next(), parts.next()) else {
            return false;
        };
        let (Some(hour), Some(minute)) = (digits(hour, 2), digits(minute, 2)) else {
            return false;
        };
        if hour > 23 || minute > 59 {
            return false;
        }
        match parts.next() {
            None => true,
            Some(seconds) => {
                if parts.next().is_some() {
                    return false;
                }
                // Seconds may carry a fraction of one to three digits.
                let (whole, fraction) = match seconds.split_once('.') {
                    Some((whole, fraction)) => (whole, Some(fraction)),
                    None => (seconds, None),
                };
                let valid_fraction = match fraction {
                    None => true,
                    Some(fraction) => {
                        (1..=3).contains(&fraction.len())
                            && fraction.bytes().all(|b| b.is_ascii_digit())
                    }
                };
                matches!(digits(whole, 2), Some(seconds) if seconds <= 59) && valid_fraction
            }
        }
    }

    pub fn is_datetime_local(value: &str) -> bool {
        // The HTML specification allows a space in place of the `T`.
        let Some((date, time)) = value.split_once(['T', ' ']) else {
            return false;
        };
        is_date(date) && is_time(time)
    }

    pub fn is_week(value: &str) -> bool {
        let Some((year, week)) = value.split_once("-W") else {
            return false;
        };
        let Some(year) = digits(year, 4) else {
            return false;
        };
        match digits(week, 2) {
            Some(week) => week >= 1 && week <= weeks_in_year(year),
            None => false,
        }
    }

    /// An ISO 8601 long year has 53 weeks. It starts on a Thursday, or on a
    /// Wednesday in a leap year.
    fn weeks_in_year(year: u32) -> u32 {
        let jan_1 = day_of_week(year, 1, 1);
        if jan_1 == 4 || (is_leap(year) && jan_1 == 3) {
            53
        } else {
            52
        }
    }

    /// The day of the week, Zeller style, where 0 is Sunday.
    fn day_of_week(year: u32, month: u32, day: u32) -> u32 {
        const OFFSETS: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let year = if month < 3 { year - 1 } else { year };
        (year + year / 4 - year / 100 + year / 400 + OFFSETS[(month - 1) as usize] + day) % 7
    }

    pub fn is_color(value: &str) -> bool {
        let Some(hex) = value.strip_prefix('#') else {
            return false;
        };
        hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

/// The error for a required field that arrived missing or blank.
pub fn required_error(_spec: &FieldSpec) -> FieldError {
    FieldError::new(ErrorKind::Required)
}

// ─── What a `validate = ...` function may return ──────────────────────────────

/// What a `#[field(validate = ...)]` function may return.
///
/// Whichever shape the function has, the check runs against the field's own
/// Rust type, `Option<T>` and `Vec<T>` included. It runs only after the value
/// passes the constraints in the spec.
///
/// | Return type | Rejects when | Message |
/// |---|---|---|
/// | `bool` | `false` | the built-in one for [`ErrorKind::Custom`] |
/// | `Result<(), &'static str \| String \| Cow>` | `Err` | the text returned |
/// | `Result<(), Text>` | `Err` | the text, or the i18n key, returned |
/// | `Result<(), FieldError>` | `Err` | whatever the error carries |
///
/// ```
/// use html_form::{Form, Text};
///
/// #[derive(Form, Debug)]
/// struct Signup {
///     // The shortest form: a predicate, with the built-in message.
///     #[field(validate = is_even)]
///     seats: u32,
///     // A key, which the error carries as its message *and* as its code.
///     #[field(validate = not_reserved)]
///     username: String,
/// }
///
/// fn is_even(seats: &u32) -> bool {
///     seats % 2 == 0
/// }
///
/// fn not_reserved(name: &String) -> Result<(), Text> {
///     match name.as_str() {
///         "admin" => Err(Text::key("signup.username.reserved")),
///         _ => Ok(()),
///     }
/// }
///
/// let errors = Signup::from_urlencoded("seats=3&username=admin").unwrap_err();
/// assert_eq!(errors.field("seats").next().unwrap().message.as_str(), "This value is not valid.");
/// assert_eq!(errors.field("username").next().unwrap().code(), Some("signup.username.reserved"));
/// ```
pub trait FieldValidation {
    /// The error to put on the field, or `None` when the value passed.
    fn into_field_error(self) -> Option<FieldError>;
}

impl FieldValidation for bool {
    /// A predicate says only whether the value is acceptable. A rejection
    /// therefore carries the built-in message and no code.
    fn into_field_error(self) -> Option<FieldError> {
        (!self).then(|| FieldError::new(ErrorKind::Custom { code: None }))
    }
}

impl<E: Into<FieldError>> FieldValidation for Result<(), E> {
    fn into_field_error(self) -> Option<FieldError> {
        self.err().map(Into::into)
    }
}

/// What a `#[form(validate = ...)]` function may return.
///
/// It takes the same shapes as [`FieldValidation`], plus the ones that name a
/// field: a `(field, message)` pair, or a whole [`FormErrors`] the function
/// built itself. A message that names no field belongs to the whole form.
///
/// ```
/// use html_form::{Form, FormErrors};
///
/// #[derive(Form, Debug)]
/// #[form(validate = passwords_match)]
/// struct Signup {
///     password: String,
///     confirm: String,
/// }
///
/// fn passwords_match(form: &Signup) -> Result<(), FormErrors> {
///     if form.password == form.confirm {
///         Ok(())
///     } else {
///         // Attach it to the field the user can correct, not to the form.
///         Err(("confirm", "The two passwords do not match.").into())
///     }
/// }
///
/// let errors = Signup::from_urlencoded("password=a&confirm=b").unwrap_err();
/// assert!(errors.has_field("confirm"));
/// ```
pub trait FormValidation {
    /// The errors to add to the parse, or `None` when the form passed.
    fn into_form_errors(self) -> Option<FormErrors>;
}

impl FormValidation for bool {
    fn into_form_errors(self) -> Option<FormErrors> {
        (!self).then(|| FieldError::new(ErrorKind::Custom { code: None }).into())
    }
}

impl<E: Into<FormErrors>> FormValidation for Result<(), E> {
    fn into_form_errors(self) -> Option<FormErrors> {
        self.err().map(Into::into)
    }
}

// ─── What a `validate = ...` function may take ────────────────────────────────

/// What `#[field(validate = ...)]` may name.
///
/// [`FieldValidation`] says what a validator may *return*. This says what it
/// may *take*. Either arity works, and the crate reads which one you wrote off
/// the function itself:
///
/// | Signature | The crate calls it with |
/// |---|---|
/// | `fn(&T) -> impl FieldValidation` | the field's value |
/// | `fn(&T, &Context) -> impl FieldValidation` | the value and the context |
///
/// `T` is the field's own Rust type, `Option<T>` and `Vec<T>` included. A
/// validator can therefore check whether a field is empty, and how many values
/// it holds, as well as the value itself.
///
/// ```
/// use html_form::{Form, Text};
///
/// struct Db(Vec<&'static str>);
///
/// #[derive(Form, Debug)]
/// #[form(context = Db)]
/// struct Signup {
///     // Takes the context, because only the database knows what is free.
///     #[field(validate = is_available)]
///     username: String,
///     // Takes only the value, as it always could.
///     #[field(validate = is_even)]
///     seats: u32,
/// }
///
/// fn is_available(name: &String, db: &Db) -> Result<(), Text> {
///     match db.0.contains(&name.as_str()) {
///         true => Err(Text::key("signup.username.taken")),
///         false => Ok(()),
///     }
/// }
///
/// fn is_even(seats: &u32) -> bool {
///     seats % 2 == 0
/// }
///
/// let db = Db(vec!["ada"]);
/// let errors = Signup::from_urlencoded_with_context("username=ada&seats=3", &db).unwrap_err();
/// assert!(errors.has_field("username") && errors.has_field("seats"));
/// ```
pub trait FieldValidator<T, C, M> {
    /// The error to put on the field, or `None` when the value passed.
    fn check(&self, value: &T, context: &C) -> Option<FieldError>;
}

impl<F, T, C, V> FieldValidator<T, C, WithoutContext> for F
where
    F: Fn(&T) -> V,
    V: FieldValidation,
{
    fn check(&self, value: &T, _context: &C) -> Option<FieldError> {
        self(value).into_field_error()
    }
}

impl<F, T, C, V> FieldValidator<T, C, WithContext> for F
where
    F: Fn(&T, &C) -> V,
    V: FieldValidation,
{
    fn check(&self, value: &T, context: &C) -> Option<FieldError> {
        self(value, context).into_field_error()
    }
}

/// What `#[form(validate = ...)]` may name: a [`FieldValidator`] for the whole
/// struct, which returns anything [`FormValidation`] accepts.
///
/// | Signature | The crate calls it with |
/// |---|---|
/// | `fn(&Self) -> impl FormValidation` | the parsed form |
/// | `fn(&Self, &Context) -> impl FormValidation` | the form and the context |
pub trait FormValidator<T, C, M> {
    /// The errors to add to the parse, or `None` when the form passed.
    fn check(&self, value: &T, context: &C) -> Option<FormErrors>;
}

impl<F, T, C, V> FormValidator<T, C, WithoutContext> for F
where
    F: Fn(&T) -> V,
    V: FormValidation,
{
    fn check(&self, value: &T, _context: &C) -> Option<FormErrors> {
        self(value).into_form_errors()
    }
}

impl<F, T, C, V> FormValidator<T, C, WithContext> for F
where
    F: Fn(&T, &C) -> V,
    V: FormValidation,
{
    fn check(&self, value: &T, context: &C) -> Option<FormErrors> {
        self(value, context).into_form_errors()
    }
}

#[cfg(test)]
mod tests {
    use super::format::*;
    use super::*;
    use crate::spec::{
        Choice, ChoiceStyle, ChooseControl, FileControl, NumberControl, NumberFormat,
        TemporalControl, TextControl, TextareaControl,
    };

    /// A spec is a `const` everywhere else. A test builds one field at a time,
    /// so it says only what the check under test reads.
    fn spec(control: Control) -> FieldSpec {
        FieldSpec {
            name: "field",
            control,
            ..FieldSpec::DEFAULT
        }
    }

    fn kinds(control: Control, raw: &str) -> Vec<ErrorKind> {
        check(&spec(control), raw)
            .into_iter()
            .map(|e| e.kind)
            .collect()
    }

    fn text(control: TextControl) -> Control {
        Control::Text(control)
    }

    fn bounds(
        min: Option<&'static str>,
        max: Option<&'static str>,
        step: Option<&'static str>,
    ) -> Bounds {
        Bounds { min, max, step }
    }

    fn number(bounds: Bounds) -> Control {
        Control::Number(NumberControl {
            format: NumberFormat::Number,
            bounds,
        })
    }

    fn temporal(format: TemporalFormat, bounds: Bounds) -> Control {
        Control::Temporal(TemporalControl { format, bounds })
    }

    // ─── The address production ───────────────────────────────────────────────

    #[test]
    fn an_address_needs_a_local_part_an_at_sign_and_a_domain() {
        assert!(is_email("ada@example.com"));
        assert!(is_email("a@b"));
        // Every punctuation character the HTML production lists.
        assert!(is_email("a.!#$%&'*+/=?^_`{|}~-@example.com"));

        assert!(!is_email("ada"), "no at sign");
        assert!(!is_email("@example.com"), "no local part");
        assert!(!is_email("ada@"), "no domain");
        assert!(!is_email("ada name@example.com"), "a space is not allowed");
    }

    /// A second `@` lands in the domain, where it is not a legal label
    /// character. There is no separate check for it.
    #[test]
    fn a_second_at_sign_is_rejected_by_the_domain_label() {
        assert!(!is_email("ada@example@com"));
    }

    #[test]
    fn a_domain_label_is_short_non_empty_and_not_hyphen_bounded() {
        assert!(!is_email("ada@example..com"), "an empty label");
        assert!(!is_email("ada@-example.com"), "a leading hyphen");
        assert!(!is_email("ada@example-.com"), "a trailing hyphen");
        assert!(!is_email("ada@ex_ample.com"), "an underscore");
        assert!(is_email("ada@ex-ample.com"), "a hyphen inside is fine");

        let long = "a".repeat(63);
        assert!(is_email(&format!("ada@{long}.com")), "63 is the limit");
        assert!(!is_email(&format!("ada@{long}a.com")), "64 is over it");
    }

    /// `multiple` accepts a comma-separated list, and trims around each entry,
    /// as HTML does.
    #[test]
    fn a_multiple_email_field_takes_a_comma_separated_list() {
        let one = text(TextControl {
            format: TextFormat::Email { multiple: false },
            ..TextControl::DEFAULT
        });
        let many = text(TextControl {
            format: TextFormat::Email { multiple: true },
            ..TextControl::DEFAULT
        });

        assert!(kinds(one, "a@b.com, c@d.com").len() == 1);
        assert!(kinds(many, "a@b.com, c@d.com").is_empty());
        // One bad entry rejects the list, and the message says it wanted a list.
        assert_eq!(
            kinds(many, "a@b.com, nope"),
            [ErrorKind::Invalid {
                expected: "a comma-separated list of email addresses".into()
            }]
        );
    }

    // ─── The other formats ────────────────────────────────────────────────────

    #[test]
    fn a_url_needs_a_scheme_a_colon_and_something_after_it() {
        assert!(is_url("https://example.com"));
        assert!(is_url("mailto:ada@example.com"));
        assert!(is_url("a+b-c.d:x"), "the scheme punctuation HTML allows");

        assert!(!is_url("example.com"), "no scheme");
        assert!(!is_url("https:"), "nothing after the colon");
        assert!(
            !is_url("1https://example.com"),
            "a scheme starts with a letter"
        );
        assert!(!is_url("ht tps://example.com"), "no whitespace anywhere");
        assert!(!is_url("https://exa mple.com"), "not even later on");
    }

    #[test]
    fn a_colour_is_six_hex_digits_behind_a_hash() {
        assert!(is_color("#00ff7F"));
        assert!(!is_color("00ff7f"), "no hash");
        assert!(!is_color("#00ff7"), "five digits");
        assert!(!is_color("#00ff7fa"), "seven digits");
        assert!(!is_color("#00ff7g"), "not a hex digit");

        assert_eq!(
            kinds(Control::Color, "red"),
            [ErrorKind::Invalid {
                expected: "a color as #rrggbb".into()
            }]
        );
        assert!(kinds(Control::Color, "#ffffff").is_empty());
    }

    #[test]
    fn a_date_is_checked_against_the_calendar_not_just_the_digit_count() {
        assert!(is_date("2026-08-06"));
        assert!(is_date("2026-01-31"));
        assert!(is_date("2024-02-29"), "a leap year");

        assert!(!is_date("2026-02-29"), "not a leap year");
        assert!(!is_date("1900-02-29"), "a century that is not a leap year");
        assert!(is_date("2000-02-29"), "a century divisible by 400");
        assert!(!is_date("2026-04-31"), "April has 30 days");
        assert!(!is_date("2026-13-01"), "there is no month 13");
        assert!(!is_date("2026-00-01"), "nor a month zero");
        assert!(!is_date("2026-01-00"), "nor a day zero");
        assert!(!is_date("2026-1-01"), "the month is two digits");
        assert!(!is_date("26-01-01"), "the year is four");
        assert!(!is_date("2026-01-1"), "and the day is two");
        assert!(!is_date("20260101"), "with the separators");
        assert!(!is_date("not-a-date"), "digits, at that");
    }

    #[test]
    fn a_time_is_hours_and_minutes_and_may_carry_seconds() {
        assert!(is_time("00:00"));
        assert!(is_time("23:59"));
        assert!(is_time("12:30:45"));
        assert!(is_time("12:30:45.1"));
        assert!(is_time("12:30:45.123"));

        assert!(!is_time("24:00"), "the hour tops out at 23");
        assert!(!is_time("12:60"), "and the minute at 59");
        assert!(!is_time("12:30:60"), "and the second at 59");
        assert!(!is_time("12"), "minutes are not optional");
        assert!(!is_time("1:30"), "the hour is two digits");
        assert!(!is_time("12:30:45.1234"), "at most three fraction digits");
        assert!(!is_time("12:30:45."), "and at least one");
        assert!(!is_time("12:30:45.abc"), "which are digits");
        assert!(!is_time("12:30:45:00"), "there is no fourth part");
    }

    #[test]
    fn a_local_datetime_joins_the_two_with_a_t_or_a_space() {
        assert!(is_datetime_local("2026-08-06T12:30"));
        assert!(is_datetime_local("2026-08-06 12:30"), "HTML allows a space");
        assert!(is_datetime_local("2026-08-06T12:30:45.500"));

        assert!(!is_datetime_local("2026-08-06"), "no time");
        assert!(!is_datetime_local("2026-02-29T12:30"), "an impossible date");
        assert!(!is_datetime_local("2026-08-06T25:00"), "an impossible time");
    }

    #[test]
    fn a_month_is_a_year_and_a_month() {
        assert!(is_month("2026-08"));
        assert!(!is_month("2026-13"));
        assert!(!is_month("2026"));
        assert!(!is_month("2026-8"));
    }

    /// An ISO 8601 long year has 53 weeks. It starts on a Thursday, or on a
    /// Wednesday in a leap year.
    #[test]
    fn a_week_number_is_checked_against_the_length_of_its_year() {
        assert!(is_week("2026-W01"));
        assert!(is_week("2026-W52"));

        assert!(is_week("2026-W53"), "2026 starts on a Thursday");
        assert!(
            is_week("2020-W53"),
            "2020 is a leap year starting Wednesday"
        );
        assert!(!is_week("2025-W53"), "2025 is an ordinary 52-week year");

        assert!(!is_week("2026-W00"), "weeks are one-based");
        assert!(!is_week("2026-W54"), "and never reach 54");
        assert!(!is_week("2026W01"), "the separator is `-W`");
        assert!(!is_week("26-W01"), "the year is four digits");
        assert!(!is_week("2026-Wxx"), "and the week is two");
    }

    #[test]
    fn a_temporal_control_reports_the_shape_it_wanted() {
        for (format, raw, expected) in [
            (TemporalFormat::Date, "nope", "a date as YYYY-MM-DD"),
            (TemporalFormat::Time, "nope", "a time as HH:MM"),
            (
                TemporalFormat::DatetimeLocal,
                "nope",
                "a date and time as YYYY-MM-DDTHH:MM",
            ),
            (TemporalFormat::Month, "nope", "a month as YYYY-MM"),
            (TemporalFormat::Week, "nope", "a week as YYYY-Www"),
        ] {
            assert_eq!(
                kinds(temporal(format, Bounds::DEFAULT), raw),
                [ErrorKind::Invalid {
                    expected: expected.into()
                }],
                "{format:?}"
            );
        }
    }

    /// A format that accepts anything is not a check at all, so nothing about
    /// the value can fail it.
    #[test]
    fn the_free_text_formats_accept_whatever_arrives() {
        for format in [
            TextFormat::Text,
            TextFormat::Password,
            TextFormat::Tel,
            TextFormat::Search,
        ] {
            let control = text(TextControl {
                format,
                ..TextControl::DEFAULT
            });
            assert!(kinds(control, "\u{1f600} anything at all").is_empty());
        }
    }

    // ─── Bounds ───────────────────────────────────────────────────────────────

    #[test]
    fn a_numeric_bound_is_compared_as_a_number() {
        let control = number(bounds(Some("10"), Some("20"), None));
        assert!(kinds(control, "15").is_empty());
        assert_eq!(
            kinds(control, "9"),
            [ErrorKind::TooSmall { min: "10".into() }]
        );
        assert_eq!(
            kinds(control, "21"),
            [ErrorKind::TooLarge { max: "20".into() }]
        );
    }

    /// The Rust type rejects what is not a number, and says more about it than
    /// a bound could. So the bound check leaves it alone rather than adding a
    /// second, vaguer complaint.
    #[test]
    fn a_value_that_is_not_a_number_is_left_to_the_rust_type() {
        assert!(kinds(number(bounds(Some("10"), None, None)), "abc").is_empty());
    }

    #[test]
    fn a_control_with_no_bounds_checks_nothing() {
        assert!(kinds(number(Bounds::DEFAULT), "-1").is_empty());
        assert!(Bounds::DEFAULT.is_empty());
        assert!(!bounds(None, None, Some("1")).is_empty());
    }

    /// A bound the crate cannot read as a number is not a bound. It still
    /// renders, so the browser makes whatever it can of it.
    #[test]
    fn an_unparseable_bound_is_skipped_rather_than_failing_everything() {
        assert!(kinds(number(bounds(Some("ten"), Some("twenty"), None)), "15").is_empty());
    }

    #[test]
    fn a_step_grid_starts_at_min_where_there_is_one() {
        // No `min`: the grid starts at zero.
        let from_zero = number(bounds(None, None, Some("5")));
        assert!(kinds(from_zero, "10").is_empty());
        assert_eq!(
            kinds(from_zero, "12"),
            [ErrorKind::Step { step: "5".into() }]
        );

        // With one: the grid starts there instead.
        let from_two = number(bounds(Some("2"), None, Some("5")));
        assert!(kinds(from_two, "12").is_empty());
        assert!(!kinds(from_two, "10").is_empty());
    }

    /// Binary floating point cannot hold 0.1, so a strict remainder would
    /// reject a value the user typed exactly as the grid asks for.
    #[test]
    fn a_fractional_step_tolerates_what_binary_floats_cannot_hold() {
        let tenths = number(bounds(None, None, Some("0.1")));
        assert!(kinds(tenths, "0.3").is_empty());
        assert!(kinds(tenths, "12345.6").is_empty());
        assert!(!kinds(tenths, "0.35").is_empty());
    }

    #[test]
    fn a_step_the_check_cannot_use_turns_it_off() {
        assert_eq!(parse_step("any"), None);
        assert_eq!(parse_step("ANY"), None, "the keyword is case-insensitive");
        assert_eq!(parse_step("0"), None, "a step of zero divides by nothing");
        assert_eq!(parse_step("-1"), None, "nor does a negative one");
        assert_eq!(parse_step("inf"), None, "nor an infinite one");
        assert_eq!(parse_step("nope"), None);
        assert_eq!(parse_step("0.5"), Some(0.5));

        // And with it off, every value sits on the grid.
        assert!(kinds(number(bounds(None, None, Some("any"))), "1.234567").is_empty());
    }

    /// Every format HTML uses puts string order and time order together, so a
    /// string comparison is the whole job.
    #[test]
    fn a_date_bound_is_compared_as_a_string() {
        let control = temporal(
            TemporalFormat::Date,
            bounds(Some("2026-01-01"), Some("2026-12-31"), None),
        );
        assert!(kinds(control, "2026-06-15").is_empty());
        assert_eq!(
            kinds(control, "2025-12-31"),
            [ErrorKind::TooSmall {
                min: "2026-01-01".into()
            }]
        );
        assert_eq!(
            kinds(control, "2027-01-01"),
            [ErrorKind::TooLarge {
                max: "2026-12-31".into()
            }]
        );
    }

    // ─── Length and pattern ───────────────────────────────────────────────────

    #[test]
    fn length_is_counted_in_characters_not_bytes() {
        let control = text(TextControl {
            minlength: Some(2),
            maxlength: Some(4),
            ..TextControl::DEFAULT
        });
        // Four characters, twelve bytes.
        assert!(kinds(control, "\u{e9}\u{e9}\u{e9}\u{e9}").is_empty());
        assert_eq!(
            kinds(control, "a"),
            [ErrorKind::TooShort {
                minlength: 2,
                length: 1
            }]
        );
        assert_eq!(
            kinds(control, "abcde"),
            [ErrorKind::TooLong {
                maxlength: 4,
                length: 5
            }]
        );
    }

    #[test]
    fn a_textarea_is_checked_for_length_and_nothing_else() {
        let control = Control::Textarea(TextareaControl {
            minlength: Some(10),
            ..TextareaControl::DEFAULT
        });
        assert_eq!(
            kinds(control, "short"),
            [ErrorKind::TooShort {
                minlength: 10,
                length: 5
            }]
        );
        assert!(kinds(control, "long enough to pass").is_empty());
    }

    #[test]
    fn a_pattern_has_to_match_the_whole_value() {
        let control = text(TextControl {
            pattern: Some("[a-z]+"),
            ..TextControl::DEFAULT
        });
        assert!(kinds(control, "abc").is_empty());
        assert_eq!(
            kinds(control, "abc1"),
            [ErrorKind::Pattern {
                pattern: "[a-z]+".into()
            }]
        );
    }

    /// A pattern the engine will not compile must not reject everything the
    /// user types. The browser is then the only place that enforces it.
    #[cfg(feature = "pattern")]
    #[test]
    fn a_pattern_that_does_not_compile_rejects_nothing() {
        let control = text(TextControl {
            pattern: Some("("),
            ..TextControl::DEFAULT
        });
        assert!(kinds(control, "anything").is_empty());
        // Twice, because the failure is cached like any other compilation.
        assert!(kinds(control, "anything").is_empty());
    }

    /// The format check answers "is this an address at all". A finer check of
    /// which addresses count says nothing useful until it is.
    #[test]
    fn a_broken_format_stops_the_finer_checks_of_the_same_control() {
        let control = text(TextControl {
            format: TextFormat::Email { multiple: false },
            pattern: Some(".*@example\\.com"),
            minlength: Some(30),
            ..TextControl::DEFAULT
        });
        assert_eq!(
            kinds(control, "nope"),
            [ErrorKind::Invalid {
                expected: "a valid email address".into()
            }]
        );
    }

    /// Where the format holds, though, every remaining check runs, so one pass
    /// reports everything wrong with the value.
    #[test]
    fn otherwise_every_check_of_a_control_runs() {
        let control = text(TextControl {
            pattern: Some("[0-9]+"),
            minlength: Some(10),
            maxlength: Some(1),
            ..TextControl::DEFAULT
        });
        assert_eq!(
            kinds(control, "abc"),
            [
                ErrorKind::Pattern {
                    pattern: "[0-9]+".into()
                },
                ErrorKind::TooShort {
                    minlength: 10,
                    length: 3
                },
                ErrorKind::TooLong {
                    maxlength: 1,
                    length: 3
                },
            ]
        );
    }

    // ─── Choices, and the controls with nothing to check ──────────────────────

    #[test]
    fn a_value_has_to_be_one_of_the_declared_choices() {
        const CHOICES: &[Choice] = &[Choice::new("de", "Germany"), Choice::new("ch", "Swiss")];
        let control = Control::Choose(ChooseControl {
            style: ChoiceStyle::Select,
            multiple: false,
            choices: CHOICES,
        });
        assert!(kinds(control, "de").is_empty());
        assert_eq!(kinds(control, "fr"), [ErrorKind::NotAChoice]);
    }

    /// An empty list means the options arrive at render time, from a database
    /// or wherever else. There is nothing yet to check the value against.
    #[test]
    fn choices_that_arrive_at_render_time_are_not_checked_here() {
        assert!(kinds(Control::SELECT, "anything").is_empty());
    }

    #[test]
    fn the_controls_the_spec_cannot_constrain_are_left_alone() {
        for control in [
            Control::Checkbox,
            Control::File(FileControl::DEFAULT),
            Control::Hidden,
        ] {
            assert!(kinds(control, "whatever").is_empty(), "{control:?}");
        }
    }

    // ─── What a validator returns ─────────────────────────────────────────────

    #[test]
    fn a_predicate_rejects_with_the_built_in_message_and_no_code() {
        assert_eq!(true.into_field_error(), None);
        let error = false.into_field_error().unwrap();
        assert_eq!(error.kind, ErrorKind::Custom { code: None });
        assert_eq!(error.message.as_str(), "This value is not valid.");

        assert_eq!(true.into_form_errors(), None);
        let errors = false.into_form_errors().unwrap();
        assert_eq!(errors.form_errors().len(), 1);
    }

    #[test]
    fn a_result_carries_whatever_its_error_converts_into() {
        let ok: Result<(), &'static str> = Ok(());
        assert_eq!(ok.into_field_error(), None);

        let failed: Result<(), &'static str> = Err("no");
        assert_eq!(failed.into_field_error().unwrap().message.as_str(), "no");

        let ok: Result<(), &'static str> = Ok(());
        assert_eq!(ok.into_form_errors(), None);

        let failed: Result<(), (&'static str, &'static str)> = Err(("password", "no"));
        assert!(failed.into_form_errors().unwrap().has_field("password"));
    }

    /// Either arity works, and the crate reads which one was written off the
    /// function itself.
    #[test]
    fn a_validator_may_take_the_context_or_ignore_it() {
        fn alone(value: &u32) -> bool {
            *value > 0
        }
        fn with_context(value: &u32, limit: &u32) -> bool {
            value <= limit
        }

        assert_eq!(FieldValidator::check(&alone, &1, &10), None);
        assert!(FieldValidator::check(&alone, &0, &10).is_some());
        assert_eq!(FieldValidator::check(&with_context, &5, &10), None);
        assert!(FieldValidator::check(&with_context, &50, &10).is_some());

        assert_eq!(FormValidator::check(&alone, &1, &10), None);
        assert!(FormValidator::check(&alone, &0, &10).is_some());
        assert_eq!(FormValidator::check(&with_context, &5, &10), None);
        assert!(FormValidator::check(&with_context, &50, &10).is_some());
    }

    #[test]
    fn a_missing_required_value_is_reported_as_required() {
        assert_eq!(
            required_error(&spec(Control::TEXT)).kind,
            ErrorKind::Required
        );
    }
}
