# web-form

Declarative HTML forms for Rust. One struct describes a form; the crate renders
it, parses the submission back into it, and — when validation fails — hands you
the same form again with the user's values and the error messages already
attached.

```rust
use web_form::{Outcome, WebForm};

#[derive(WebForm)]
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
```

```rust
// GET  /signup
let html = Signup::render().to_html();

// POST /signup
match Signup::submit_urlencoded(body) {
    Outcome::Valid(signup) => create_account(signup),
    // `view` is the same render format, now carrying what the user typed
    // and what was wrong with it.
    Outcome::Invalid { errors, view } => render_page(&view),
}
```

## What it does

| | |
|---|---|
| **Render** | `Signup::render()` → a `FormView`: flat, `serde::Serialize`, ready for MiniJinja/Askama, or `.to_html()` for markup out of the box |
| **Parse** | `Signup::from_urlencoded(body)` → `Result<Signup, FormErrors>` |
| **Validate** | Every HTML attribute — `required`, `pattern`, `minlength`, `min`, `step`, the control's own type — is re-checked server-side |
| **Collect** | Validation never stops at the first failure; one pass reports every problem on every field |
| **Re-render** | `Outcome::Invalid` gives back a `FormView` with the submitted values and per-field messages |
| **Reuse** | `#[field(flatten)]` splices one form into another, optionally under a name prefix |
| **Localise** | Any label, help text, placeholder, legend or option label can be an i18n key instead of text |
| **Context** | `#[form(context = Session)]` hands the session — or a connection, or the clock — to the form's own `default` and `validate` functions |

## Collecting every error

Each field is attempted independently, so one round trip tells the user
everything that is wrong:

```rust
let body = "email=nope&password=short&age=7";
let errors = Signup::from_urlencoded(body).unwrap_err();

assert_eq!(errors.len(), 3);
for (field, error) in errors.iter() {
    println!("{field}: {}", error.message);
}
// email:    Enter a valid email address.
// password: Enter at least 12 characters (currently 5).
// age:      Must be 18 or more.
```

Errors carry a typed `ErrorKind` alongside the built-in English message, so you
can match on `ErrorKind::TooShort { minlength, length }` and write your own (or
localised) text. `FormErrors` also serialises to
`{"form": [...], "fields": {"email": [...]}}` for JSON APIs.

## Localisation

Anywhere a person-facing string is accepted, `t("…")` names an i18n key instead
of giving the text:

```rust
#[derive(WebForm)]
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

`label`, `help`, `placeholder`, `legend`, `submit`, and the labels and
`group`s of `#[option(...)]` and `#[choice(...)]` all take either form. The two
are told apart in the spec by `Text`, and the difference survives into the
render format.

The crate resolves nothing itself — it has no more opinion about your i18n stack
than about your HTTP stack. Hand it a lookup and it walks the view:

```rust
let view = Signup::render_localized(|key| bundle.get(key));

// …or on a view from anywhere else
let view = article.render_filled().localized(|key| bundle.get(key));
let view = outcome.view().unwrap().localized(&translate);
```

`translate` is any `Fn(&str) -> Option<impl Into<Cow<'static, str>>>`, so a
lookup that hands back `&'static str` costs nothing to apply.

Or leave it to the template. Every translatable string comes with a companion
`…_key`, set only while the key is still unresolved:

```jinja
{{ field.label_key and t(field.label_key) or field.label }}
```

Until a key is resolved the string *is* the key, so `{{ field.label }}` renders
something either way, and a key no backend recognises stays visible rather than
turning into a blank label — a bug a reader can report. Once resolved, the
`…_key` is cleared, so nothing translates twice.

`Choice::keyed("de", "country.de")` and `Choice::owned_keyed(id, key)` build
keyed options at render time, next to `Choice::new` and `Choice::owned`.

## Reuse: flattening one form into another

Any `WebForm` can be embedded in another. Without a prefix it shares the
parent's namespace; with one, the same sub-form can be embedded more than once.

```rust
#[derive(WebForm)]
struct Address {
    #[field(label = "Street")]
    street: String,
    #[field(label = "Postcode", pattern = r"\d{5}")]
    postcode: String,
}

#[derive(WebForm)]
struct Order {
    customer: String,

    #[field(flatten, prefix = "billing_", legend = "Billing address")]
    billing: Address,

    #[field(flatten, prefix = "shipping_", legend = "Shipping address")]
    shipping: Address,
}
```

