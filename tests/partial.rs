//! Saving one field while another is still being typed.

use html_form::{Form, FormErrors, Outcome, Values};

#[derive(Form, Debug, PartialEq, Clone)]
struct Settings {
    greeting: String,
    #[field(min = 1024)]
    listen_port: u16,
    verbose: bool,
    #[field(flatten, prefix = "log_")]
    log: Log,
    servers: Vec<String>,
}

#[derive(Form, Debug, PartialEq, Clone)]
struct Log {
    path: String,
}

fn current() -> Settings {
    Settings {
        greeting: "hello".into(),
        listen_port: 8080,
        verbose: true,
        log: Log {
            path: "/var/log".into(),
        },
        servers: vec!["a".into(), "b".into()],
    }
}

#[test]
fn a_half_typed_field_does_not_block_the_one_being_saved() {
    let values = Values::parse("greeting=&listen_port=9000");
    let Outcome::Valid(saved) = current().submit_partial(&values, &["listen_port"]) else {
        panic!("only the port was named");
    };
    assert_eq!(saved.listen_port, 9000);
    assert_eq!(saved.greeting, "hello");
}

#[test]
fn a_named_field_is_validated() {
    let values = Values::parse("listen_port=80");
    let Outcome::Invalid { errors, .. } = current().submit_partial(&values, &["listen_port"])
    else {
        panic!("80 is below the minimum");
    };
    assert_eq!(errors.len(), 1);
    assert!(errors.has_field("listen_port"));
}

/// An unticked box submits nothing, and that is a change like any other.
#[test]
fn a_named_field_that_was_not_sent_is_cleared() {
    let Outcome::Valid(saved) = current().submit_partial(&Values::new(), &["verbose"]) else {
        panic!("a box may be unticked");
    };
    assert!(!saved.verbose);
    assert_eq!(saved.listen_port, 8080);
}

#[test]
fn a_list_is_replaced_whole_and_a_prefix_is_part_of_the_name() {
    let values = Values::parse("servers=c&log_path=/tmp");
    let Outcome::Valid(saved) = current().submit_partial(&values, &["servers", "log_path"]) else {
        panic!("both are fine");
    };
    assert_eq!(saved.servers, ["c"]);
    assert_eq!(saved.log.path, "/tmp");
}

fn ports_differ(form: &Pair) -> Result<(), FormErrors> {
    match form.a == form.b {
        true => Err(("b", "They must differ.").into()),
        false => Ok(()),
    }
}

/// A check across fields still sees the whole form.
#[test]
fn a_form_check_runs_on_the_merged_value() {
    let pair = Pair { a: 1, b: 2 };
    let outcome = pair.submit_partial(&Values::parse("b=1"), &["b"]);
    assert!(!outcome.is_valid());
}

#[derive(Form, Debug)]
#[form(validate = ports_differ)]
struct Pair {
    a: u16,
    b: u16,
}

/// The parse fills a skipped field in, and only the caller knows what it held.
#[test]
fn a_skipped_field_comes_back_as_its_default() {
    #[derive(Form, Debug)]
    struct Extra {
        name: String,
        #[field(skip)]
        extra: Vec<String>,
    }
    let stored = Extra {
        name: "a".into(),
        extra: vec!["keep".into()],
    };
    let Outcome::Valid(saved) = stored.submit_partial(&Values::parse("name=b"), &["name"]) else {
        panic!("a name is enough");
    };
    assert!(saved.extra.is_empty());
}
