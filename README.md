# html-form

[![docs.rs](https://img.shields.io/docsrs/html-form)](https://docs.rs/html-form)
[![crates.io](https://img.shields.io/crates/v/html-form)](https://crates.io/crates/html-form)

> **This crate is AI generated.** A language model wrote the code, the tests and
> this document. A person reviewed the result, but no such review catches
> everything. Read the source before you depend on it, and treat every claim
> here as something to verify rather than something to trust.

Declarative HTML forms for Rust. One struct describes a form. The crate renders
the form and parses the submission back into the struct. When validation fails,
you get the same form again, with the user's values and the error messages
attached.

```rust
use html_form::{Form, Outcome};

#[derive(Form)]
#[form(action = "/signup", method = "post", submit = "Create account")]
struct Signup {
    #[field(type = "email", label = "Email address", autocomplete = "email")]
    email: String,

    #[field(type = "password", label = "Password", minlength = 12,
            help = "At least 12 characters.")]
    password: String,

    #[field(label = "Age", min = 18, max = 120)]
    age: Option<u32>,

    #[field(label = "Subscribe to the newsletter", default = true)]
    newsletter: bool,
}

// GET /signup — a blank form, with each field showing its default.
let html = Signup::render().to_html();
assert!(html.contains(r#"<input type="email" name="email""#));

// POST /signup with a bad body: every problem is reported at once.
match Signup::submit_urlencoded("email=nope&password=short&age=7") {
    Outcome::Valid(_) => unreachable!(),
    Outcome::Invalid { errors, view } => {
        // A bad address, a short password, an age below the minimum.
        assert_eq!(errors.len(), 3);
        // The re-render keeps what the user typed…
        assert_eq!(view.field("email").unwrap().value.as_deref(), Some("nope"));
        // …and carries the messages.
        assert!(view.field("age").unwrap().errors[0].contains("18"));
    }
}

// And a good one.
let signup = Signup::from_urlencoded(
    "email=a@example.com&password=correct-horse-battery&age=30&newsletter=on",
)
.unwrap();
assert_eq!(signup.age, Some(30));
assert!(signup.newsletter);
```

## What it does

| | | |
|---|---|---|
| **Render** | `Signup::render()` gives a `FormView`. It is flat and `serde::Serialize`, ready for MiniJinja or Askama. `.to_html()` gives markup with no further work | [docs](https://docs.rs/html-form/latest/html_form/guide/index.html#rendering) |
| **Parse** | `Signup::from_urlencoded(body)` gives `Result<Signup, FormErrors>` | [docs](https://docs.rs/html-form/latest/html_form/struct.Values.html) |
| **Validate** | The server checks every HTML attribute again: `required`, `pattern`, `minlength`, `min`, `step`, and the control's own type | [docs](https://docs.rs/html-form/latest/html_form/guide/index.html#server-side-validation) |
| **Collect** | Validation does not stop at the first failure. One pass reports every problem on every field | [docs](https://docs.rs/html-form/latest/html_form/guide/index.html#collecting-every-error) |
| **Re-render** | `Outcome::Invalid` gives back a `FormView` with the submitted values and the messages for each field | [docs](https://docs.rs/html-form/latest/html_form/enum.Outcome.html) |
| **Reuse** | `#[field(flatten)]` puts one form into another. A name prefix is optional | [docs](https://docs.rs/html-form/latest/html_form/guide/index.html#reuse-flattening-one-form-into-another) |
| **Localize** | Every label, help text, placeholder, legend and option label takes an i18n key in place of the text | [docs](https://docs.rs/html-form/latest/html_form/guide/index.html#localization) |
| **Context** | `#[form(context = Session)]` gives the session to the form's own `default` and `validate` functions. A connection or the clock works the same way | [docs](https://docs.rs/html-form/latest/html_form/guide/index.html#what-the-forms-own-functions-receive) |

Beyond the checks HTML can express, `validate = ...` names a function: per field,
or once over the whole struct. A type can also carry its own control and its own
rules with `#[derive(FormValue)]`, and any `FromStr + Display` type — a `Uuid`, a
`NaiveDate` — becomes a field with `#[field(from_str)]` and no impl at all.

## The rest of the documentation

Everything above is the summary. The full guide lives on
[docs.rs](https://docs.rs/html-form), where every example is a doctest:

- [Checks the markup cannot express](https://docs.rs/html-form/latest/html_form/guide/index.html#checks-the-markup-cannot-express)
  — `validate = ...` on a field and on the form
- [Localization](https://docs.rs/html-form/latest/html_form/guide/index.html#localization)
  — `t("key")` anywhere a person-facing string goes
- [Reuse](https://docs.rs/html-form/latest/html_form/guide/index.html#reuse-flattening-one-form-into-another)
  — `#[field(flatten)]`, prefixes and generic wrapper forms
- [Rendering](https://docs.rs/html-form/latest/html_form/guide/index.html#rendering)
  — the built-in renderer, MiniJinja, Askama, runtime choices, what it costs
- [Attributes](https://docs.rs/html-form/latest/html_form/guide/index.html#attributes)
  — every `#[form(...)]` and `#[field(...)]` key, and `attr(...)` for the rest
- [Types](https://docs.rs/html-form/latest/html_form/guide/index.html#types)
  — which Rust type gives which control, `FormChoice`, `FormValue`, `from_str`
- [Controls](https://docs.rs/html-form/latest/html_form/guide/index.html#controls-attributes-that-cannot-be-misplaced)
  — why `#[field(type = "date", minlength = 3)]` is a compile error
- [Server-side validation](https://docs.rs/html-form/latest/html_form/guide/index.html#server-side-validation)
  and [defaults](https://docs.rs/html-form/latest/html_form/guide/index.html#defaults-blank-values-and-checkboxes)
- [Context](https://docs.rs/html-form/latest/html_form/guide/index.html#what-the-forms-own-functions-receive)
  — a session, a connection or the clock reaching the form's own functions
- [Framework integration](https://docs.rs/html-form/latest/html_form/guide/index.html#framework-integration)
  — `Values::from_pairs`, JSON bodies, and the two
  [axum](https://docs.rs/html-form/latest/html_form/guide/index.html#axum) extractors

The API entry points are
[`Form`](https://docs.rs/html-form/latest/html_form/trait.Form.html),
[`FormView`](https://docs.rs/html-form/latest/html_form/struct.FormView.html),
[`Outcome`](https://docs.rs/html-form/latest/html_form/enum.Outcome.html) and
[`FormErrors`](https://docs.rs/html-form/latest/html_form/struct.FormErrors.html).

The `examples/` directory runs the same ground: `minijinja_render.rs` is a whole
request cycle through a template engine, `csrf.rs` a generic `WithCsrf<T>`
wrapper, and `axum_signup.rs` an application built on the axum extractor.

## Feature flags

| Feature | Default | What it does |
|---|---|---|
| `derive` | on | `#[derive(Form)]`, `#[derive(FormValue)]`, `#[derive(FormChoice)]` |
| `html` | on | The built-in renderer: `to_html`, the `Display` impls and `escape` |
| `pattern` | on | Server-side `pattern` checking with `regex-lite`. Without it, the crate still renders `pattern`, and the browser enforces it |
| `axum` | off | `Outcome<T>` and `axum::Form<T, R>` are axum 0.8 extractors |

## License

Licensed under either of
[Apache License, Version 2.0](https://github.com/lperlaki/html-form-rs/blob/main/LICENSE-APACHE)
or [MIT license](https://github.com/lperlaki/html-form-rs/blob/main/LICENSE-MIT)
at your option. Unless you explicitly state otherwise,
any contribution intentionally submitted for inclusion in this crate by you, as
defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
