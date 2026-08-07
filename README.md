# html-form

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

| | |
|---|---|
| **Render** | `Signup::render()` gives a `FormView`. It is flat and `serde::Serialize`, ready for MiniJinja or Askama. `.to_html()` gives markup with no further work |
| **Parse** | `Signup::from_urlencoded(body)` gives `Result<Signup, FormErrors>` |
| **Validate** | The server checks every HTML attribute again: `required`, `pattern`, `minlength`, `min`, `step`, and the control's own type |
| **Collect** | Validation does not stop at the first failure. One pass reports every problem on every field |
| **Re-render** | `Outcome::Invalid` gives back a `FormView` with the submitted values and the messages for each field |
| **Reuse** | `#[field(flatten)]` puts one form into another. A name prefix is optional |
| **Localize** | Every label, help text, placeholder, legend and option label takes an i18n key in place of the text |
| **Context** | `#[form(context = Session)]` gives the session to the form's own `default` and `validate` functions. A connection or the clock works the same way |

## Collecting every error

The crate parses each field on its own, so one round trip tells the user
everything that is wrong:

```rust
# use html_form::Form;
# #[derive(Form, Debug)]
# struct Signup {
#     #[field(type = "email")]
#     email: String,
#     #[field(type = "password", minlength = 12)]
#     password: String,
#     #[field(min = 18, max = 120)]
#     age: Option<u32>,
# }
let body = "email=nope&password=short&age=7";
let errors = Signup::from_urlencoded(body).unwrap_err();

assert_eq!(errors.len(), 3);
for (field, error) in errors.iter() {
    println!("{field}: {}", error.message.as_str());
}
// email:    Enter a valid email address.
// password: Enter at least 12 characters (currently 5).
// age:      Must be 18 or more.
```

Each error carries a typed `ErrorKind` next to the built-in English message. You
can match on `ErrorKind::TooShort { minlength, length }` and write your own text,
translated or not. `FormErrors` also serializes to
`{"form": [...], "fields": {"email": [...]}}` for JSON APIs.

## Checks the markup cannot express

`validate = ...` names a function. The crate runs it after every check the spec
could make: per field with `#[field(...)]`, or once over the whole struct with
`#[form(...)]`. A predicate is enough when the built-in message will do. Return a
`Result` to say more.

```rust
use html_form::{Form, FormErrors};

#[derive(Form, Debug)]
#[form(validate = passwords_match)]
struct Signup {
    #[field(validate = is_available)]
    username: String,
    #[field(type = "password")]
    password: String,
    #[field(type = "password")]
    confirm: String,
}

fn is_available(name: &String) -> bool {
    name != "admin"
}

fn passwords_match(form: &Signup) -> Result<(), FormErrors> {
    if form.password == form.confirm {
        Ok(())
    } else {
        // Attach it to the field the user can correct, not to the form.
        Err(("confirm", "The two passwords do not match.").into())
    }
}

let errors =
    Signup::from_urlencoded("username=admin&password=a&confirm=b").unwrap_err();
assert!(errors.has_field("username") && errors.has_field("confirm"));
```

A field validator may return a `bool`, a `Result<(), &str | String | Cow>`, a
`Result<(), Text>` for an i18n key, or a `Result<(), FieldError>`. A form
validator takes those and two more that name a field: a `(field, message)` pair,
or a whole `FormErrors` it built itself.

## Localization

Where the crate accepts a string a person reads, `t("…")` names an i18n key in
place of the text:

```rust
# use html_form::Form;
#[derive(Form)]
#[form(submit = t("signup.submit"))]
struct Signup {
    #[field(type = "email",
            label = t("signup.email.label"),
            help = t("signup.email.help"),
            placeholder = t("signup.email.placeholder"))]
    email: String,

    #[field(label = "Age")]                     // still plain text
    age: Option<u32>,

    #[field(label = t("signup.country"))]
    #[option("de", t("country.de"))]
    #[option("ch", t("country.ch"), group = t("country.non-eu"))]
    country: String,
}
```

`label`, `help`, `placeholder`, `legend` and `submit` take either form. So do
the labels and the `group`s of `#[option(...)]` and `#[choice(...)]`. In the
spec, `Text` keeps the two apart, and the render format keeps the difference.

The crate resolves no key itself. It has no more opinion about your i18n stack
than about your HTTP stack. Give it a lookup and it walks the view:

```rust
# use html_form::Form;
# #[derive(Form)]
# #[form(submit = t("signup.submit"))]
# struct Signup {
#     #[field(type = "email", label = t("signup.email"), help = "Never shared.")]
#     email: String,
# }
let view = Signup::render_localized(|key| match key {
    "signup.email" => Some("E-Mail-Adresse"),
    "signup.submit" => Some("Konto erstellen"),
    _ => None,
});

assert_eq!(view.field("email").unwrap().label.as_deref(), Some("E-Mail-Adresse"));
assert_eq!(view.submit_label, "Konto erstellen");
// A literal is a literal, whatever language the rest is in.
assert_eq!(view.field("email").unwrap().help.as_deref(), Some("Never shared."));
```

`localized` does the same to a view from anywhere else, so
`article.render_filled().localized(&translate)` and
`outcome.view().unwrap().localized(&translate)` work the same way. `translate` is
any `Fn(&str) -> Option<impl Into<Cow<'static, str>>>`, so a lookup that returns
`&'static str` costs nothing to apply.

Or leave the work to the template. Every translatable string has a companion
`…_key`. The crate sets it only while the key is unresolved:

```jinja
{{ field.label_key and t(field.label_key) or field.label }}
```

Until something resolves a key, the string *is* the key. `{{ field.label }}`
therefore renders text either way, and a key that no backend knows stays visible
in place of a blank label. A reader can report that bug. After the crate
resolves a key, it clears the `…_key`, so nothing translates twice.

