#![allow(dead_code)]

//! `#[derive(FormValue)]`: what a type carries so that no field of it has to.

use html_form::{
    Choice, Control, ErrorKind, FieldKind, Form, FormValue, Text, TextFormat, ValueError,
};

#[derive(FormValue, Debug, PartialEq)]
#[value(type = "email", maxlength = 254, validate = is_company_address)]
struct WorkEmail(String);

fn is_company_address(email: &WorkEmail) -> Result<(), Text> {
    match email.0.ends_with("@example.com") {
        true => Ok(()),
        false => Err(Text::key("invite.email.outside")),
    }
}

/// A named single field is the same thing written differently.
#[derive(FormValue, Debug, PartialEq)]
#[value(type = "range", min = 0, max = 100, step = 5, default = 50)]
struct Percent {
    value: u8,
}

#[derive(FormValue, Debug, PartialEq)]
#[value(validate = is_slug)]
struct Slug(String);

fn is_slug(slug: &Slug) -> bool {
    !slug.0.is_empty() && slug.0.chars().all(|c| c.is_ascii_lowercase() || c == '-')
}

#[derive(Form, Debug)]
struct Invite {
    colleague: WorkEmail,
    handle: Slug,
    #[field(label = "Progress")]
    progress: Percent,
}

#[test]
fn the_type_settles_the_control_and_what_it_constrains() {
    assert!(matches!(
        WorkEmail::CONTROL,
        Control::Text(text)
            if text.format == TextFormat::Email { multiple: false }
                && text.maxlength == Some(254)
    ));

    // The wrapped type's own bounds survive the merge: `u8` implies `step = 1`
    // and a floor of 0, and `#[value(...)]` narrows from there.
    let Control::Number(number) = Percent::CONTROL else {
        panic!("a range is a number control");
    };
    assert_eq!(number.bounds.min, Some("0"));
    assert_eq!(number.bounds.max, Some("100"));
    assert_eq!(number.bounds.step, Some("5"));
}

#[test]
fn a_field_of_the_type_needs_none_of_it_repeated() {
    let view = Invite::render();

    let colleague = view.field("colleague").unwrap();
    assert_eq!(colleague.kind, FieldKind::Email);
    assert_eq!(colleague.maxlength, Some(254));
    // The label is still the field's own business.
    assert_eq!(colleague.label.as_deref(), Some("Colleague"));

    let progress = view.field("progress").unwrap();
    assert_eq!(progress.kind, FieldKind::Range);
    assert_eq!(progress.value.as_deref(), Some("50"));
    assert_eq!(progress.min.as_deref(), Some("0"));
    assert_eq!(progress.max.as_deref(), Some("100"));
}

/// A type's default is the field's default, so it follows the same rule: it
/// fills a blank form, and a parse still reads what arrived.
#[test]
fn the_types_default_fills_a_blank_form_and_no_more() {
    assert_eq!(
        Invite::render().field("progress").unwrap().value.as_deref(),
        Some("50")
    );

    let errors = Invite::from_urlencoded("colleague=ada@example.com&handle=ada")
        .expect_err("progress is absent");
    assert!(errors.has_field("progress"));

    let invite = Invite::from_urlencoded("colleague=ada@example.com&handle=ada&progress=50")
        .expect("every field arrived");
    assert_eq!(invite.progress, Percent { value: 50 });
}

#[test]
fn a_field_may_override_what_the_type_says() {
    #[derive(Form, Debug)]
    struct Override {
        // Narrower than the type asks for, and pre-filled where it asked for
        // nothing — the control it named is still the one that renders.
        #[field(maxlength = 40, default = "nobody@example.com")]
        colleague: WorkEmail,
        // The control itself, where a form has a use for the value the type
        // describes but not for the box it describes.
        #[field(type = "hidden")]
        referrer: WorkEmail,
    }

    let view = Override::render();
    let colleague = view.field("colleague").unwrap();
    assert_eq!(colleague.kind, FieldKind::Email);
    assert_eq!(colleague.maxlength, Some(40));
    assert_eq!(colleague.value.as_deref(), Some("nobody@example.com"));
    assert_eq!(view.field("referrer").unwrap().kind, FieldKind::Hidden);
}

#[test]
fn the_types_own_check_runs_wherever_it_is_used() {
    let errors = Invite::from_urlencoded("colleague=ada@example.org&handle=Not+A+Slug&progress=50")
        .unwrap_err();

    assert_eq!(errors.len(), 2);
    // A keyed message doubles as the code, as it does for a field's validator.
    assert_eq!(
        errors.field("colleague").next().unwrap().code(),
        Some("invite.email.outside")
    );
    // A predicate says only that the value is unacceptable.
    let handle = errors.field("handle").next().unwrap();
    assert_eq!(handle.kind, ErrorKind::Custom { code: None });
}

