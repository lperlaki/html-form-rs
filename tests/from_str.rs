#![allow(dead_code)]

//! `from_str`: a type converted by its own `FromStr` and `Display` rather than
//! by a `FormValue` impl it has not got — which is every type from a crate that
//! has never heard of this one.

use std::fmt;
use std::str::FromStr;

use html_form::{Control, ErrorKind, FieldKind, Form, FormValue, Text};

/// Stands in for the foreign types this is for — a `Uuid`, a `NaiveDate`, a
/// `Decimal`: no `FormValue` impl, and none that can be written here.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Isbn(u64);

impl FromStr for Isbn {
    type Err = &'static str;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let digits: String = raw.chars().filter(|c| *c != '-').collect();
        match digits.len() == 13 && digits.bytes().all(|b| b.is_ascii_digit()) {
            true => Ok(Isbn(digits.parse().map_err(|_| "not a number")?)),
            false => Err("thirteen digits"),
        }
    }
}

impl fmt::Display for Isbn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:013}", self.0)
    }
}

#[derive(Form, Debug)]
struct Book {
    #[field(from_str, label = "ISBN", pattern = r"[\d-]+")]
    isbn: Isbn,

    #[field(from_str)]
    reprint_of: Option<Isbn>,

    #[field(from_str)]
    cites: Vec<Isbn>,

    title: String,
}

#[test]
fn a_foreign_type_becomes_a_field_without_an_impl() {
    let book = Book::from_urlencoded(
        "isbn=978-0-13-475759-9&cites=9780262033848&cites=9781593278281&title=Ada",
    )
    .unwrap();

    assert_eq!(book.isbn, Isbn(9_780_134_757_599));
    assert_eq!(book.reprint_of, None);
    assert_eq!(book.cites.len(), 2);
    assert_eq!(book.title, "Ada");
}

#[test]
fn it_renders_as_text_until_the_field_says_otherwise() {
    // A foreign type has no `CONTROL` to be asked for, so text it is.
    let view = Book::render();
    let isbn = view.field("isbn").unwrap();
    assert_eq!(isbn.kind, FieldKind::Text);
    assert_eq!(isbn.pattern.as_deref(), Some(r"[\d-]+"));
    assert!(isbn.required);
    // The shape of the field still decides the rest of it.
    assert!(!view.field("reprint_of").unwrap().required);
    assert!(!view.field("cites").unwrap().required);
}

#[test]
fn a_value_that_will_not_parse_is_reported_like_any_other() {
    let errors = Book::from_urlencoded("isbn=12345&title=Ada").unwrap_err();
    let error = errors.field("isbn").next().unwrap();
    assert_eq!(
        error.kind,
        ErrorKind::Invalid {
            expected: "a valid value".into()
        }
    );
    assert_eq!(error.message.as_str(), "Enter a valid value.");
}

#[test]
fn the_constraints_in_the_spec_are_checked_first_and_say_more() {
    // `pattern` rejects the letters before `FromStr` gets a word in, which is
    // the whole reason a field pairs `from_str` with a `type` or a `pattern`.
    let errors = Book::from_urlencoded("isbn=not-an-isbn&title=Ada").unwrap_err();
    assert!(matches!(
        errors.field("isbn").next().unwrap().kind,
        ErrorKind::Pattern { .. }
    ));
    assert_eq!(errors.field("isbn").count(), 1);
}

#[test]
fn a_type_check_pairs_with_the_html_one() {
    #[derive(Form, Debug)]
    struct Dated {
        // The format check comes from `type`, and its message is the good one.
        #[field(type = "date", from_str, min = "2026-01-01")]
        published: Day,
    }

    #[derive(Debug, PartialEq)]
    struct Day(String);

    impl FromStr for Day {
        type Err = &'static str;
        fn from_str(raw: &str) -> Result<Self, Self::Err> {
            Ok(Day(raw.to_owned()))
        }
    }

    impl fmt::Display for Day {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }

    assert_eq!(
        Dated::render().field("published").unwrap().kind,
        FieldKind::Date
    );

    let errors = Dated::from_urlencoded("published=nonsense").unwrap_err();
    assert_eq!(
        errors.field("published").next().unwrap().message.as_str(),
        "Enter a date as YYYY-MM-DD."
    );

    let errors = Dated::from_urlencoded("published=2025-06-01").unwrap_err();
    assert!(matches!(
        errors.field("published").next().unwrap().kind,
        ErrorKind::TooSmall { .. }
    ));
}

#[test]
fn display_writes_it_back_out() {
    let book = Book {
        isbn: Isbn(9_780_134_757_599),
        reprint_of: Some(Isbn(9_780_262_033_848)),
        cites: vec![Isbn(9_781_593_278_281)],
        title: "Ada".to_owned(),
    };

    let values = book.to_values();
    assert_eq!(values.get("isbn"), Some("9780134757599"));
    assert_eq!(values.get("reprint_of"), Some("9780262033848"));
    assert_eq!(values.get("cites"), Some("9781593278281"));

    // So an edit form shows what the record holds.
    let view = book.render_filled();
    assert_eq!(
        view.field("isbn").unwrap().value.as_deref(),
        Some("9780134757599")
    );
}

#[test]
fn surrounding_whitespace_is_dropped_before_the_type_sees_it() {
    let book =
        Book::from_urlencoded("isbn=9780134757599&reprint_of=++9780262033848++&title=Ada").unwrap();
    assert_eq!(book.reprint_of, Some(Isbn(9_780_262_033_848)));

    // The constraints in the spec are checked against the raw value, though,
    // exactly as the browser checks them — so a `pattern` has to allow for
    // whatever it is willing to be given.
    let errors = Book::from_urlencoded("isbn=++9780134757599++&title=Ada").unwrap_err();
    assert!(matches!(
        errors.field("isbn").next().unwrap().kind,
        ErrorKind::Pattern { .. }
    ));
}