`Choice::keyed("de", "country.de")` and `Choice::owned_keyed(id, key)` build
keyed options at render time, next to `Choice::new` and `Choice::owned`.

A `validate = ...` function localizes the same way. Return a `Text::key`, and the
crate resolves the message with everything else. The key is also the error's
code, so a caller that would rather build its own message can match on that
instead. The messages of the *built-in* checks are English and carry no key on
purpose: every `ErrorKind` names the constraint the value broke, so a caller that
needs a translation matches on the kind and writes its own text. There is no key
to guess at, and no message table to keep in step.

## Reuse: flattening one form into another

You can put any `Form` inside another. Without a prefix, the sub-form shares the
parent's namespace. With a prefix, you can use the same sub-form more than once.

```rust
use html_form::Form;

#[derive(Form)]
struct Address {
    #[field(label = "Street")]
    street: String,
    #[field(label = "Postcode", pattern = r"\d{4,5}")]
    postcode: String,
}

#[derive(Form)]
struct Order {
    #[field(label = "Customer")]
    customer: String,

    #[field(flatten, prefix = "billing_", legend = "Billing address")]
    billing: Address,

    #[field(flatten, prefix = "shipping_", legend = "Shipping address")]
    shipping: Address,
}

let view = Order::render();
let names: Vec<&str> = view.fields.iter().map(|f| f.name.as_ref()).collect();
assert_eq!(
    names,
    ["customer", "billing_street", "billing_postcode",
     "shipping_street", "shipping_postcode"]
);

let order = Order::from_urlencoded(
    "customer=Ada&billing_street=Main+1&billing_postcode=12345\
     &shipping_street=Side+2&shipping_postcode=54321",
)
.unwrap();
assert_eq!(order.shipping.postcode, "54321");
```

The browser submits the fields under the prefixed names, and the errors use the
same ones. Each group renders inside its own `<fieldset>` with the given legend.
`Address` is still a good form on its own. Nothing about it had to change.

A form may be generic, which is what makes a *wrapper* possible. `Form::SPEC` is
an associated constant, so the compiler resolves `<T as Form>::SPEC` once per
instantiation, and the derive adds the bounds each field implies. A flatten
brings in the sub-form's fields alone: its `action`, `method` and submit label
describe its own `<form>` element, so a wrapper declares the ones it wants.

```rust
use html_form::Form;

#[derive(Form)]
#[form(method = "post")]
struct WithCsrf<T> {
    #[field(type = "hidden", default = fresh_token)]
    csrf_token: String,

    #[field(flatten)]
    inner: T,
}

#[derive(Form)]
struct Signup {
    #[field(type = "email")]
    email: String,
}

fn fresh_token() -> String {
    // A real one comes from a CSPRNG, and the session remembers it.
    "3f9c…".to_owned()
}

let view = WithCsrf::<Signup>::render();
let names: Vec<&str> = view.fields.iter().map(|f| f.name.as_ref()).collect();
assert_eq!(names, ["csrf_token", "email"]);
assert_eq!(view.field("csrf_token").unwrap().value.as_deref(), Some("3f9c…"));
```

## Rendering

`FormView` is the render format: one flat, fully resolved struct per form, with
one `FieldView` per control. Nothing in it is specific to Rust, so a template
author never has to know what an enum is.

### Built in

```rust
# use html_form::Form;
# #[derive(Form)]
# struct Signup { #[field(type = "email")] email: String }
println!("{}", Signup::render());
```

This gives plain, unstyled markup with `html-form__*` class hooks and correct
`for`/`id` pairs. It points `aria-invalid` and `aria-describedby` at the help
text and the error list. Flattened groups get a `<fieldset>`, and grouped
choices get an `<optgroup>`.

This is the `html` feature, on by default. If you render through a template
engine, turn the feature off. You lose `to_html`, the `Display` impls and
`escape`. Nothing else changes, `FormView` included.

### MiniJinja

`FormView` is `Serialize`, so it goes straight into a context:

```rust
use minijinja::{Environment, context};
# use html_form::Form;
# #[derive(Form)]
# struct Signup { #[field(type = "email", label = "Email address")] email: String }
# let mut env = Environment::new();
# env.add_template("signup.html", "{% for f in form.fields %}{{ f.label }}{% endfor %}")?;
# let template = env.get_template("signup.html")?;
let html = template.render(context! { form => Signup::render() })?;
# assert_eq!(html, "Email address");
# Ok::<(), Box<dyn std::error::Error>>(())
```

```jinja
{% for field in form.fields %}
  <div class="field{% if field.has_errors %} field--invalid{% endif %}">
    {% if field.label %}<label for="{{ field.id }}">{{ field.label }}</label>{% endif %}
    {% if field.element == "select" %}
      <select name="{{ field.name }}" id="{{ field.id }}">
        {% for choice in field.choices %}
        <option value="{{ choice.value }}"{% if choice.selected %} selected{% endif %}>{{ choice.label }}</option>
        {% endfor %}
      </select>
    {% else %}
      <input type="{{ field.input_type }}" name="{{ field.name }}" id="{{ field.id }}"
             value="{{ field.value or "" }}"{% if field.required %} required{% endif %}>
    {% endif %}
    {% for message in field.errors %}<p class="error">{{ message }}</p>{% endfor %}
  </div>
{% endfor %}
```

`examples/minijinja_render.rs` is the whole request cycle, and it runs in CI:
`cargo run --example minijinja_render`.

### Askama

Every field of `FormView` and `FieldView` is public, so an Askama template walks
the same structure directly:

```html
{% for field in form.fields %}
  <div class="field">
    {% if let Some(label) = field.label %}
      <label for="{{ field.id }}">{{ label }}</label>
    {% endif %}
    {{ field.control_html()|safe }}
    {% for message in field.errors %}<p class="error">{{ message }}</p>{% endfor %}
  </div>
{% endfor %}
```

