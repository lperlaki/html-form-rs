#![allow(dead_code)]

//! Parsing and validating a submission. The defining property under test:
//! *every* problem is reported, not just the first one.

use std::borrow::Cow;

use html_form::{ErrorKind, FieldError, Form, FormErrors, Outcome, Text, Values};

#[derive(Form, Debug)]
#[form(validate = passwords_match)]
struct Signup {
    #[field(type = "email", pattern = r"[^@]+@[^@]+\.[a-z]{2,}")]
    email: String,

    #[field(type = "password", minlength = 8, maxlength = 64)]
    password: String,

    #[field(type = "password")]
    confirm: String,

    #[field(min = 18, max = 120)]
    age: Option<u32>,

    #[field(validate = not_reserved)]
    username: String,

    #[field(required)]
    accept_terms: bool,

    #[field(type = "select", multiple)]
    #[option("rust", "Rust")]
    #[option("go", "Go")]
    #[option("ts", "TypeScript")]
    languages: Vec<String>,

    #[field(default = "web", optional)]
    source: String,
}

fn passwords_match(form: &Signup) -> Result<(), FormErrors> {
    if form.password == form.confirm {
        Ok(())
    } else {
        Err(("confirm", "The two passwords do not match.").into())
    }
}

fn not_reserved(name: &String) -> Result<(), Cow<'static, str>> {
    if name == "admin" {
        Err("That name is reserved.".into())
    } else {
        Ok(())
    }
}

fn good_body() -> String {
    [
        "email=ada%40example.com",
        "password=correcthorse",
        "confirm=correcthorse",
        "age=36",
        "username=ada",
        "accept_terms=on",
        "languages=rust",
        "languages=go",
    ]
    .join("&")
}

#[test]
fn a_valid_submission_round_trips_into_the_struct() {
    let form = Signup::from_urlencoded(&good_body()).unwrap();

    assert_eq!(form.email, "ada@example.com");
    assert_eq!(form.age, Some(36));
    assert!(form.accept_terms);
    assert_eq!(form.languages, ["rust", "go"]);
    // Absent from the body, and a default fills in a *render*, so what the
    // struct holds is what arrived: nothing.
    assert_eq!(form.source, "");
}

#[test]
fn every_error_is_collected_in_one_pass() {
    // Every field here is wrong in a different way, and every one of them has
    // to be reported by the single pass.
    let body = "email=nope&password=short&confirm=other&age=7&username=admin&languages=cobol";
    let errors = Signup::from_urlencoded(body).unwrap_err();

    let kinds: Vec<(&str, &ErrorKind)> = errors.iter().map(|(n, e)| (n, &e.kind)).collect();

    // `nope` is not an address at all, so `type = "email"` rejects it before
    // the narrower `pattern` gets a say.
    assert!(matches!(kinds[0], ("email", ErrorKind::Invalid { .. })));
    assert!(matches!(kinds[1], ("password", ErrorKind::TooShort { .. })));
    assert!(matches!(kinds[2], ("age", ErrorKind::TooSmall { .. })));
    assert!(matches!(kinds[3], ("username", ErrorKind::Custom { .. })));
    assert!(matches!(kinds[4], ("accept_terms", ErrorKind::Required)));
    assert!(matches!(kinds[5], ("languages", ErrorKind::NotAChoice)));

    // The cross-field check needs a complete struct, and `accept_terms` never
    // produced a value, so `passwords_match` did not run — its "the two
    // passwords do not match" is the one message missing here.
    assert_eq!(errors.len(), 6);
}

#[test]
fn one_field_can_carry_several_errors() {
    #[derive(Form, Debug)]
    struct Code {
        #[field(pattern = "[a-z]+", minlength = 4)]
        value: String,
    }

    let errors = Code::from_urlencoded("value=A1").unwrap_err();
    let kinds: Vec<&ErrorKind> = errors.field("value").map(|e| &e.kind).collect();
    assert_eq!(kinds.len(), 2);
    assert!(matches!(kinds[0], ErrorKind::Pattern { .. }));
    assert!(matches!(kinds[1], ErrorKind::TooShort { .. }));
}