#[test]
fn a_validator_sees_the_type_the_field_was_written_as() {
    #[derive(Form, Debug)]
    struct Checked {
        #[field(from_str, validate = is_registered)]
        isbn: Isbn,
        #[field(from_str, validate = at_most_two)]
        cites: Vec<Isbn>,
    }

    fn is_registered(isbn: &Isbn) -> Result<(), Text> {
        match isbn.0 > 9_780_000_000_000 {
            true => Ok(()),
            false => Err(Text::key("book.isbn.unregistered")),
        }
    }

    // A validator is handed the field's own type, so a `Vec` field's takes a
    // `&Vec` — that is what lets it check cardinality at all.
    #[allow(clippy::ptr_arg)]
    fn at_most_two(cites: &Vec<Isbn>) -> bool {
        cites.len() <= 2
    }

    assert!(Checked::from_urlencoded("isbn=9780134757599").is_ok());

    let errors = Checked::from_urlencoded(
        "isbn=1234567890123&cites=9780134757599&cites=9780262033848&cites=9781593278281",
    )
    .unwrap_err();
    assert_eq!(
        errors.field("isbn").next().unwrap().code(),
        Some("book.isbn.unregistered")
    );
    assert!(errors.has_field("cites"));
}

#[test]
fn a_generic_form_asks_for_the_conversion_its_field_declared() {
    // `T: FormValue` is not the bound this field wants, and the derive knows
    // which one it does — nothing is written on the struct either way.
    #[derive(Form, Debug)]
    struct Wrapper<T> {
        #[field(from_str, label = "Value")]
        value: T,
    }

    let wrapped = Wrapper::<Isbn>::from_urlencoded("value=9780134757599").unwrap();
    assert_eq!(wrapped.value, Isbn(9_780_134_757_599));
    assert_eq!(
        Wrapper::<Isbn>::render()
            .field("value")
            .unwrap()
            .label
            .as_deref(),
        Some("Value")
    );
}

// ─── The same, on the type rather than on every field ─────────────────────────

/// With `from_str` the derive asks nothing of the shape of the type, so this is
/// a struct no wrapper spelling could have described.
#[derive(FormValue, Debug, PartialEq)]
#[value(from_str, type = "text", pattern = r"\d+\.\d+\.\d+", default = "1.0.0")]
struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl FromStr for Version {
    type Err = &'static str;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let mut parts = raw.split('.');
        let mut next = || {
            parts
                .next()
                .ok_or("three numbers")?
                .parse::<u32>()
                .map_err(|_| "a number")
        };
        let version = Version {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        match parts.next() {
            None => Ok(version),
            Some(_) => Err("three numbers"),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// And an enum, which the wrapper spelling rejects outright.
#[derive(FormValue, Debug, PartialEq)]
#[value(from_str)]
enum Channel {
    Stable,
    Beta,
}

impl FromStr for Channel {
    type Err = &'static str;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "stable" => Ok(Channel::Stable),
            "beta" => Ok(Channel::Beta),
            _ => Err("stable or beta"),
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        })
    }
}

#[derive(Form, Debug)]
struct Release {
    // No `from_str` on the field: the type is a `FormValue` in its own right
    // now, so every form that uses it needs nothing said.
    version: Version,
    channel: Channel,
}

#[test]
fn a_type_may_convert_itself_and_carry_the_rest() {
    assert!(matches!(Version::CONTROL, Control::Text(_)));
    assert_eq!(Version::DEFAULT, Some("1.0.0"));

    let view = Release::render();
    let version = view.field("version").unwrap();
    assert_eq!(version.value.as_deref(), Some("1.0.0"));
    assert_eq!(version.pattern.as_deref(), Some(r"\d+\.\d+\.\d+"));

    let release = Release::from_urlencoded("version=2.11.3&channel=beta").unwrap();
    assert_eq!(
        release.version,
        Version {
            major: 2,
            minor: 11,
            patch: 3
        }
    );
    assert_eq!(release.channel, Channel::Beta);

    assert_eq!(
        release.to_values().get("version").unwrap().to_owned(),
        "2.11.3"
    );
}

#[test]
fn a_type_that_converts_itself_still_checks_itself() {
    #[derive(FormValue, Debug)]
    #[value(from_str, validate = is_released)]
    struct Released(Version);

    impl FromStr for Released {
        type Err = &'static str;
        fn from_str(raw: &str) -> Result<Self, Self::Err> {
            raw.parse().map(Released)
        }
    }

    impl fmt::Display for Released {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.fmt(f)
        }
    }

    fn is_released(version: &Released) -> bool {
        version.0.major >= 1
    }

    #[derive(Form, Debug)]
    struct Publish {
        version: Released,
    }

    assert!(Publish::from_urlencoded("version=1.0.0").is_ok());
    assert_eq!(
        Publish::from_urlencoded("version=0.9.0")
            .unwrap_err()
            .field("version")
            .next()
            .unwrap()
            .kind,
        ErrorKind::Custom { code: None }
    );
}

#[test]
fn what_it_will_not_parse_is_rejected_wherever_the_type_is_used() {
    let errors = Release::from_urlencoded("version=2.11&channel=nightly").unwrap_err();
    // The pattern the type carries speaks before its `FromStr` does.
    assert!(matches!(
        errors.field("version").next().unwrap().kind,
        ErrorKind::Pattern { .. }
    ));
    assert!(matches!(
        errors.field("channel").next().unwrap().kind,
        ErrorKind::Invalid { .. }
    ));
}