`FieldView::control_html()` renders one control on its own.
`FieldView::to_html()` renders the control with its label, help text and errors.
You can therefore mix your own markup with generated controls at the level that
suits you.

### Options that only exist at runtime

The view is owned and mutable, which is where a list from the database goes:

```rust
use html_form::{Choice, Form};

# struct Room { id: String, name: String }
# let rooms = vec![Room { id: "a1".into(), name: "Attic".into() }];
#[derive(Form)]
struct Booking {
    #[field(type = "select")]
    room: String,
}

let mut view = Booking::render();
view.field_mut("room")
    .unwrap()
    .set_choices(rooms.iter().map(|r| Choice::owned(&r.id, &r.name)));

assert_eq!(view.field("room").unwrap().choices[0].label, "Attic");
```

`view.add_field_error("email", "That address is already registered.")` adds the
errors that only your database knows about.

### What rendering costs

A spec is `const`. `Form::SPEC` is one const-evaluated value, flattened
sub-forms included. The first render of a form therefore builds nothing, and
reaching a sub-form's description costs no call.

Every string in the render format is a `Cow<'static, str>`, and rendering
borrows from the spec where it can. A blank form allocates only what the spec
does not hold:

| Borrowed from the spec | Built per render |
|---|---|
| names, labels, help text, placeholders | the three ids derived from each field's name |
| options, constraints, custom attributes | the `Vec`s of fields and of options |
| defaults, and the built-in error messages | the values a user submitted, which do not outlive the request |

The owned half of each `Cow` keeps the view an ordinary owned value that a
handler can return. It also holds the runtime strings that `set_value`,
`set_choices` and `localize` put in. Serialization does not change: a `Cow`
serializes as the string it holds.

Parsing works the same way. A field's name reaches `FormErrors` borrowed from
the spec. Only `#[field(flatten, prefix = "…")]` makes a name that the crate has
to build.

A context costs one `&dyn Any` passed down the walk, and nothing else. A default
costs one indirect call and one `TypeId` comparison, made where the render reads
the value and nowhere else. There is no pass over the form to collect defaults
first, so a form that declares none pays nothing for the mechanism.

## Attributes

### `#[form(...)]` — on the struct