#[test]
fn the_form_level_validator_runs_once_the_struct_is_assembled() {
    let body = good_body().replace("confirm=correcthorse", "confirm=different");
    let errors = Signup::from_urlencoded(&body).unwrap_err();

    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors.field("confirm").next().unwrap().message.as_str(),
        "The two passwords do not match."
    );
}

#[test]
fn a_type_error_reports_what_was_expected() {
    let body = good_body().replace("age=36", "age=middle-aged");
    let errors = Signup::from_urlencoded(&body).unwrap_err();

    let error = errors.field("age").next().unwrap();
    assert!(matches!(&error.kind, ErrorKind::Invalid { expected } if expected == "a whole number"));
    assert_eq!(error.message.as_str(), "Enter a whole number.");
}

#[test]
fn an_out_of_range_integer_says_so_instead_of_just_not_a_number() {
    #[derive(Form, Debug)]
    struct Small {
        count: u8,
    }

    let errors = Small::from_urlencoded("count=300").unwrap_err();
    assert_eq!(
        errors.field("count").next().unwrap().message.as_str(),
        "Enter a whole number between 0 and 255."
    );
}

#[test]
fn a_range_violation_wins_over_the_conversion_failure() {
    // -5 is both out of range for the field and unrepresentable as u32; the
    // range message is the one worth showing.
    let body = good_body().replace("age=36", "age=-5");
    let errors = Signup::from_urlencoded(&body).unwrap_err();

    let kinds: Vec<&ErrorKind> = errors.field("age").map(|e| &e.kind).collect();
    assert_eq!(kinds.len(), 1);
    assert!(matches!(kinds[0], ErrorKind::TooSmall { .. }));
}

#[test]
fn blank_and_absent_optionals_are_both_none() {
    let blank = good_body().replace("age=36", "age=");
    assert_eq!(Signup::from_urlencoded(&blank).unwrap().age, None);

    let absent = good_body().replace("age=36&", "");
    assert_eq!(Signup::from_urlencoded(&absent).unwrap().age, None);
}

/// A default says what a blank *form* shows. A parse reads what arrived, and a
/// field the submission left out arrived as nothing.
#[test]
fn a_default_fills_a_render_and_never_a_parse() {
    // The blank form offers it…
    assert_eq!(
        Signup::render().field("source").unwrap().value.as_deref(),
        Some("web")
    );
    // …and the parse of a body without it gets nothing.
    let absent = Signup::from_urlencoded(&good_body()).unwrap();
    assert_eq!(absent.source, "");

    // Submitting the field empty is the same answer, and submitting a value is
    // the only way to hold one.
    let cleared = Signup::from_urlencoded(&format!("{}&source=", good_body())).unwrap();
    assert_eq!(cleared.source, "");
    let sent = Signup::from_urlencoded(&format!("{}&source=email", good_body())).unwrap();
    assert_eq!(sent.source, "email");
}

/// The rule holds whichever kind of default it is, which is what keeps a form
/// from answering its own question. A submission carrying no token at all must
/// not arrive holding a valid one.
#[test]
fn a_default_never_stands_in_for_a_field_a_submission_left_out() {
    fn issued() -> String {
        "issued".to_owned()
    }

    #[derive(Form, Debug)]
    struct Ticket {
        #[field(type = "hidden", default = issued)]
        token: String,
        #[field(type = "hidden", default = "web")]
        source: String,
    }

    // A body missing one of them is a body missing a required field, whether
    // the form could have produced the value or had it written down already.
    for (missing, body) in [("token", "source=web"), ("source", "token=abc")] {
        let errors = Ticket::from_urlencoded(body).unwrap_err();
        assert!(
            matches!(
                errors.field(missing).next().unwrap().kind,
                ErrorKind::Required
            ),
            "{missing} was not reported as missing"
        );
    }

    let ticket = Ticket::from_urlencoded("token=abc&source=email").unwrap();
    assert_eq!(ticket.token, "abc");
    assert_eq!(ticket.source, "email");
}