#[test]
fn the_constraints_the_type_carries_are_re_checked_on_the_server() {
    let errors = Invite::from_urlencoded("colleague=nope&handle=ada&progress=50").unwrap_err();
    // The control's own format check, from a `type` nothing but the type named.
    assert!(matches!(
        errors.field("colleague").next().unwrap().kind,
        ErrorKind::Invalid { .. }
    ));

    let errors =
        Invite::from_urlencoded("colleague=ada@example.com&handle=ada&progress=52").unwrap_err();
    assert!(matches!(
        errors.field("progress").next().unwrap().kind,
        ErrorKind::Step { .. }
    ));
}

#[test]
fn a_check_that_passes_leaves_the_value_alone() {
    let invite = Invite::from_urlencoded("colleague=ada@example.com&handle=ada&progress=25")
        .expect("every check passes");
    assert_eq!(invite.colleague, WorkEmail("ada@example.com".to_owned()));
    assert_eq!(invite.handle, Slug("ada".to_owned()));
    assert_eq!(invite.progress, Percent { value: 25 });
}

#[test]
fn a_value_is_written_back_out_through_the_type_it_wraps() {
    let invite = Invite {
        colleague: WorkEmail("ada@example.com".to_owned()),
        handle: Slug("ada".to_owned()),
        progress: Percent { value: 25 },
    };
    let values = invite.to_values();
    assert_eq!(values.get("colleague"), Some("ada@example.com"));
    assert_eq!(values.get("progress"), Some("25"));

    // And back in, so an edit form round-trips.
    let view = invite.render_filled();
    assert_eq!(view.field("progress").unwrap().value.as_deref(), Some("25"));
}

#[test]
fn converting_and_checking_are_separate_questions() {
    // The conversion is the wrapped type's, and answers only whether the string
    // could become a value at all.
    assert_eq!(
        Slug::parse_form_value("Not A Slug"),
        Ok(Slug("Not A Slug".to_owned()))
    );
    assert!(
        Slug::parse_form_value("Not A Slug")
            .unwrap()
            .validate_form_value()
            .is_err()
    );
    assert!(Percent::parse_form_value("300").is_err());
}

#[test]
fn a_set_of_options_travels_with_the_type_too() {
    const COUNTRIES: &[Choice] = &[
        Choice::new("de", "Germany"),
        Choice::new("ch", "Switzerland"),
    ];

    #[derive(FormValue, Debug)]
    #[value(choices = COUNTRIES, type = "radio")]
    struct Country(String);

    #[derive(Form, Debug)]
    struct Ship {
        country: Country,
    }

    let view = Ship::render();
    assert_eq!(view.field("country").unwrap().kind, FieldKind::Radio);
    assert_eq!(view.field("country").unwrap().choices.len(), 2);

    // And what is not one of them is rejected, wherever the type is used.
    assert!(Ship::from_urlencoded("country=de").is_ok());
    let errors = Ship::from_urlencoded("country=fr").unwrap_err();
    assert_eq!(
        errors.field("country").next().unwrap().kind,
        ErrorKind::NotAChoice
    );
}

#[test]
fn a_wrapper_may_be_generic() {
    #[derive(FormValue, Debug, PartialEq)]
    struct Trimmed<T>(T);

    #[derive(Form, Debug)]
    struct Note {
        title: Trimmed<String>,
        seats: Trimmed<u32>,
    }

    // Each instantiation resolves the control of what it wraps.
    assert!(matches!(
        <Trimmed<u32> as FormValue>::CONTROL,
        Control::Number(_)
    ));
    let note = Note::from_urlencoded("title=Hello&seats=3").unwrap();
    assert_eq!(note.seats, Trimmed(3));
}

#[test]
fn a_hand_written_impl_still_needs_only_the_two_conversions() {
    struct Bare(String);

    impl FormValue for Bare {
        const CONTROL: Control = Control::TEXT;

        fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
            Ok(Bare(raw.to_owned()))
        }

        fn to_form_value(&self) -> std::borrow::Cow<'static, str> {
            std::borrow::Cow::Owned(self.0.clone())
        }
    }

    assert_eq!(Bare::DEFAULT, None);
    assert!(Bare("anything".to_owned()).validate_form_value().is_ok());
}