| | |
|---|---|
| `id`, `name`, `action`, `class` | The crate renders these onto the `<form>` |
| `method = "post"` | `get`, `post` or `dialog` |
| `enctype = "multipart/form-data"` | For file uploads |
| `novalidate` | Turn off the browser's own validation. The server still validates |
| `submit = "Create account"` | Caption of the built-in submit button. Use `submit = t("key")` for an i18n key |
| `validate = path::to::fn` | Cross-field check: `fn(&Self) -> Result<(), E>`, or `fn(&Self, &Context) -> Result<(), E>` |
| `context = Session` | What the crate gives the form's own functions, and what every `…_with_context` call takes. Defaults to `()` |
| `attr("hx-post" = "/signup")` | Anything the crate has no opinion about. The crate renders it as written |
| `renderer`, `status`, `from_request`, `into_response` | With the `axum` feature: the struct is its own extractor and response. See [axum](#axum) |

### `#[field(...)]` — on a field

| | |
|---|---|
| `type = "email"` | The control. The crate infers it from the Rust type otherwise |
| `label`, `placeholder`, `help`, `autocomplete`, `id`, `class`, `rows`, `cols` | Presentation. `label` defaults to the humanized field name. `label = ""` renders no label |
| `label = t("key")` | `label`, `help`, `placeholder` and `legend` also take an i18n key |
| `name = "e-mail"` | The submitted name. Defaults to the field name |
| `required` / `optional` | Overrides the inferred default |
| `default = "…"` | Value shown on a blank form. Never a fallback while parsing |
| `default` | The same, taken from the field type's own `Default::default()` |
| `default = path::to::fn` | A new value for each render: a token, a nonce, today's date. Write `fn() -> FieldType`, or `fn(&Context) -> FieldType`, or either one returning something that converts into it |
| `reset` / `reset = false` | Show the default on every render, and never what was submitted or stored. For a field the form owns rather than the user: a token, a password box. A hidden field with `default = path::to::fn` resets untold, and `reset = false` turns that off |
| `pattern`, `minlength`, `maxlength`, `min`, `max`, `step`, `accept` | Validation |
| `disabled`, `readonly`, `autofocus`, `multiple` | Flags |
| `choices = SOME_CONST` | A `&'static [Choice]` |
| `validate = path::to::fn` | Per-field check: `fn(&FieldType) -> Result<(), E>`, or `fn(&FieldType, &Context) -> Result<(), E>` |
| `from_str` | Convert with the type's own `FromStr` and `Display` in place of a `FormValue` impl |
| `attr("data-role" = "input")` | Anything the crate has no opinion about. The crate renders it as written |
| `flatten` (+ `prefix`, `legend`) | Put another form in |
| `skip` | Not part of the form. The crate fills it with `Default::default()` |

You can also list options inline:

```rust
# use html_form::Form;
#[derive(Form)]
struct Delivery {
    #[field(label = "Country")]
    #[option("de", "Germany")]
    #[option("ch", "Switzerland", group = "Non-EU")]
    country: String,
}
# assert_eq!(Delivery::render().field("country").unwrap().choices.len(), 2);
```

### Custom attributes

`attr(...)` carries the markup this crate knows nothing about: `data-*`, `hx-*`,
`aria-*`, and whatever else your front end reads.

```rust
use html_form::Form;

#[derive(Form)]
#[form(attr("hx-post" = "/search", "hx-target" = "#results"))]
struct Search {
    #[field(attr("hx-trigger" = "keyup changed delay:300ms", autocorrect = "off"))]
    query: String,

    #[field(attr(inert))]                     // a bare word is a boolean attribute
    sort: Option<String>,
}

let html = Search::render().to_html();
assert!(html.contains(r#"hx-post="/search""#));
assert!(html.contains(r#"autocorrect="off""#));
```

Write a dashed name as a string. The crate takes a bare word as written, so
`attr(data_role = "x")` renders `data_role`, not `data-role`. A value may be any
literal, and `"3"` and `3` mean the same thing. The crate escapes each entry
like every other value.

The spec holds the list in `FieldSpec::attrs`. It reaches the render format as
`field.attrs`, a `[{name, value}]` list in which a `null` value means a boolean
attribute. The crate renders the list after every attribute it generates itself.
Naming one of *those* is a compile error, not a duplicate attribute the browser
would ignore: `attr("class" = "x")` tells you to write `#[field(class = "x")]`.

`FormView::set_attr` and `FieldView::set_attr` set attributes at render time,
next to `set_choices`. A *value* set there may be anything, because the renderer
escapes it. A **name** sits outside the quotes, where escaping cannot help, so
both refuse a name markup could not carry and both refuse one the crate already
renders — the two the derive rejects at compile time. They return `false` and
set nothing in either case, so the refusal holds for a template engine reading
`field.attrs` exactly as it does for `to_html`. Build a name from a constant,
not from input; `html_form::is_attr_name` is the check itself, for a view you
edit some other way.

## Types

| Rust type | Control | Required by default |
|---|---|---|
| `String` | `text` | yes |
| `u8` … `i128`, `usize` | `number`. The type implies `min` and `step` | yes |
| `f32`, `f64` | `number`, `step="any"` | yes |
| `bool` | `checkbox`. An absent value means `false` | no |
| `Option<T>` | as `T` | no |
| `Vec<T>` | As `T`. The crate collects every value submitted under the name, and renders `multiple` on the controls that accept it (`select`, `email`, `file`) | no |
| `Vec<T>` + `type = "checkbox"` | one checkbox per option, sharing the name | no |
| `#[derive(FormChoice)] enum` | `select` with the variants as options | yes |
| `#[derive(FormValue)] struct Wrapper(T)` | as `T`, plus whatever `#[value(...)]` adds | yes |
| any `FromStr + Display` type, with `#[field(from_str)]` | `text`, until `type = "…"` says otherwise | yes |
| your own type | whatever its `FormValue` impl says | yes |

A fieldless enum becomes a `<select>`:

```rust
use html_form::{Form, FormChoice};

#[derive(FormChoice, Debug, PartialEq)]
enum Plan {
    Free,                                       // submits "free", labeled "Free"
    #[choice(value = "pro", label = "Professional")]
    Pro,
    #[choice(group = "Contact sales", disabled)]
    SelfHosted,                                 // submits "self-hosted"
}

#[derive(Form, Debug)]
struct Subscribe {
    plan: Plan,
}

assert_eq!(Subscribe::from_urlencoded("plan=pro").unwrap().plan, Plan::Pro);
// A `<select>` is a constraint, not a suggestion.
let errors = Subscribe::from_urlencoded("plan=enterprise").unwrap_err();
assert_eq!(
    errors.field("plan").next().unwrap().kind,
    html_form::ErrorKind::NotAChoice
);
```

### A type that carries its own rules

A check that the whole application makes of a value does not belong to one field
of one form. `#[derive(FormValue)]` puts the check on the type. The wrapped type
does the conversion, because it is already a `FormValue`, and `#[value(...)]`
says what the wrapper adds.

```rust
use html_form::{Form, Text};

#[derive(html_form::FormValue, Debug)]
#[value(type = "email", maxlength = 254, validate = is_company_address)]
struct WorkEmail(String);

fn is_company_address(email: &WorkEmail) -> Result<(), Text> {
    match email.0.ends_with("@example.com") {
        true => Ok(()),
        false => Err(Text::key("invite.email.outside")),
    }
}

#[derive(Form, Debug)]
struct Invite {
    #[field(label = "Who should we invite?")]
    colleague: WorkEmail,      // the control, the length, the check: all the type's
}

// The control, and every constraint on it, came from the type.
let view = Invite::render();
let field = view.field("colleague").unwrap();
assert_eq!(field.kind, html_form::FieldKind::Email);
assert_eq!(field.maxlength, Some(254));

// So did the check, which runs on every form that uses the type.
let errors = Invite::from_urlencoded("colleague=ada@example.org").unwrap_err();
assert_eq!(
    errors.field("colleague").next().unwrap().code(),
    Some("invite.email.outside")
);
```

| `#[value(...)]` | |
|---|---|
| `type = "email"` | The control that every field of this type renders as. The wrapped type's control otherwise |
| `pattern`, `minlength`, `maxlength`, `min`, `max`, `step`, `accept`, `rows`, `cols`, `multiple` | Constraints the type carries. The browser *and* the server enforce them |
| `choices = SOME_CONST` | A `&'static [Choice]`. The value has to be one of them |
| `default = "…"` | What a blank form shows for a field of this type |
| `validate = path::to::fn` | The type's own check: `fn(&Self) -> bool`, or `-> Result<(), E>`. These are the shapes `#[field(validate = ...)]` takes, i18n key included |
| `from_str` | Convert with the type's own `FromStr` and `Display` in place of the value it wraps |

A field can still override the control and the default, exactly as an attribute
overrides the control a Rust type implies. The check belongs to the type and
always runs, next to any check the field declares. What `#[value(...)]` will not
take is anything that describes the *field*, such as a label or a placeholder.
The same type is a "Work email" on one form and a "Recipient" on the next. A
validator here also takes no context: a `FormValue` belongs to no form, so a
check that needs a context belongs on the field.

You derive it for a struct with exactly one field, named or not, because a form
control submits one string. A struct with several fields is a form of its own.
Derive `Form` and put it in with `#[field(flatten)]`, unless the struct converts
itself. The next section covers that case.

### A type from a crate that has never heard of this one

`#[field(from_str)]` converts a field with the type's own `FromStr` and
`Display`. The crate asks nothing else of the type, so a `Uuid`, a `NaiveDate`
or a `Decimal` becomes a field with no impl and no newtype:

```rust
use html_form::Form;
# use std::fmt;
# use std::str::FromStr;
# #[derive(Debug, PartialEq)]
# struct NaiveDate(String);
# impl FromStr for NaiveDate {
#     type Err = &'static str;
#     fn from_str(raw: &str) -> Result<Self, Self::Err> { Ok(NaiveDate(raw.to_owned())) }
# }
# impl fmt::Display for NaiveDate {
#     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
# }

#[derive(Form, Debug)]
struct Booking {
    #[field(from_str, type = "date", min = "2026-01-01")]
    day: NaiveDate,          // and `Option<T>`, and `Vec<T>`
}

let booking = Booking::from_urlencoded("day=2026-08-06").unwrap();
assert_eq!(booking.day, NaiveDate("2026-08-06".to_owned()));

// The crate makes every check the spec could make first. Those checks say more
// than a conversion could: this is the date control's own format check.
let errors = Booking::from_urlencoded("day=whenever").unwrap_err();
assert_eq!(
    errors.field("day").next().unwrap().message.as_str(),
    "Enter a date as YYYY-MM-DD."
);
```

Everything else about the field stays the same. A `validate` function gets the
type the field was written as, and `Display` writes the value out again for an
edit form.

There are two things it cannot do, and one answer to both. It implies no
control, because a foreign type has no `CONTROL` to give one. The field is
therefore text until `type = "…"` says otherwise. And a value that will not
parse gets the message "Enter a valid value." A `FromStr` error speaks to
whoever wrote the call, not to whoever filled in the form. Give the field the
`type` or the `pattern` it really has. That check runs first, and it says what
it wanted.

For a type you own, say this once on the type in place of at every field that
names it. Self-conversion asks nothing of a type's shape, so this is also how a
several-field struct or an enum becomes one value:

```rust
use std::fmt;
use std::str::FromStr;

#[derive(html_form::FormValue)]
#[value(from_str, pattern = r"\d+\.\d+\.\d+", default = "1.0.0")]
struct Version { major: u32, minor: u32, patch: u32 }

// `from_str` asks for these two, and for nothing else.
impl FromStr for Version {
    type Err = &'static str;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let mut parts = raw.split('.').map(str::parse);
        let mut next = || parts.next().transpose().ok().flatten().ok_or("not a version");
        Ok(Version { major: next()?, minor: next()?, patch: next()? })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}
```

### By hand

The trait is small enough to write out. It requires only the two conversions:

```rust
use std::borrow::Cow;
use html_form::{Control, FormValue, ValueError};

struct Slug(String);

impl FormValue for Slug {
    const CONTROL: Control = Control::TEXT;

    fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
        match raw.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
            true => Ok(Slug(raw.to_owned())),
            false => Err(ValueError::new("lowercase letters and dashes")),
        }
    }

    fn to_form_value(&self) -> Cow<'static, str> {
        Cow::Owned(self.0.clone())
    }
}
```

The derive fills in `DEFAULT`, `into_form_value` and `validate_form_value`. They
default to "none", "borrow and copy" and "nothing to add". Write
`into_form_value(self)` where the type has a `String` to move: the crate calls
it wherever it owns the value, such as a default a form produced.

`CONTROL` is one constant, not five. The control a type renders as *and* the
constraints it implies travel together, so `u32` says

```rust
# use html_form::{Bounds, Control, NumberControl};
const CONTROL: Control = Control::Number(NumberControl {
    bounds: Bounds { min: Some("0"), step: Some("1"), max: None },
    ..NumberControl::DEFAULT
});
```

A type therefore cannot promise a `step` and a text format at the same time.

## Controls: attributes that cannot be misplaced

A field's `Control` names the control *and* carries the attributes that the
control accepts. Nothing else can name them.

| Variant | What it carries |
|---|---|
| `Text` | `format` (`text`, `password`, `tel`, `search`, `url`, `email { multiple }`), `pattern`, `minlength`, `maxlength` |
| `Textarea` | `minlength`, `maxlength`, `rows`, `cols`. No `pattern`, which `<textarea>` really lacks |
| `Number` | `format` (`number`/`range`) and `Bounds` (`min`/`max`/`step`), compared as numbers |
| `Temporal` | `format` (`date`, `time`, `datetime-local`, `month`, `week`) and `Bounds`, compared as dates and times |
| `Choose` | `style` (`select`/`radio`/`checkbox`), `multiple`, the choice list |
| `File` | `accept`, `multiple` |
| `Checkbox`, `Color`, `Hidden` | nothing |

So `#[field(type = "date", minlength = 3)]` is a compile error, and so is
`#[field(pattern = "…")]` on a `u32`. Neither becomes an attribute that quietly
does nothing. And `validate.rs` is one exhaustive `match` with no arm in which a
`<select>` has a `pattern`.

Where the Rust type and the attribute both have an opinion, the attribute wins,
but the crate does not discard the type. `#[field(type = "range", max = 10)]` on
a `u32` keeps the `min = "0"` and `step = "1"` that the integer implies.
`#[field(type = "radio")]` on a `FormChoice` enum changes the style of the
control and keeps its variants.

## Choosing between options

A set of options renders three ways, and the style decides how many values the
field can carry:

| `type` | Markup | Values |
|---|---|---|
| `select` (default) | `<select>`, with `multiple` when the field is a `Vec` | one, or many |
| `radio` | one `<input type="radio">` per option | always one |
| `checkbox` | one `<input type="checkbox">` per option | always many |

On a `bool`, `type = "checkbox"` means the single boolean box. On anything that
already chooses between options, it means a checkbox *group*. That is a
`FormChoice` enum, or a field with `choices` or `#[option(...)]`:

```rust
use html_form::{Form, FormChoice};

#[derive(FormChoice, Debug, PartialEq)]
enum Topic {
    Releases,
    Security,
}

#[derive(Form, Debug)]
struct Preferences {
    #[field(type = "checkbox", label = "Notify me about")]
    notify: Vec<Topic>,
}

let prefs = Preferences::from_urlencoded("notify=releases&notify=security").unwrap();
assert_eq!(prefs.notify, [Topic::Releases, Topic::Security]);
```

A checkbox group is the usable alternative to `<select multiple>`. Two things
follow from HTML rather than from this crate:

- A group carries many values whatever the field type is, so use a `Vec`.
- `required` on a checkbox means "tick *this* box", not "tick one of them". The
  group therefore renders `aria-required`, and the server enforces the
  requirement.

## Server-side validation

The server checks again everything the markup asks the browser to enforce,
because a submission can come from anywhere:

- `required` — a missing, an empty *and* a whitespace-only value all fail
- `pattern` — anchored at both ends, as the browser anchors it (needs the
  `pattern` feature, on by default)
- `minlength` / `maxlength` — counted in characters
- `min` / `max` — as numbers for `number` and `range`, as dates and times for
  `date`, `time`, `month` and `week`
- `step` — including a step base relative to `min`
- the control's own type — the crate checks the format of `email`, `url`,
  `color` and the date and time controls. It checks a date against the calendar,
  so it rejects `2026-02-31`
- `<select>`, radio and checkbox-group values must be one of the declared
  choices. For the multi-valued ones, every submitted value must be

## Defaults, blank values and checkboxes

- `default` fills a blank form, and that is all it does. **Parsing never reads
  it.** A field a submission left out is a field with no value: an optional one
  parses as empty, and a required one is reported as missing. If a default stood
  in, a request carrying no CSRF token at all would arrive holding a valid one.
- An unchecked checkbox submits nothing at all, so absence means `false`. It
  never means "use the default". A user must check a `required` checkbox.
- A literal (`default = "web"`), the field type's own `Default` (`default` with
  nothing after it) and a **generated** one (`default = some_fn`) share one slot
  in the spec, and the derive writes the glue for all three. The last two hand
  back the field's own type, or anything that converts into it, and the crate
  writes it out as it writes out every other value.