The fields are submitted as `billing_street`, `shipping_postcode` and so on;
errors are keyed by the same prefixed names, and each group renders inside its
own `<fieldset>` with the given legend. `Address` remains a perfectly good form
on its own — nothing about it had to change to become embeddable.

## Rendering

`FormView` is the render format: one flat, fully-resolved struct per form, with
one `FieldView` per control. Nothing about it is Rust-specific, so a template
author never has to know what an enum is.

### Built in

```rust
println!("{}", Signup::render());
```

Plain, unstyled markup with `web-form__*` class hooks, correct `for`/`id`
pairing, `aria-invalid` and `aria-describedby` wired to the help text and error
list, `<fieldset>`s for flattened groups, and `<optgroup>`s for grouped choices.

This is the `html` feature, on by default. Rendering through a template engine
instead? Turn it off — `to_html`, the `Display` impls and `escape` go with it,
and nothing else changes.

### MiniJinja

`FormView` is `Serialize`, so it drops straight into a context:

```rust
let html = template.render(context! { form => Signup::render() })?;
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

See `examples/minijinja_render.rs` for the whole request cycle:
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

`FieldView::control_html()` renders one control on its own and
`FieldView::to_html()` renders it with its label, help text and errors, so you
can mix your own markup with generated controls at whatever granularity suits.

### Options that only exist at runtime

The view is owned and mutable, which is where a list from the database goes:

```rust
let mut view = Booking::render();
view.field_mut("room")
    .unwrap()
    .set_choices(rooms.iter().map(|r| Choice::owned(&r.id, &r.name)));
```

`view.add_field_error("email", "That address is already registered.")` covers the
errors only your database knows about.

### What rendering costs

A spec is `const`: `WebForm::SPEC` is one const-evaluated value, flattened
sub-forms and all, so there is nothing to build the first time a form renders,
and no call to make to reach a sub-form's description.

Every string in the render format is a `Cow<'static, str>`, and rendering borrows
from the spec wherever it can. What a blank form still allocates is what the spec
does not contain:

| Borrowed from the spec | Built per render |
|---|---|
| names, labels, help text, placeholders | the three ids derived from each field's name |
| options, constraints, custom attributes | the `Vec`s of fields and of options |
| defaults, and the built-in error messages | the values a user submitted, which do not outlive the request |

The owned half of each `Cow` is what keeps the view an ordinary owned value a
handler can return, and what `set_value`, `set_choices` and `localize` put
runtime strings into. Serialisation is unaffected — a `Cow` serialises as the
string it holds.

The same holds while parsing: a field's name reaches `FormErrors` borrowed from
the spec, and only `#[field(flatten, prefix = "…")]` makes a name that has to be
built.

## Attributes

### `#[form(...)]` — on the struct

| | |
|---|---|
| `id`, `name`, `action`, `class` | Rendered onto the `<form>` |
| `method = "post"` | `get`, `post` or `dialog` |
| `enctype = "multipart/form-data"` | For file uploads |
| `novalidate` | Turn off the browser's own validation (the server still validates) |
| `submit = "Create account"` | Caption of the built-in submit button; `submit = t("key")` for an i18n key |
| `validate = path::to::fn` | Cross-field check: `fn(&Self) -> Result<(), E>`, or `fn(&Self, &Context) -> Result<(), E>` |
| `context = Session` | What the form's own functions are handed, and what every `…_with_context` call takes. Defaults to `()` |
| `attr("hx-post" = "/signup")` | Anything the crate has no opinion about, rendered verbatim |

### `#[field(...)]` — on a field

