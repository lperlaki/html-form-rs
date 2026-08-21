//! Many inputs under one name, served by axum, rendered by MiniJinja.
//!
//! The form has a list of IP addresses. The browser submits them as repeated
//! `name="ip"` pairs — a little JavaScript adds and removes rows — and the
//! crate parses them into a `Vec`. On a re-render `FieldView::values` holds
//! every value the user submitted, so the template puts the rows back.
//!
//! An error carries the input it came from. `FormErrors` keys errors by field
//! name, and one name is what all the rows share, so the row is
//! [`FieldError::index`], and in the view it is `field.error_indexes[i]`,
//! alongside `field.errors[i]`. The template puts each message under the input
//! that caused it.
//!
//! The index counts every value the browser sent under the name, blank rows
//! included, so it lines up with `field.values` and with the inputs on the
//! page. Counting only the values that parsed would drift: a row the user added
//! and left empty never reaches the parse.
//!
//! `each_message_lands_on_its_own_input` asserts the whole of that, and runs
//! before the server comes up.
//!
//! Run with: `cargo run --example multi_ip --features axum`
//! then open <http://127.0.0.1:3000>.

use std::borrow::Cow;
use std::net::IpAddr;

use axum::Router;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use html_form::{Control, Form, FormValue, FormView, Outcome, ValueError};
use minijinja::{Environment, context};

/// `IpAddr` is `FromStr`, so `#[field(from_str)]` would do this too. It is
/// written out here only to keep the example to one moving part.
#[derive(Debug)]
struct Ip(IpAddr);

impl FormValue for Ip {
    const CONTROL: Control = Control::TEXT;

    fn parse_form_value(raw: &str) -> Result<Self, ValueError> {
        raw.parse()
            .map(Ip)
            .map_err(|_| ValueError::new("an IP address"))
    }

    fn to_form_value(&self) -> Cow<'static, str> {
        Cow::Owned(self.0.to_string())
    }
}

#[derive(Form, Debug)]
#[form(
    action = "/allowlist",
    method = "post",
    submit = "Save allowlist",
    renderer = page
)]
struct Allowlist {
    #[field(label = "Rule name", maxlength = 40)]
    name: String,

    #[field(label = "Allowed addresses", placeholder = "203.0.113.7")]
    ip: Vec<Ip>,
}

const TEMPLATE: &str = r#"
<form action="{{ form.action }}" method="{{ form.method }}">
  {%- for field in form.fields %}
  {%- if field.name == "ip" %}
  <fieldset class="repeat" id="ip-rows">
    <legend>{{ field.label }}</legend>
    {%- for value in field.values or [""] %}
    {%- set row = loop.index0 %}
    <div class="row">
      <input type="text" name="{{ field.name }}" value="{{ value }}"
             placeholder="{{ field.placeholder }}">
      <button type="button" class="remove">×</button>
      {%- for message in field.errors %}
      {%- if field.error_indexes[loop.index0] == row %}
      <p class="error">{{ message }}</p>
      {%- endif %}
      {%- endfor %}
    </div>
    {%- endfor %}
    {%- for message in field.errors %}
    {%- if field.error_indexes[loop.index0] is none %}
    <p class="error">{{ message }}</p>
    {%- endif %}
    {%- endfor %}
  </fieldset>
  <button type="button" id="add-ip">Add address</button>
  {%- else %}
  <div class="field">
    <label for="{{ field.id }}">{{ field.label }}</label>
    <input type="{{ field.input_type }}" name="{{ field.name }}" id="{{ field.id }}"
           value="{{ field.value or "" }}"{% if field.required %} required{% endif %}>
    {%- for message in field.errors %}
    <p class="error">{{ message }}</p>
    {%- endfor %}
  </div>
  {%- endif %}
  {%- endfor %}
  <button type="submit">{{ form.submit_label }}</button>
</form>
<script>
  const rows = document.getElementById("ip-rows");
  document.getElementById("add-ip").onclick = () => {
    const row = rows.querySelector(".row").cloneNode(true);
    row.querySelectorAll("input").forEach(i => i.value = "");
    row.querySelectorAll(".error").forEach(p => p.remove());
    rows.append(row);
  };
  rows.onclick = e => {
    if (e.target.matches(".remove") && rows.querySelectorAll(".row").length > 1)
      e.target.closest(".row").remove();
  };
</script>
"#;

/// The renderer the form named: the blank form, and the one a rejected
/// submission comes back as, are the same page.
fn page(view: FormView) -> Html<String> {
    let mut env = Environment::new();
    env.add_template("allowlist.html", TEMPLATE).unwrap();
    let form = env
        .get_template("allowlist.html")
        .unwrap()
        .render(context! { form => view })
        .unwrap();

    Html(format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>Allowlist</title>\n<style>.error {{ color: crimson }}</style>\n\
         </head>\n<body>\n<h1>Allowlist</h1>\n{form}\n</body>\n</html>\n"
    ))
}

#[tokio::main]
async fn main() {
    each_message_lands_on_its_own_input();

    let app = Router::new()
        .route("/", get(async || page(Allowlist::render())))
        .route("/allowlist", post(saved));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

/// The submission, once every row parsed. A row that did not never reaches
/// here: the extractor answers with `page` and the form again.
async fn saved(allowlist: Allowlist) -> impl IntoResponse {
    println!("saved {allowlist:?}");

    let rows: String = allowlist
        .ip
        .iter()
        .map(|ip| format!("<li><code>{}</code></li>\n", ip.0))
        .collect();
    Html(format!(
        "<!doctype html>\n<title>Saved</title>\n<h1>Saved</h1>\n\
         <p>{} allows:</p>\n<ul>\n{rows}</ul>\n<p><a href=\"/\">Back</a></p>\n",
        html_form::escape(&allowlist.name)
    ))
}

/// Two good rows, one the user added and left empty, and two typos.
fn each_message_lands_on_its_own_input() {
    let body = "name=Office&ip=203.0.113.7&ip=192.168.1.999&ip=&ip=nope&ip=2001:db8::1";
    let Outcome::Invalid { errors, mut view } = Allowlist::submit_urlencoded(body) else {
        panic!("unexpectedly accepted");
    };

    // Each error names the input it came from, counted over every row the
    // browser sent — the empty row included, which is why the last is 3 and
    // not 2.
    let at: Vec<Option<usize>> = errors.field("ip").map(|error| error.index).collect();
    assert_eq!(at, [Some(1), Some(3)]);

    let field = view.field("ip").unwrap();
    assert_eq!(
        field.values,
        ["203.0.113.7", "192.168.1.999", "", "nope", "2001:db8::1"]
    );
    assert_eq!(field.error_indexes, at);

    // Whatever finds a problem later says which row it was about, the same way.
    view.field_mut("ip")
        .unwrap()
        .add_error_at(4, "Already in another rule.");

    // Every message sits in its own row.
    let Html(html) = page(*view);
    let rows: Vec<&str> = html.split(r#"<div class="row">"#).skip(1).collect();
    fn message(row: &str) -> Option<&str> {
        row.split_once(r#"<p class="error">"#)
            .map(|(_, m)| m.trim_start())
    }
    assert!(message(rows[0]).is_none());
    assert!(
        message(rows[1])
            .unwrap()
            .starts_with("Enter an IP address.")
    );
    assert!(message(rows[2]).is_none());
    assert!(
        message(rows[3])
            .unwrap()
            .starts_with("Enter an IP address.")
    );
    assert!(
        message(rows[4])
            .unwrap()
            .starts_with("Already in another rule.")
    );
}