- Once there are values to show, such as a submission to re-render or a record
  to edit, the crate mints only a hidden field whose default it generates. A
  visible field shows what it received, an empty value included — a form that
  filled one in would put a value the caller never had in front of the user, to
  save without noticing. Where a render shows what it received, the generator is
  not called at all.
- `#[field(reset)]` says the same about any other field. It shows its default on
  every render, and never what was submitted or stored. The derive settles this
  once, in the spec, so `reset = false` also turns off the rule above.

```rust
use html_form::Form;

# struct Session { csrf: String }
# fn issued_token(session: &Session) -> String { session.csrf.clone() }
#[derive(Form)]
#[form(context = Session)]
struct ChangePassword {
    // A hidden field the form generates resets already. `reset` says it where
    // the crate cannot tell on its own: a literal default, or a visible control.
    #[field(type = "hidden", default = issued_token)]
    csrf_token: String,

    // No default, so a rejected submission comes back with an empty box.
    #[field(type = "password", reset, minlength = 12)]
    new_password: String,
}

let session = Session { csrf: "3f9c…".to_owned() };
let outcome = ChangePassword::submit_urlencoded_with_context(
    "csrf_token=3f9c…&new_password=short",
    &session,
);
let view = outcome.view().unwrap();

// The box comes back empty, and the token comes back freshly minted.
assert_eq!(view.field("new_password").unwrap().value, None);
assert!(view.field("new_password").unwrap().has_errors);
assert_eq!(view.field("csrf_token").unwrap().value.as_deref(), Some("3f9c…"));
```