| | |
|---|---|
| `type = "email"` | The control; inferred from the Rust type otherwise |
| `label`, `placeholder`, `help`, `autocomplete`, `id`, `class`, `rows`, `cols` | Presentation. `label` defaults to the humanised field name; `label = ""` renders none |
| `label = t("key")` | `label`, `help`, `placeholder` and `legend` also take an i18n key |
| `name = "e-mail"` | Submitted name; defaults to the field name |
| `required` / `optional` | Overrides the inferred default |
| `default = "…"` | Value shown on a blank form |
| `default = path::to::fn` | A value produced afresh per render — a token, a nonce, today's date: `fn() -> impl Into<Cow<'static, str>>`, or `fn(&Context) -> …`. Never a fallback while parsing |
| `pattern`, `minlength`, `maxlength`, `min`, `max`, `step`, `accept` | Validation |
| `disabled`, `readonly`, `autofocus`, `multiple` | Flags |
| `choices = SOME_CONST` | A `&'static [Choice]` |
| `validate = path::to::fn` | Per-field check: `fn(&FieldType) -> Result<(), E>`, or `fn(&FieldType, &Context) -> Result<(), E>` |
| `attr("data-role" = "input")` | Anything the crate has no opinion about, rendered verbatim |
| `flatten` (+ `prefix`, `legend`) | Splice another form in |
| `skip` | Not part of the form; filled with `Default::default()` |

Options can also be listed inline:

```rust
#[field(label = "Country")]
#[option("de", "Germany")]
#[option("ch", "Switzerland", group = "Non-EU")]
country: String,
```

### Custom attributes

`attr(...)` is the escape hatch for markup this crate knows nothing about —
`data-*`, `hx-*`, `aria-*`, whatever your front end reads:

```rust
#[derive(WebForm)]
#[form(attr("hx-post" = "/search", "hx-target" = "#results"))]
struct Search {
    #[field(attr("hx-trigger" = "keyup changed delay:300ms", autocorrect = "off"))]
    query: String,

    #[field(attr(inert))]                     // a bare word is a boolean attribute
    sort: Option<String>,
}
```

A dashed name has to be written as a string; a bare word is taken as written, so
`attr(data_role = "x")` renders `data_role`, not `data-role`. Values may be any
literal (`"3"` and `3` are the same thing), and each entry is escaped like every
other value.

The list is stored in the spec (`FieldSpec::attrs`), reaches the render format
as `field.attrs` — a `[{name, value}]`, where a `null` value means a boolean
attribute — and is rendered after every attribute the crate generates itself.
Naming one of *those* is a compile error rather than a duplicate attribute the
browser would ignore: `attr("class" = "x")` tells you to write
`#[field(class = "x")]`. `FormView::set_attr` and `FieldView::set_attr` set them
at render time, alongside `set_choices`.

## Types

| Rust type | Control | Required by default |
|---|---|---|
| `String` | `text` | yes |
| `u8` … `i128`, `usize` | `number` (with `min`/`step` implied by the type) | yes |
| `f32`, `f64` | `number`, `step="any"` | yes |
| `bool` | `checkbox` — absent means `false` | no |
| `Option<T>` | as `T` | no |
| `Vec<T>` | as `T`; every value submitted under the name is collected, and `multiple` is rendered on the controls that accept one (`select`, `email`, `file`) | no |
| `Vec<T>` + `type = "checkbox"` | one checkbox per option, sharing the name | no |
| `#[derive(FormChoice)] enum` | `select` with the variants as options | yes |
| your own type | whatever its `FormValue` impl says | yes |

A fieldless enum becomes a `<select>`:

```rust
#[derive(FormChoice)]
enum Plan {
    Free,                                       // submits "free", labelled "Free"
    #[choice(value = "pro", label = "Professional")]
    Pro,
    #[choice(group = "Contact sales", disabled)]
    SelfHosted,                                 // submits "self-hosted"
}
```

A value outside the declared options is rejected with `ErrorKind::NotAChoice` —
`<select>` is a constraint, not a suggestion.

Anything else implements `FormValue`:

```rust
impl FormValue for Slug {
    const CONTROL: Control = Control::TEXT;

    fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
        // …
    }

    fn to_form_value(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.0)
    }
}
```

`CONTROL` is one constant, not five: the control a type renders as *and* the
constraints it implies travel together, so `u32` says

```rust
const CONTROL: Control = Control::Number(NumberControl {
    bounds: Bounds { min: Some("0"), step: Some("1"), max: None },
    ..NumberControl::DEFAULT
});
```

and there is no way for it to promise a `step` and a text format at the same
time.

## Controls: attributes that cannot be misplaced