#[test]
fn an_unchecked_box_is_false_rather_than_missing() {
    #[derive(Form)]
    struct Prefs {
        #[field(default = true)]
        newsletter: bool,
    }

    // The default only pre-fills the blank form; an unchecked box submits
    // nothing, and that has to mean `false`.
    assert!(!Prefs::from_urlencoded("").unwrap().newsletter);
    assert!(Prefs::from_urlencoded("newsletter=on").unwrap().newsletter);
    assert!(
        !Prefs::from_urlencoded("newsletter=false")
            .unwrap()
            .newsletter
    );
}

#[test]
fn whitespace_does_not_satisfy_a_required_field() {
    let body = good_body().replace("username=ada", "username=+++");
    let errors = Signup::from_urlencoded(&body).unwrap_err();
    assert!(matches!(
        errors.field("username").next().unwrap().kind,
        ErrorKind::Required
    ));
}

#[test]
fn step_is_enforced() {
    #[derive(Form, Debug)]
    struct Order {
        #[field(min = 0, max = 100, step = 5)]
        quantity: u32,
    }

    assert!(Order::from_urlencoded("quantity=25").is_ok());
    let errors = Order::from_urlencoded("quantity=23").unwrap_err();
    assert!(matches!(
        errors.field("quantity").next().unwrap().kind,
        ErrorKind::Step { .. }
    ));
}

#[test]
fn dates_compare_chronologically() {
    #[derive(Form, Debug)]
    struct Booking {
        #[field(type = "date", min = "2026-01-01", max = "2026-12-31")]
        day: String,
    }

    assert!(Booking::from_urlencoded("day=2026-06-15").is_ok());
    let errors = Booking::from_urlencoded("day=2025-12-31").unwrap_err();
    assert!(matches!(
        errors.field("day").next().unwrap().kind,
        ErrorKind::TooSmall { .. }
    ));
}

// ─── What a `validate = ...` function may return ──────────────────────────────

#[derive(Form, Debug)]
#[form(validate = seats_fit_the_table)]
struct Booking {
    #[field(validate = is_even)]
    seats: u32,

    #[field(validate = not_reserved_room)]
    room: String,

    #[field(validate = is_upstairs, optional)]
    floor: String,
}

/// The shortest form: a predicate, with the built-in message.
fn is_even(seats: &u32) -> bool {
    seats.is_multiple_of(2)
}

/// A key, which becomes both the message and the code.
fn not_reserved_room(room: &String) -> Result<(), Text> {
    if room == "boardroom" {
        Err(Text::key("booking.room.reserved"))
    } else {
        Ok(())
    }
}

/// A code that is not the message, for a caller that wants both.
fn is_upstairs(floor: &String) -> Result<(), FieldError> {
    if floor == "basement" {
        Err(FieldError::coded("no_lift", "That floor has no lift."))
    } else {
        Ok(())
    }
}

/// A cross-field predicate: no message to give, only a verdict.
fn seats_fit_the_table(booking: &Booking) -> bool {
    booking.seats <= 12
}

#[test]
fn a_predicate_is_enough_when_the_built_in_message_will_do() {
    let errors = Booking::from_urlencoded("seats=3&room=kitchen").unwrap_err();

    let error = errors.field("seats").next().unwrap();
    assert!(matches!(error.kind, ErrorKind::Custom { code: None }));
    assert_eq!(error.message.as_str(), "This value is not valid.");

    assert!(Booking::from_urlencoded("seats=4&room=kitchen").is_ok());
}