The field still *parses* as any other does. Resetting is about what the next
render shows, so a validator sees the value the user sent, and the error it
reports stays on the field.

## What the form's own functions receive

A `const` cannot hold a token that has to match the session. Nor a list of
options only the database knows, nor a check that depends on who is logged in.
`#[form(context = …)]` declares a type. The caller passes it in at the moment
the form renders or parses, and every function the form names receives it.

```rust
use html_form::{Form, Text};

/// Whatever a handler already has: the session, a connection, the clock.
struct Session { csrf: String }

#[derive(Form, Debug)]
#[form(method = "post", context = Session)]
struct Comment {
    #[field(type = "hidden", default = issued_token, validate = is_our_token)]
    csrf_token: String,

    #[field(type = "textarea", label = "Comment", maxlength = 2000)]
    body: String,
}

/// A default may take the context. The crate calls it once per render.
fn issued_token(session: &Session) -> String {
    session.csrf.clone()
}

/// So may a validator, after the value it checks.
fn is_our_token(submitted: &String, session: &Session) -> Result<(), Text> {
    match *submitted == session.csrf {
        true => Ok(()),
        false => Err(Text::key("form.csrf.rejected")),
    }
}

let session = Session { csrf: "3f9c…".to_owned() };

// The session that will check the hidden field also fills it in.
let view = Comment::render_with_context(&session);
assert_eq!(view.field("csrf_token").unwrap().value.as_deref(), Some("3f9c…"));

let comment = Comment::from_urlencoded_with_context(
    "csrf_token=3f9c…&body=Nice+post",
    &session,
)
.unwrap();
assert_eq!(comment.body, "Nice post");

// The check the markup could not make rejects somebody else's token.
let errors =
    Comment::from_urlencoded_with_context("csrf_token=forged&body=x", &session).unwrap_err();
assert_eq!(
    errors.field("csrf_token").next().unwrap().code(),
    Some("form.csrf.rejected")
);
```