A field's `Control` names the control *and* carries the attributes that control
accepts — nothing else can name them.

| Variant | What it carries |
|---|---|
| `Text` | `format` (`text`, `password`, `tel`, `search`, `url`, `email { multiple }`), `pattern`, `minlength`, `maxlength` |
| `Textarea` | `minlength`, `maxlength`, `rows`, `cols` — no `pattern`, which `<textarea>` genuinely lacks |
| `Number` | `format` (`number`/`range`) and `Bounds` (`min`/`max`/`step`, compared numerically) |
| `Temporal` | `format` (`date`, `time`, `datetime-local`, `month`, `week`) and `Bounds`, compared chronologically |
| `Choose` | `style` (`select`/`radio`/`checkbox`), `multiple`, the choice list |
| `File` | `accept`, `multiple` |
| `Checkbox`, `Color`, `Hidden` | nothing |

So `#[field(type = "date", minlength = 3)]` and `#[field(pattern = "…")]` on a
`u32` are compile errors rather than attributes that quietly do nothing, and
`validate.rs` is one exhaustive `match` with no arm in which a `<select>` has a
`pattern`.

Where the Rust type and the attribute both have an opinion, the attribute wins
but the type is not discarded: `#[field(type = "range", max = 10)]` on a `u32`
keeps the `min = "0"` and `step = "1"` the integer implies, and
`#[field(type = "radio")]` on a `FormChoice` enum restyles the control without
losing its variants.

## Choosing between options

A set of options renders three ways, and the style decides how many values the
field can carry:

| `type` | Markup | Values |
|---|---|---|
| `select` (default) | `<select>`, with `multiple` when the field is a `Vec` | one, or many |
| `radio` | one `<input type="radio">` per option | always one |
| `checkbox` | one `<input type="checkbox">` per option | always many |

`type = "checkbox"` means the lone boolean box on a `bool`, and a checkbox
*group* on anything that is already choosing between options — a `FormChoice`
enum, or a field with `choices` / `#[option(...)]`:

```rust
#[field(type = "checkbox", label = "Notify me about")]
notify: Vec<Topic>,
```

A checkbox group is the usable alternative to `<select multiple>`. Two things
follow from HTML rather than from this crate:

- It is multi-valued whatever the field is typed as, so pair it with a `Vec`.
- `required` on a checkbox means "tick *this* box", not "tick one of them", so
  the group renders `aria-required` instead and the requirement is enforced on
  the server.

## Server-side validation

Everything the markup asks the browser to enforce is enforced again on the
server, because a submission can come from anywhere:

- `required` — missing, empty *and* whitespace-only all fail
- `pattern` — anchored at both ends, like the browser does (needs the `pattern`
  feature, on by default)
- `minlength` / `maxlength` — counted in characters
- `min` / `max` — numerically for `number`/`range`, chronologically for
  `date`/`time`/`month`/`week`
- `step` — including a `min`-relative step base
- the control's own type — `email`, `url`, `color` and the date/time formats are
  format-checked, and dates are checked against the calendar, so `2026-02-31` is
  rejected
- `<select>`, radio and checkbox-group values must be one of the declared
  choices — every submitted value, for the multi-valued ones

## What the form's own functions are handed

A token that has to match the session, a check that depends on who is logged in,
a default only the request knows: none of that fits in a `const`.
`#[form(context = …)]` declares a type the caller passes in at the moment it
renders or parses, and every function the form names is handed it.

```rust
use web_form::{Text, WebForm};

struct Session { csrf: String }

#[derive(WebForm)]
#[form(method = "post", context = Session)]
struct Comment {
    #[field(type = "hidden", default = issued_token, validate = is_our_token)]
    csrf_token: String,

    #[field(type = "textarea", label = "Comment", maxlength = 2000)]
    body: String,
}

fn issued_token(session: &Session) -> String {
    session.csrf.clone()
}

fn is_our_token(submitted: &String, session: &Session) -> Result<(), Text> {
    match *submitted == session.csrf {
        true => Ok(()),
        false => Err(Text::key("form.csrf.rejected")),
    }
}

let view = Comment::render_with_context(&session);
let outcome = Comment::submit_urlencoded_with_context(body, &session);
```