#[test]
fn a_keyed_message_doubles_as_the_errors_code() {
    let errors = Booking::from_urlencoded("seats=4&room=boardroom").unwrap_err();
    let error = errors.field("room").next().unwrap();

    // Nothing has resolved the key yet, so it is what the message reads as —
    // the same bargain every other person-facing string strikes.
    assert_eq!(error.message.as_str(), "booking.room.reserved");
    assert_eq!(error.message.key_str(), Some("booking.room.reserved"));
    assert_eq!(error.code(), Some("booking.room.reserved"));
}

#[test]
fn a_code_can_be_named_apart_from_the_message() {
    let errors = Booking::from_urlencoded("seats=4&room=kitchen&floor=basement").unwrap_err();
    let error = errors.field("floor").next().unwrap();

    assert_eq!(error.code(), Some("no_lift"));
    assert_eq!(error.message.as_str(), "That floor has no lift.");
    // A message that is not a key is not one to translate.
    assert_eq!(error.message.key_str(), None);
}

#[test]
fn a_form_level_predicate_rejects_the_form_rather_than_a_field() {
    let errors = Booking::from_urlencoded("seats=14&room=kitchen").unwrap_err();

    assert_eq!(errors.len(), 1);
    assert!(errors.iter().next().is_none()); // nothing attached to a field
    assert!(matches!(
        errors.form_errors()[0].kind,
        ErrorKind::Custom { code: None }
    ));
}

#[test]
fn values_can_come_from_a_frameworks_own_body_parser() {
    let values = Values::from_pairs([
        ("email", "ada@example.com"),
        ("password", "correcthorse"),
        ("confirm", "correcthorse"),
        ("username", "ada"),
        ("accept_terms", "true"),
    ]);

    let form = Signup::from_values(&values).unwrap();
    assert_eq!(form.username, "ada");
}

#[test]
fn a_body_can_be_parsed_before_anything_has_called_it_a_string() {
    let body: &[u8] = b"username=ada&accept_terms=on";
    assert_eq!(
        Values::parse_bytes(body),
        Values::parse("username=ada&accept_terms=on")
    );

    // Percent-decoding produces bytes whatever the body was, so the decode is
    // lossy rather than fatal: one mangled value, not one lost submission.
    let mut mangled = b"username=ada&source=".to_vec();
    mangled.push(0xff);
    let values = Values::parse_bytes(&mangled);
    assert_eq!(values.get("username"), Some("ada"));
    assert_eq!(values.get("source"), Some("\u{fffd}"));
}