A context is a type that owns what it holds — `String` above rather than
`&'a str`. One `const` spec describes every render, so it cannot name a context
type: the render hands the context to the spec as a `&dyn Any` and the glue the
derive wrote beside it names the type back. That is what makes the context
`'static`, and it is also why pairing a spec with the wrong context is a panic
that says so rather than anything worse. A context that has to borrow reaches
the form behind an `Arc` or a handle.

A context changes the **names** of the calls, not their meaning. Every method
that renders or parses has a `…_with_context` twin that takes `&Context`. A form
that declares no context keeps the short name: `render()`, `from_values()`,
`submit()`. `Form` itself holds both halves. The short one asks for
`Context: EmptyContext`, which `()` satisfies. A form that needs nothing
therefore needs no extra import, and reads exactly as it always did.

| Without a context | With one |
|---|---|
| `Signup::render()` | `Signup::render_with_context(&ctx)` |
| `Signup::render_localized(t)` | `Signup::render_localized_with_context(t, &ctx)` |
| `Signup::render_submitted(&values, &errors)` | `Signup::render_submitted_with_context(&values, &errors, &ctx)` |
| `signup.render_filled()` | `signup.render_filled_with_context(&ctx)` |
| `Signup::from_values(&values)` | `Signup::from_values_with_context(&values, &ctx)` |
| `Signup::from_urlencoded(body)` | `Signup::from_urlencoded_with_context(body, &ctx)` |
| `Signup::submit(&values)` | `Signup::submit_with_context(&values, &ctx)` |
| `Signup::submit_urlencoded(body)` | `Signup::submit_urlencoded_with_context(body, &ctx)` |

Either arity works wherever you name a function: `fn() -> String` and
`fn(&Session) -> String`, `fn(&T) -> bool` and `fn(&T, &Session) -> bool`. The
crate reads which one you wrote off the function itself. A form that gains a
context therefore need not rewrite the checks that never took one.

The crate parses and renders a flattened sub-form with the enclosing form's
context. `Provides` lets the two differ. Most usefully, it lets you reuse a form
written without a context inside one that has a context:

```rust
# struct Session { csrf: String }
impl html_form::Provides<()> for Session {
    fn provide(&self) -> &() { &() }
}
```

`examples/csrf.rs` is a generic `WithCsrf<T>` wrapper that puts all of this
together. It has one hidden field, a token from the session, and a check the
markup could not make.

## Editing an existing record

`fill_in` runs the conversion the other way, so you can show a filled-in form:

```rust
use html_form::Form;

#[derive(Form)]
struct Article {
    #[field(label = "Title")]
    title: String,
    #[field(type = "textarea", label = "Body")]
    body: String,
}

let article = Article { title: "Hello".into(), body: "World".into() };
let view = article.render_filled();

assert_eq!(view.field("title").unwrap().value.as_deref(), Some("Hello"));
```

## Feature flags

| Feature | Default | What it does |
|---|---|---|
| `derive` | on | `#[derive(Form)]`, `#[derive(FormValue)]`, `#[derive(FormChoice)]` |
| `html` | on | The built-in renderer: `to_html`, the `Display` impls and `escape` |
| `pattern` | on | Server-side `pattern` checking with `regex-lite`. Without it, the crate still renders `pattern`, and the browser enforces it |
| `axum` | off | `Outcome<T>` and `axum::Form<T, R>` are axum 0.8 extractors |

## Framework integration

The crate has no opinion about your HTTP stack. `Values::from_pairs` takes
anything a framework's body parser produces:

```rust
use html_form::{Form, Outcome, Values};

# #[derive(Form)]
# struct Signup { #[field(type = "email")] email: String }
# let parsed_body = vec![("email", "ada@example.com")];
let values = Values::from_pairs(parsed_body);
assert!(matches!(Signup::submit(&values), Outcome::Valid(_)));
```

### JSON

A submission also does not have to be a form. `Values` is `Serialize` and
`Deserialize`, so a JSON body is a submission too. You get the same struct and
the same checks, and the errors already serialize for the client that sent JSON
in the first place:

```rust
use html_form::{Form, Values};

# #[derive(Form)]
# struct Signup {
#     #[field(type = "email")]
#     email: String,
#     age: Option<u32>,
#     tags: Vec<String>,
# }
let values: Values = serde_json::from_str(
    r#"{"email": "ada@example.com", "age": 36, "tags": ["rust", "forms"]}"#,
)?;
let signup = Signup::from_values(&values)?;
assert_eq!(signup.age, Some(36));
assert_eq!(signup.tags, ["rust", "forms"]);

// And back out, for a client that wants JSON in place of markup.
let json = serde_json::to_string(&signup.to_values())?;
assert_eq!(json, r#"{"email":"ada@example.com","age":"36","tags":["rust","forms"]}"#);
# Ok::<(), Box<dyn std::error::Error>>(())
```

An object is a submission, and so is a list of `[name, value]` pairs:

| JSON | Read as |
|---|---|
| `{"email": "a@b.com"}` | one field |
| `{"age": 36}`, `{"newsletter": true}` | the string a form would have submitted. A field is a string whatever type a client sent |
| `{"tag": ["x", "y"]}` | a name submitted more than once, as a checkbox group submits it |
| `{"age": null}` | a name the client did *not* submit, so `Option` sees nothing. To clear a field, a client sends `""`, as the browser does |
| `[["tag", "x"], ["tag", "y"]]` | the same, and every repeat keeps its place |
| `{"billing": {"street": "…"}}` | rejected. A form submits a flat list of names, so flatten the struct and send `billing_street` |

