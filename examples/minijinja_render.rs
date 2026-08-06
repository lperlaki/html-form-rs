//! Rendering a form through MiniJinja instead of the built-in renderer.
//!
//! `FormView` is `serde::Serialize`, so it drops straight into a template
//! context and the template decides what the markup looks like. The same view
//! type is used for the blank form and for the re-render after a failed
//! submission — the only difference is that the second one carries values and
//! error messages.
//!
//! Run with: `cargo run --example minijinja_render`

use html_form::{Form, Outcome};
use minijinja::{Environment, context};

#[derive(Form, Debug)]
struct FormId {
    #[field(type = "hidden")]
    id: String,
}

#[derive(Form, Debug)]
#[form(action = "/signup", method = "post", submit = "Create account")]
struct Signup {
    #[field(
        type = "email",
        label = "Email address",
        autocomplete = "email",
        placeholder = "you@example.com"
    )]
    email: String,

    #[field(
        type = "password",
        label = "Password",
        minlength = 12,
        help = "At least 12 characters."
    )]
    password: String,

    #[field(label = "Age", min = 18, max = 120)]
    age: Option<u32>,

    #[field(label = "Plan")]
    #[option("free", "Free")]
    #[option("pro", "Professional")]
    plan: String,

    #[field(type = "textarea", label = "About you", rows = 3, maxlength = 400)]
    bio: Option<String>,

    #[field(label = "Send me the newsletter", default = true)]
    newsletter: bool,

    #[field(flatten)]
    id: FormId,
}

const TEMPLATE: &str = r#"
<form action="{{ form.action }}" method="{{ form.method }}" class="signup">
  {%- for message in form.errors %}
  <p class="form-error">{{ message }}</p>
  {%- endfor %}
  {%- for field in form.fields %}
  <div class="field{% if field.has_errors %} field--invalid{% endif %}">
    {%- if field.label %}
    <label for="{{ field.id }}">{{ field.label }}{% if field.required %} *{% endif %}</label>
    {%- endif %}
    {%- if field.element == "select" %}
    <select name="{{ field.name }}" id="{{ field.id }}"{% if field.required %} required{% endif %}>
      {%- for choice in field.choices %}
      <option value="{{ choice.value }}"{% if choice.selected %} selected{% endif %}>{{ choice.label }}</option>
      {%- endfor %}
    </select>
    {%- elif field.element == "textarea" %}
    <textarea name="{{ field.name }}" id="{{ field.id }}" rows="{{ field.rows or 3 }}"
              maxlength="{{ field.maxlength or 1000 }}">{{ field.value or "" }}</textarea>
    {%- else %}
    <input type="{{ field.input_type }}" name="{{ field.name }}" id="{{ field.id }}"
           value="{{ field.value or "" }}"
           {%- if field.placeholder %} placeholder="{{ field.placeholder }}"{% endif %}
           {%- if field.pattern %} pattern="{{ field.pattern }}"{% endif %}
           {%- if field.minlength %} minlength="{{ field.minlength }}"{% endif %}
           {%- if field.min %} min="{{ field.min }}"{% endif %}
           {%- if field.max %} max="{{ field.max }}"{% endif %}
           {%- if field.checked %} checked{% endif %}
           {%- if field.required %} required{% endif %}>
    {%- endif %}
    {%- if field.help %}
    <p class="help">{{ field.help }}</p>
    {%- endif %}
    {%- for message in field.errors %}
    <p class="error">{{ message }}</p>
    {%- endfor %}
  </div>
  {%- endfor %}
  <button type="submit">{{ form.submit_label }}</button>
</form>
"#;

fn main() {
    let mut env = Environment::new();
    env.add_template("signup.html", TEMPLATE).unwrap();
    let template = env.get_template("signup.html").unwrap();

    // 1. GET /signup — a blank form, showing each field's default.
    println!("── GET /signup ─────────────────────────────────────────────");
    let html = template
        .render(context! { form => Signup::render() })
        .unwrap();
    println!("{html}");

    // 2. POST /signup with a bad body — same template, same view type, now
    //    carrying what the user typed and what was wrong with it.
    let body = "email=not-an-email&password=short&age=7&plan=free&bio=Hi&newsletter=on";
    println!("── POST /signup (rejected) ─────────────────────────────────");
    match Signup::submit_urlencoded(body) {
        Outcome::Valid(signup) => println!("unexpectedly accepted: {signup:?}"),
        Outcome::Invalid { errors, view } => {
            println!("// {} error(s): {errors}\n", errors.len());
            println!("{}", template.render(context! { form => view }).unwrap());
        }
    }

    // 3. POST /signup with a good body — straight to a typed struct.
    let body = "email=ada@example.com&password=correct-horse-battery&age=36&plan=pro&newsletter=on";
    println!("── POST /signup (accepted) ─────────────────────────────────");
    match Signup::submit_urlencoded(body) {
        Outcome::Valid(signup) => println!("{signup:#?}"),
        Outcome::Invalid { errors, .. } => println!("unexpectedly rejected: {errors}"),
    }
}