#[test]
fn an_invalid_submission_comes_back_as_a_renderable_form() {
    let body = "email=nope&password=short&confirm=short&username=ada&accept_terms=on&languages=go";

    let Outcome::Invalid { errors, view } = Signup::submit_urlencoded(body) else {
        panic!("expected the submission to be rejected");
    };

    assert_eq!(errors.len(), 2); // bad email, short password

    // What the user typed survives…
    assert_eq!(view.field("email").unwrap().value.as_deref(), Some("nope"));
    assert_eq!(view.field("languages").unwrap().values, ["go"]);
    assert!(view.field("accept_terms").unwrap().checked);

    // …the messages are attached to their fields…
    assert!(view.field("email").unwrap().has_errors);
    assert_eq!(view.field("password").unwrap().errors.len(), 1);
    assert!(!view.field("username").unwrap().has_errors);
    assert!(view.has_errors);

    // …the selected option stays selected…
    let go = view
        .field("languages")
        .unwrap()
        .choices
        .iter()
        .find(|c| c.value == "go")
        .unwrap();
    assert!(go.selected);

    // …and the markup says so, for both sighted users and screen readers.
    let html = view.to_html();
    assert!(html.contains(r#"value="nope""#));
    assert!(html.contains(r#"aria-invalid="true""#));
    assert!(html.contains(r#"aria-describedby="email-error""#));
    assert!(html.contains("html-form__field--invalid"));
}

#[test]
fn errors_serialise_for_json_apis() {
    let errors = Signup::from_urlencoded("email=nope").unwrap_err();
    let json = serde_json::to_value(&errors).unwrap();

    assert_eq!(json["fields"]["email"][0], "Enter a valid email address.");
    assert_eq!(json["fields"]["password"][0], "This field is required.");
    assert!(json["form"].as_array().unwrap().is_empty());
}

#[test]
fn a_form_can_be_filled_from_an_existing_value() {
    let form = Signup::from_urlencoded(&good_body()).unwrap();
    let view = form.render_filled();

    assert_eq!(
        view.field("email").unwrap().value.as_deref(),
        Some("ada@example.com")
    );
    assert_eq!(view.field("age").unwrap().value.as_deref(), Some("36"));
    assert_eq!(view.field("languages").unwrap().values, ["rust", "go"]);
    assert!(view.field("accept_terms").unwrap().checked);
    assert!(!view.has_errors);

    // And the filled form parses back into an equal value.
    let again = Signup::from_values(&form.to_values()).unwrap();
    assert_eq!(again.email, form.email);
    assert_eq!(again.languages, form.languages);
    assert_eq!(again.age, form.age);
}

// ─── Formats implied by the control's type ────────────────────────────────────

#[derive(Form, Debug)]
struct Formats {
    #[field(type = "email")]
    email: Option<String>,
    #[field(type = "email", multiple)]
    recipients: Option<String>,
    #[field(type = "url")]
    website: Option<String>,
    #[field(type = "date")]
    day: Option<String>,
    #[field(type = "time")]
    at: Option<String>,
    #[field(type = "datetime-local")]
    starts: Option<String>,
    #[field(type = "month")]
    month: Option<String>,
    #[field(type = "week")]
    week: Option<String>,
    #[field(type = "color")]
    colour: Option<String>,
}

#[track_caller]
fn accepts(pair: &str) {
    if let Err(errors) = Formats::from_urlencoded(pair) {
        panic!("`{pair}` should have been accepted, got: {errors}");
    }
}

#[track_caller]
fn rejects(pair: &str) {
    let field = pair.split('=').next().unwrap();
    match Formats::from_urlencoded(pair) {
        Ok(_) => panic!("`{pair}` should have been rejected"),
        Err(errors) => assert!(
            matches!(
                errors.field(field).next().map(|e| &e.kind),
                Some(ErrorKind::Invalid { .. })
            ),
            "`{pair}` was rejected, but not as a malformed value: {errors}"
        ),
    }
}

#[test]
fn the_control_type_is_re_checked_on_the_server() {
    // A browser would never submit these, so a submission that does is not
    // coming from the form we rendered.
    accepts("email=ada@example.com");
    accepts("email=a.b%2Bc@sub.example.co.uk"); // %2B is a literal `+`
    rejects("email=nope");
    rejects("email=a@@b.com");
    rejects("email=@example.com");
    rejects("email=ada@-example.com");

    accepts("recipients=a@example.com,+b@example.com");
    rejects("recipients=a@example.com,nope");

    accepts("website=https://example.com/x?y=1");
    accepts("website=mailto:ada@example.com");
    rejects("website=example.com");
    rejects("website=https://exa mple.com");

    accepts("colour=%23ff8800");
    rejects("colour=ff8800");
    rejects("colour=%23ff88");
}

#[test]
fn date_and_time_formats_are_checked_as_calendars_not_just_digits() {
    accepts("day=2026-02-28");
    accepts("day=2024-02-29"); // a leap year
    rejects("day=2026-02-29"); // not one
    rejects("day=2026-13-01");
    rejects("day=2026-2-01");

    accepts("at=09:30");
    accepts("at=09:30:15.500");
    rejects("at=24:00");
    rejects("at=09:60");

    accepts("starts=2026-06-15T09:30");
    rejects("starts=2026-06-15");

    accepts("month=2026-06");
    rejects("month=2026-00");

    accepts("week=2026-W01");
    accepts("week=2026-W53"); // 2026 is a 53-week year
    rejects("week=2025-W53"); // 2025 is not
    rejects("week=2026-W54");
}