Serializing goes the other way. A name with one value becomes a string, and a
name with several becomes a list. `signup.to_values()` is therefore a JSON body
a client can send straight back.

### axum

With the `axum` feature, `Outcome<T>` is an extractor, so a handler receives the
submission already parsed and validated. It goes last, because it consumes the
body.

```rust,no_run
use axum::response::{Html, IntoResponse, Response};
use axum::http::StatusCode;
use html_form::{Form, Outcome};

# #[derive(Form)]
# struct Signup { #[field(type = "email")] email: String }
async fn signup(form: Outcome<Signup>) -> Response {
    match form {
        Outcome::Valid(signup) => Html(format!("Welcome, {}!", signup.email)).into_response(),
        // The re-render carries what the user typed and each error.
        Outcome::Invalid { view, .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, Html(view.to_html())).into_response()
        }
    }
}
```

A failed validation is **not** an extractor rejection. The handler still runs
and decides what to do with the re-render. `FormRejection` covers only the cases
with no submission to validate. A body that is not
`application/x-www-form-urlencoded` gives 415. A body the server could not read,
or one that is not UTF-8, gives 400. As with `axum::Form`, the crate reads `GET`
and `HEAD` from the query string.

An extractor has nothing but the request, so `Outcome<T>` extracts only a form
whose `Context` is `()`. Submit a form that asks for a context in the handler,
where the context is: take the body, then call `T::submit_with_context`.

#### Rejecting with the form

`html_form::axum::Form<T, R>` is the other extractor. It rejects a failed
validation with the form itself, so the handler runs only on a valid submission
and holds nothing else. `R` is a `Renderer`: one type, written once, that turns
a failed submission into the response for every form that shares a layout.

```rust,no_run
use axum::response::{Html, IntoResponse};
use html_form::FormView;
use html_form::axum::{Form, Renderer};

# #[derive(html_form::Form)]
# struct Signup { #[field(type = "email")] email: String }
struct Page;

impl<T: html_form::Form<Context = ()>> Renderer<T> for Page {
    fn render(view: FormView, _context: &()) -> impl IntoResponse {
        Html(format!("<h1>Check the form</h1>{}", view.to_html()))
    }
}

// No case left to match on: the submission is valid, or the handler never ran.
async fn signup(form: Form<Signup, Page>) -> Html<String> {
    Html(format!("Welcome, {}!", form.data.email))
}

// `Form<T, R>` is also a response. A handler that returns one shows the form
// filled in from the value, through the same renderer, so the page a `GET` puts
// up and the page a failed `POST` puts up are the same page.
async fn edit() -> Form<Signup, Page> {
    Form::new(Signup { email: "ada@example.com".to_owned() })
}
```

A renderer builds a page, not a status. `html_form::axum::Builtin` is the one
around `view.to_html()`, for a form that is the whole answer. A renderer may
also be a plain function, or a closure written in the attribute.

`#[form(renderer = ...)]` moves the renderer onto the declaration, and the struct
itself becomes the extractor and the response — so a `GET` handler returns
`Signup` and a bad `POST` never arrives. `#[form(status = 201)]` moves the status
of the page a handler returns, and `from_request = false` /
`into_response = false` leave either impl out.

The rejection is not a response yet. `Rejection<T, R, C>` keeps each refusal in
the type it refused as: `C` is whatever the context's own extractor rejects
with, and the invalid case is an `Invalid<T, R>` holding the `FormView`, the
`FormErrors` and the context. The renderer runs when a response is asked for, so
a layer can log what failed, and a handler can map the rejection to something
else without ever rendering the page:

```rust,no_run
use axum::response::{IntoResponse, Response};
use html_form::axum::{Builtin, Form, Rejection};
# #[derive(html_form::Form, Debug)]
# struct Signup { #[field(type = "email")] email: String }

/// Whatever `Form<Signup, Builtin>` turned the request down with.
fn handle(rejection: Rejection<Signup, Builtin, std::convert::Infallible>) -> Response {
    let Rejection::Invalid(invalid) = rejection else {
        return rejection_is_not_ours();
    };
    // Still a form and a set of errors, because nothing has rendered yet.
    eprintln!("{} field(s) rejected", invalid.errors.len());

    invalid.into_response() // `R` runs here, and the `400` goes on here.
}
# fn rejection_is_not_ours() -> Response { unimplemented!() }
```

The status is decided there too, because what went wrong is what a status is
about:

| Rejection | Status |
|---|---|
| A body that is not `application/x-www-form-urlencoded` | 415 Unsupported Media Type |
| A body the server could not read | the one that rejection came with, 400 among them |
| A context that turned the request down | whatever that extractor answers with |
| A submission that failed validation | 400 Bad Request |

This extractor also reads the form's `Context` out of the request, which is what
`Outcome<T>` cannot do. The context has to be an axum extractor of its own
(`T::Context: FromRequestParts<S>`), which a session read from a cookie already
is. A form that declares no context asks for `()`, and axum extracts that from
any request, so a form that needs nothing needs nothing done to it.

The feature depends on `axum-core`, not `axum`. `examples/axum_signup.rs` is a
whole application on `Form<T, R>` — one `Renderer` for the layout, a blank form,
a submission and an edit page — run with
`cargo run --example axum_signup --features axum`.

`multipart/form-data` bodies are out of scope. Parse them with a multipart crate
and pass the text fields to `Values::from_pairs`.

## License

Licensed under either of
[Apache License, Version 2.0](https://github.com/lperlaki/html-form-rs/blob/main/LICENSE-APACHE)
or [MIT license](https://github.com/lperlaki/html-form-rs/blob/main/LICENSE-MIT)
at your option. Unless you explicitly state otherwise,
any contribution intentionally submitted for inclusion in this crate by you, as
defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
