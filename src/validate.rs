//! Server-side re-checking of the HTML validation attributes, and the shapes a
//! `validate = ...` function is allowed to return.
//!
//! Everything the markup asks the browser to enforce is enforced again here,
//! because a submission can come from anywhere. What the markup *cannot* ask
//! for is a `validate = ...` function, whose return value reaches the parse
//! through [`FieldValidation`] or [`FormValidation`].

use crate::context::{WithContext, WithoutContext};
use crate::error::{ErrorKind, FieldError, FormErrors};
use crate::spec::{Bounds, Control, FieldSpec, TemporalFormat, TextFormat};

/// Check one non-blank submitted value against a field's constraints.
///
/// Returns every violation, not just the first. Which checks even exist is
/// decided by the [`Control`]: there is no arm in which a `<select>` has a
/// `pattern` or a date has a `minlength`, because those are not states the
/// spec can be in.
pub fn check(spec: &FieldSpec, raw: &str) -> Vec<FieldError> {
    let mut errors = Vec::new();

    match &spec.control {
        Control::Text(text) => {
            // The control's own type is a constraint too: `type="email"` is a
            // promise the browser keeps and a submission from anywhere else
            // need not. A value of the wrong shape entirely makes the
            // finer-grained checks redundant — something that is not an address
            // will usually also fail the `pattern` written to narrow which
            // addresses count.
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
                return vec![invalid("a colour as #rrggbb")];
            }
        }
        // A checkbox's value is checked by `bool`'s `FormValue` impl, a file's
        // bytes never reach this crate, and a hidden field carries whatever the
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

/// HTML counts length in UTF-16 code units; counting scalar values is close
/// enough and never surprises a caller writing plain text.
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

/// `min` / `max` / `step` for `number` and `range`, compared numerically.
///
/// A value that is not a number at all is left alone: the field's Rust type
/// rejects it with a better message than any bound could.
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

/// `min` / `max` for the date and time controls.
///
/// The value has already been checked against its format, and for every format
/// HTML uses, lexicographic order *is* chronological order — so a string
/// comparison is the whole job. `step` is rendered for the browser but has no
/// server-side meaning without a calendar.
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

/// `step="any"` (and a non-positive step) disables the check.
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
    // Relative tolerance: 0.1 + 0.2 must still count as being on a 0.1 grid.
    (steps - nearest).abs() <= 1e-9 * steps.abs().max(1.0)
}

/// Whether the whole value matches an HTML `pattern`.
///
/// Without the `pattern` feature the attribute is still rendered — and so still
/// enforced by the browser — but not re-checked here.
#[cfg(feature = "pattern")]
fn matches_pattern(pattern: &str, value: &str) -> bool {
    match compiled(pattern) {
        // An un-compilable pattern must not reject everything the user types;
        // the browser remains the only enforcement in that case.
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
    /// What this format expected, if `raw` is not in that shape.
    ///
    /// `Text`, `Password`, `Tel` and `Search` accept anything: HTML asks the
    /// browser for nothing more, so neither does the server.
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
/// Deliberately dependency-free and no stricter than the HTML specification:
/// the aim is to reject what a browser would have refused to submit, not to
/// decide whether an address can receive mail.
///
/// Numeric controls are left out on purpose — the field's Rust type already
/// rejects what it cannot represent, with a better message than a generic
/// "that is not a number".
mod format {
    /// The HTML "valid email address" production, which is intentionally more
    /// permissive than RFC 5322.
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
        // A second `@` lands in the domain, where it is not an allowed label
        // character, so it is rejected by the label check below.
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
            // A calendar check, so that 2026-02-31 is rejected like the browser
            // would reject it.
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
        // The HTML spec allows a space in place of the `T`.
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

    /// ISO 8601 long years (53 weeks) start on a Thursday, or on a Wednesday in
    /// a leap year.
    fn weeks_in_year(year: u32) -> u32 {
        let jan_1 = day_of_week(year, 1, 1);
        if jan_1 == 4 || (is_leap(year) && jan_1 == 3) {
            53
        } else {
            52
        }
    }

    /// Zeller-style day of week, 0 = Sunday.
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

/// The error produced for a required field that arrived missing or blank.
pub fn required_error(_spec: &FieldSpec) -> FieldError {
    FieldError::new(ErrorKind::Required)
}

// ─── What a `validate = ...` function may return ──────────────────────────────

/// What a `#[field(validate = ...)]` function may return.
///
/// Whichever shape the function has, the check runs against the field's own
/// Rust type — `Option<T>` and `Vec<T>` included — and only after the value
/// has survived the constraints in the spec.
///
/// | Return type | Rejects when | Message |
/// |---|---|---|
/// | `bool` | `false` | the built-in one for [`ErrorKind::Custom`] |
/// | `Result<(), &'static str \| String \| Cow>` | `Err` | the text returned |
/// | `Result<(), Text>` | `Err` | the text — or the i18n key — returned |
/// | `Result<(), FieldError>` | `Err` | whatever the error carries |
///
/// ```
/// use web_form::{Text, WebForm};
///
/// #[derive(WebForm, Debug)]
/// struct Signup {
///     // The shortest form: a predicate, and the built-in message.
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
    /// The error to attach to the field, or `None` when the value passed.
    fn into_field_error(self) -> Option<FieldError>;
}

impl FieldValidation for bool {
    /// A predicate says only whether the value is acceptable, so a rejection
    /// carries the built-in message and no code.
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
/// The same shapes as [`FieldValidation`], plus the ones that name a field:
/// a `(field, message)` pair, or a whole [`FormErrors`] built by the function
/// itself. A message that names no field belongs to the form as a whole.
///
/// ```
/// use web_form::{FormErrors, WebForm};
///
/// #[derive(WebForm, Debug)]
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
///         // Attached to the field that can be corrected, not to the form.
///         Err(("confirm", "The two passwords do not match.").into())
///     }
/// }
///
/// let errors = Signup::from_urlencoded("password=a&confirm=b").unwrap_err();
/// assert!(errors.has_field("confirm"));
/// ```
pub trait FormValidation {
    /// The errors to fold into the parse, or `None` when the form passed.
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
/// [`FieldValidation`] says what a validator may *return*; this says what it may
/// *take*. Either arity will do, and which one was written is read off the
/// function itself:
///
/// | Signature | Called with |
/// |---|---|
/// | `fn(&T) -> impl FieldValidation` | the field's value |
/// | `fn(&T, &Context) -> impl FieldValidation` | the value and the context |
///
/// `T` is the field's own Rust type, `Option<T>` and `Vec<T>` included, so a
/// validator can check emptiness or cardinality as well as the value.
///
/// ```
/// use web_form::{Text, WebForm};
///
/// struct Db(Vec<&'static str>);
///
/// #[derive(WebForm, Debug)]
/// #[form(context = Db)]
/// struct Signup {
///     // Takes the context: only the database knows what is left.
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
    /// The error to attach to the field, or `None` when the value passed.
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

/// What `#[form(validate = ...)]` may name: [`FieldValidator`] for the whole
/// assembled struct, returning anything [`FormValidation`] accepts.
///
/// | Signature | Called with |
/// |---|---|
/// | `fn(&Self) -> impl FormValidation` | the parsed form |
/// | `fn(&Self, &Context) -> impl FormValidation` | the form and the context |
pub trait FormValidator<T, C, M> {
    /// The errors to fold into the parse, or `None` when the form passed.
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