Declaring a context changes the **names** of the calls, not their meaning: every
method that renders or parses has a `…_with_context` form taking `&Context`, and
a form that declares none keeps the short one — `render()`, `from_values()`,
`submit()`. Both halves are on `WebForm` itself; the short one is bounded by
`Context: EmptyContext`, which `()` is, so a form that asks for nothing needs no
extra import and reads exactly as it always did.

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

Either arity will do wherever a function is named — `fn() -> String` and
`fn(&Session) -> String`, `fn(&T) -> bool` and `fn(&T, &Session) -> bool`. Which
one was written is read off the function itself, so a form that gains a context
does not have to rewrite the checks that never needed one.

A flattened sub-form is parsed and rendered with the enclosing form's context.
Say what a context hands down to one that asks for something else — most often a
sub-form written without a context at all:

```rust
impl web_form::Provides<()> for Session {
    fn provide(&self) -> &() { &() }
}
```

`examples/csrf.rs` is a generic `WithCsrf<T>` wrapper putting the lot together:
one hidden field, a token from the session, and a check the markup could not
have made.

## Editing an existing record

`fill_in` runs the conversion the other way, so a form can be shown filled in:

```rust
let view = article.render_filled();
```

## Defaults, blank values and checkboxes

- `default` fills a blank form, and applies when a field is **absent** from a
  submission.
- A field that is **present but empty** is never replaced by its default — that
  is how a user clears a field.
- An unchecked checkbox submits nothing at all, so absence means `false`, never
  "fall back to the default". A `required` checkbox must be checked.

## Feature flags

| Feature | Default | What it does |
|---|---|---|
| `derive` | on | `#[derive(WebForm)]`, `#[derive(FormChoice)]` |
| `pattern` | on | Server-side `pattern` checking via `regex-lite`. Without it, `pattern` is still rendered and enforced by the browser |
| `axum` | off | `Outcome<T>` is an axum 0.8 extractor |

## Framework integration

The crate has no opinion about your HTTP stack. `Values::from_pairs` takes
anything a framework's body parser produces:

```rust
let values = Values::from_pairs(parsed_body);
match Signup::submit(&values) { /* … */ }
```

### axum

With the `axum` feature, `Outcome<T>` is an extractor, so a handler receives the
submission already parsed and validated. It goes last, because it consumes the
body.

```rust
async fn signup(form: Outcome<Signup>) -> Response {
    match form {
        Outcome::Valid(signup) => Html(format!("Welcome, {}!", signup.email)).into_response(),
        // The re-render carries what the user typed and what was wrong with it.
        Outcome::Invalid { view, .. } => {
            (StatusCode::UNPROCESSABLE_ENTITY, Html(view.to_html())).into_response()
        }
    }
}
```

A failed validation is **not** an extractor rejection — the handler still runs
and decides what to do with the re-render. `FormRejection` covers only the cases
where there is no submission to validate: a body that is not
`application/x-www-form-urlencoded` (415), one that could not be read, or one
that is not UTF-8 (400). As with axum's own `Form`, `GET` and `HEAD` are read
from the query string.

An extractor has nothing but the request, so `Outcome<T>` extracts only a form
whose `Context` is `()`. A form that asks for one is submitted in the handler,
where the context is: take the body, then call `T::submit_with_context`.

The feature pulls in `axum-core`, not `axum`. See
`examples/axum_signup.rs`, run with
`cargo run --example axum_signup --features axum`.

`multipart/form-data` bodies are out of scope: parse them with a multipart crate
and feed the text fields in through `Values::from_pairs`.

## Layout

| | |
|---|---|
| `src/spec.rs` | `FormSpec`/`FieldSpec`/`Control`/`Text` — the `const` description the derive emits |
| `src/view.rs` | `FormView`/`FieldView` — the render format, plus the built-in HTML |
| `src/runtime.rs` | `ParseCtx` — the error-collecting parse, and the `const` control assembly |
| `src/context.rs` | `Provides`, and how a `default`/`validate` function of either arity reaches the context |
| `src/validate.rs` | Server-side re-checking of every HTML constraint |
| `src/value.rs` | `FormValue` — Rust types ↔ submitted strings |
| `web-form-derive/` | The derive macros |
