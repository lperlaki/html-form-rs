#![allow(dead_code)]

//! The built-in renderer: what a [`FormView`] and a [`FieldView`] write out.
//!
//! The markup is plain and unstyled, and every element carries a `html-form__*`
//! class to hook CSS onto. These tests pin down the parts of it that a template
//! engine would otherwise have to guess at: where a `<fieldset>` opens and
//! closes, which label wraps which control, and what a group of options
//! becomes.

use html_form::{FieldKind, Form, FormChoice, FormErrors, FormView, Values, escape};

#[derive(FormChoice, Debug, PartialEq)]
enum Plan {
    Free,
    Pro,
}

#[derive(Form, Debug)]
#[form(
    id = "signup",
    name = "signup",
    action = "/signup",
    method = "post",
    class = "card wide",
    novalidate,
    submit_label = "Create account"
)]
struct Signup {
    #[field(type = "email", label = "Email address", help = "We never share it.")]
    email: String,

    #[field(type = "textarea", label = "About you", rows = 4, cols = 40)]
    about: String,

    #[field(label = "Plan")]
    plan: Plan,

    #[field(label = "I accept the terms")]
    terms: bool,

    #[field(type = "hidden", default = "web")]
    source: String,
}

fn view() -> FormView {
    Signup::render()
}

// ─── The form element ─────────────────────────────────────────────────────────

#[test]
fn the_form_element_carries_everything_the_spec_declared() {
    let html = view().to_html();
    assert!(html.starts_with(
        r#"<form id="signup" name="signup" action="/signup" method="post" class="html-form card wide" novalidate>"#
    ));
    assert!(html.ends_with(
        "  <button type=\"submit\" class=\"html-form__submit\">Create account</button>\n</form>"
    ));
}

/// A form that declares nothing still renders. `post` is the method, because
/// that is what a form the crate parses is for.
#[test]
fn a_form_that_declares_nothing_still_renders_a_usable_element() {
    #[derive(Form)]
    struct Bare {
        name: String,
    }

    let html = Bare::render().to_html();
    assert!(html.starts_with(r#"<form method="post" class="html-form">"#));
    assert!(html.contains(">Submit</button>"));
}

/// The method a form declares reaches the view as the attribute value, so a
/// template need not map the enum itself.
#[test]
fn each_method_and_enctype_reaches_the_markup_as_written() {
    #[derive(Form)]
    #[form(method = "get")]
    struct Search {
        q: String,
    }

    #[derive(Form)]
    #[form(method = "dialog")]
    struct Confirm {
        note: String,
    }

    #[derive(Form)]
    #[form(enctype = "multipart/form-data")]
    struct Upload {
        #[field(type = "file")]
        avatar: String,
    }

    #[derive(Form)]
    #[form(enctype = "text/plain")]
    struct Plain {
        note: String,
    }

    #[derive(Form)]
    #[form(enctype = "application/x-www-form-urlencoded")]
    struct Urlencoded {
        note: String,
    }

    assert_eq!(Search::render().method, "get");
    assert_eq!(Confirm::render().method, "dialog");
    assert_eq!(
        Upload::render().enctype,
        Some("multipart/form-data"),
        "which the file input needs to arrive at all"
    );
    assert_eq!(Plain::render().enctype, Some("text/plain"));
    assert_eq!(
        Urlencoded::render().enctype,
        Some("application/x-www-form-urlencoded"),
        "written out even though it is also the default a browser assumes"
    );
    assert_eq!(Signup::render().enctype, None);

    assert!(Search::render().to_html().contains(r#"method="get""#));
    assert!(
        Upload::render()
            .to_html()
            .contains(r#"enctype="multipart/form-data""#)
    );
}

/// A form-level error belongs to no field, so it renders as a list above them
/// rather than beside one of them.
#[test]
fn a_form_level_error_renders_above_the_fields() {
    let mut view = view();
    assert!(!view.to_html().contains("html-form__errors"));

    view.add_error("The two passwords do not match.");
    view.add_error("And the card was declined.");

    let html = view.to_html();
    assert!(html.contains(
        "  <ul class=\"html-form__errors\">\n    <li>The two passwords do not match.</li>\n\
         \x20   <li>And the card was declined.</li>\n  </ul>\n"
    ));
    // It opens before the first field.
    assert!(html.find("html-form__errors").unwrap() < html.find("data-field").unwrap());
}

/// One `<fieldset>` wraps the fields that follow each other under the same
/// legend. That is how a flattened sub-form keeps its identity in the markup.
#[test]
fn each_flattened_group_opens_and_closes_one_fieldset() {
    #[derive(Form)]
    struct Address {
        street: String,
        city: String,
    }

    #[derive(Form)]
    struct Order {
        email: String,
        #[field(flatten, prefix = "billing_", legend = "Billing")]
        billing: Address,
        #[field(flatten, prefix = "shipping_", legend = "Shipping")]
        shipping: Address,
        note: String,
    }

    let html = Order::render().to_html();
    assert_eq!(html.matches("<fieldset").count(), 2);
    assert_eq!(html.matches("</fieldset>").count(), 2);
    assert!(html.contains("<legend>Billing</legend>"));
    assert!(html.contains("<legend>Shipping</legend>"));

    // The ungrouped field before the groups is outside them, and the one after
    // closes the last group before it renders.
    let first = html.find("<fieldset").unwrap();
    assert!(html.find(r#"data-field="email""#).unwrap() < first);
    let last_close = html.rfind("</fieldset>").unwrap();
    assert!(html.find(r#"data-field="note""#).unwrap() > last_close);
}

/// A group with no legend is not a group in the markup. Its fields sit
/// alongside the enclosing form's own.
#[test]
fn a_flatten_without_a_legend_opens_no_fieldset() {
    #[derive(Form)]
    struct Address {
        street: String,
    }

    #[derive(Form)]
    struct Order {
        #[field(flatten)]
        billing: Address,
    }

    let html = Order::render().to_html();
    assert!(!html.contains("<fieldset"));
    assert!(html.contains(r#"data-field="street""#));
}

// ─── One field at a time ──────────────────────────────────────────────────────

/// A field renders on its own, for a template that lays the form out itself
/// and wants only the parts the crate generates.
#[test]
fn a_field_renders_on_its_own_with_and_without_its_label() {
    let view = view();
    let email = view.field("email").unwrap();

    let whole = email.to_html();
    assert!(whole.contains(r#"<label class="html-form__label" for="email">Email address"#));
    assert!(whole.contains(r#"<input type="email""#));
    assert!(whole.contains(r#"<p class="html-form__help" id="email-help">We never share it.</p>"#));

    // The control alone has none of that around it.
    let control = email.control_html();
    assert!(control.starts_with(r#"<input type="email""#));
    assert!(!control.contains("<label"));
    assert!(!control.contains("html-form__help"));
}

/// `Display` renders the same markup, so a view drops straight into a
/// `format!` or a `write!`.
#[test]
fn displaying_a_view_renders_it() {
    let view = view();
    assert_eq!(view.to_string(), view.to_html());

    let email = view.field("email").unwrap();
    assert_eq!(email.to_string(), email.to_html());
    assert_eq!(format!("{email}"), email.to_html());
}

/// A hidden field has nothing to label and nothing to describe, so it renders
/// as the bare control.
#[test]
fn a_hidden_field_renders_as_the_control_and_nothing_else() {
    let view = view();
    let source = view.field("source").unwrap();
    assert_eq!(source.kind, FieldKind::Hidden);
    assert_eq!(
        source.to_html().trim_end(),
        r#"<input type="hidden" name="source" id="source" value="web">"#
    );
}

/// The label of a checkbox comes *after* the box, which is how a browser lays
/// one out and what a click on the text has to hit.
#[test]
fn a_checkbox_is_labelled_after_the_box() {
    let view = view();
    let html = view.field("terms").unwrap().to_html();

    let control = html.find(r#"type="checkbox""#).unwrap();
    let label = html.find("I accept the terms").unwrap();
    assert!(control < label);
}

#[test]
fn a_required_field_marks_its_label() {
    let view = view();
    assert!(
        view.field("email")
            .unwrap()
            .to_html()
            .contains(r#"<span class="html-form__required" aria-hidden="true">*</span>"#)
    );
    // A `bool` is a box the user may leave alone, so nothing marks it.
    assert!(
        !view
            .field("terms")
            .unwrap()
            .to_html()
            .contains("__required")
    );
}

#[test]
fn a_textarea_carries_its_size_and_holds_its_value_as_content() {
    let filled = Signup {
        email: "ada@example.com".to_owned(),
        about: "I write <programs>.".to_owned(),
        plan: Plan::Pro,
        terms: true,
        source: "web".to_owned(),
    };
    let html = filled
        .render_filled()
        .field("about")
        .unwrap()
        .control_html();

    assert!(html.starts_with("<textarea"));
    assert!(html.contains(r#"rows="4""#) && html.contains(r#"cols="40""#));
    // The value is content, not an attribute, and it is escaped either way.
    assert!(html.ends_with("I write &lt;programs&gt;.</textarea>"));

    // An empty textarea is still a closed element.
    assert!(
        view()
            .field("about")
            .unwrap()
            .control_html()
            .ends_with("></textarea>")
    );
}

// ─── The controls that are more than one element ──────────────────────────────

#[test]
fn a_select_groups_its_options_under_optgroups() {
    #[derive(Form)]
    struct Trip {
        #[option("ber", "Berlin", group = "Germany")]
        #[option("muc", "Munich", group = "Germany")]
        #[option("zrh", "Zurich", group = "Switzerland")]
        #[option("elsewhere", "Elsewhere")]
        city: String,
    }

    let html = Trip::render().field("city").unwrap().control_html();
    assert_eq!(html.matches("<optgroup").count(), 2);
    assert_eq!(html.matches("</optgroup>").count(), 2);
    assert!(html.contains(r#"<optgroup label="Germany">"#));
    // An option with no group closes the one before it and stands on its own.
    let last_close = html.rfind("</optgroup>").unwrap();
    assert!(html.find(r#"value="elsewhere""#).unwrap() > last_close);
}

/// A single-valued select that is not required needs an empty option, so the
/// user can say "nothing".
#[test]
fn an_optional_select_offers_an_empty_option_and_a_required_one_does_not() {
    #[derive(Form)]
    struct Choices {
        plan: Option<Plan>,
        required: Plan,
        many: Vec<Plan>,
    }

    let view = Choices::render();
    assert!(
        view.field("plan")
            .unwrap()
            .control_html()
            .contains(r#"<option value="" selected></option>"#)
    );
    assert!(
        !view
            .field("required")
            .unwrap()
            .control_html()
            .contains(r#"value="""#)
    );
    assert!(
        !view
            .field("many")
            .unwrap()
            .control_html()
            .contains(r#"value="""#)
    );
}

/// A group labels each option, so the group itself gets a plain caption and an
/// `aria-labelledby` in place of a `for=` that would point at one box.
#[test]
fn a_radio_group_captions_itself_and_labels_each_option() {
    #[derive(Form)]
    struct Pick {
        #[field(type = "radio", label = "Plan")]
        plan: Plan,
    }

    let html = Pick::render().field("plan").unwrap().to_html();
    assert!(html.contains(r#"<span class="html-form__label" id="plan-label">Plan "#));
    assert!(html.contains(r#"role="radiogroup" aria-labelledby="plan-label""#));
    assert!(!html.contains("<label class=\"html-form__label\" for="));

    // One box per option, each with an id of its own.
    assert_eq!(html.matches(r#"type="radio""#).count(), 2);
    assert!(html.contains(r#"id="plan-0""#) && html.contains(r#"id="plan-1""#));
    assert!(html.contains(r#"value="free""#) && html.contains(r#"value="pro""#));
}

#[test]
fn a_checkbox_group_announces_a_requirement_the_browser_cannot_enforce() {
    #[derive(Form)]
    struct Pick {
        #[field(type = "checkbox", label = "Plans")]
        plans: Vec<Plan>,
    }

    let html = Pick::render().field("plans").unwrap().to_html();
    assert!(html.contains(r#"role="group" aria-labelledby="plans-label""#));
    assert_eq!(html.matches(r#"type="checkbox""#).count(), 2);
    // Each box carries the value it submits, unlike a lone boolean box.
    assert!(html.contains(r#"value="free""#));
}

/// A lone radio or checkbox with no options is one box, so it is labelled the
/// way a checkbox is rather than captioned like a group.
#[test]
fn a_choice_control_with_no_options_falls_back_to_one_box() {
    let mut view = view();
    let terms = view.field_mut("terms").unwrap();
    terms.kind = FieldKind::Radio;
    terms.choices.clear();

    let html = terms.to_html();
    assert!(html.contains(r#"<label class="html-form__label" for="terms">"#));
    assert!(!html.contains("radiogroup"));
}

// ─── Errors and accessibility ─────────────────────────────────────────────────

#[test]
fn a_field_that_failed_is_marked_and_points_at_its_own_messages() {
    let errors = Signup::from_urlencoded("email=nope&about=x&plan=free").unwrap_err();
    let view = Signup::render_submitted(&Values::parse("email=nope&about=x&plan=free"), &errors);
    let html = view.field("email").unwrap().to_html();

    assert!(html.contains("html-form__field--invalid"));
    assert!(html.contains(r#"<ul class="html-form__errors" id="email-error">"#));
    assert!(html.contains("<li>Enter a valid email address.</li>"));
    // The control names both the help text and the errors, in that order.
    assert!(html.contains(r#"aria-describedby="email-help email-error""#));
    assert!(html.contains(r#"aria-invalid="true""#));
}

/// With only one of the two, the control names only that one.
#[test]
fn a_control_names_whichever_of_help_and_errors_it_has() {
    let view = view();
    assert!(
        view.field("email")
            .unwrap()
            .to_html()
            .contains(r#"aria-describedby="email-help""#)
    );
    assert!(
        !view
            .field("about")
            .unwrap()
            .to_html()
            .contains("aria-describedby")
    );

    let mut with_error = view;
    with_error.add_field_error("about", "Say something.");
    assert!(
        with_error
            .field("about")
            .unwrap()
            .to_html()
            .contains(r#"aria-describedby="about-error""#)
    );
}

// ─── Escaping ─────────────────────────────────────────────────────────────────

#[test]
fn escaping_covers_every_character_that_ends_an_attribute_or_a_tag() {
    assert_eq!(escape("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&#39;f");
    // Text with nothing to escape comes back as it is, which is most text.
    assert_eq!(escape("plain text"), "plain text");
    assert!(matches!(
        escape("plain text"),
        std::borrow::Cow::Borrowed(_)
    ));
    assert!(matches!(escape("<"), std::borrow::Cow::Owned(_)));
    assert_eq!(escape(""), "");
    assert_eq!(escape("<<<"), "&lt;&lt;&lt;", "one after another");
    assert_eq!(
        escape("\u{e9}<\u{e9}"),
        "\u{e9}&lt;\u{e9}",
        "around a multi-byte character"
    );
}

/// Every string the renderer writes goes through it, wherever a form's own
/// data ends up in the markup.
#[test]
fn what_a_user_typed_cannot_break_out_of_the_markup() {
    let attack = r#""><script>alert(1)</script>"#;
    let values = Values::from_pairs([("email", attack), ("about", attack)]);
    let mut errors = FormErrors::new();
    errors.reject_field("email", attack);

    let html = Signup::render_submitted(&values, &errors).to_html();
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
}

/// An attribute *value* is escaped, so it may be anything. A **name** sits
/// outside the quotes, where a space would end it and start a second attribute
/// nobody declared — and no escaping helps, because a space needs none. The
/// renderer therefore writes only names made of what a name may be made of.
#[test]
fn an_attribute_name_cannot_forge_a_second_attribute() {
    let mut view = Signup::render();
    let field = view.field_mut("email").expect("the email field");

    // Each of these would render a live `onfocus` handler if the name went
    // through as written. `set_attr` says no and sets nothing, so the name
    // never reaches the view — which is what a template engine reads.
    assert!(!field.set_attr(r#"x onfocus="alert(1)""#, Some("y")));
    assert!(!field.set_attr("x\tonfocus=alert(1)", None));
    assert!(!field.set_attr(r#"x">"#, Some("y")));
    assert!(!field.set_attr("", Some("y")));
    assert!(field.attrs.is_empty(), "{:?}", field.attrs);

    // And the value is what may hold anything, because that one is escaped.
    assert!(field.set_attr("data-note", Some(r#""><script>alert(1)</script>"#)));

    let html = view.to_html();
    assert!(!html.contains("onfocus"), "{html}");
    assert!(!html.contains("<script>"), "{html}");
    assert!(
        html.contains("data-note=\"&quot;&gt;&lt;script&gt;"),
        "{html}"
    );

    // What a real attribute is made of still goes through untouched.
    let mut view = Signup::render();
    let field = view.field_mut("email").expect("the email field");
    assert!(field.set_attr("data-role", Some("primary")));
    assert!(field.set_attr("hx-trigger", Some("keyup changed delay:300ms")));
    assert!(field.set_attr("autocorrect", None));

    let html = view.to_html();
    assert!(html.contains(r#"data-role="primary""#), "{html}");
    assert!(
        html.contains(r#"hx-trigger="keyup changed delay:300ms""#),
        "{html}"
    );
    assert!(html.contains(" autocorrect"), "{html}");
}

/// The guard is the view's, not the renderer's. A template engine reads the
/// `attrs` list itself, so a name that only `to_html` refused would still reach
/// a page through MiniJinja or Askama — writing `{{ a.name }}="{{ a.value }}"`
/// re-creates the very injection above, and autoescaping cannot stop it because
/// a space needs no escape. `set_attr` is where the name is refused, so every
/// renderer gets the same answer.
#[test]
fn a_refused_attribute_never_reaches_a_template_either() {
    let mut view = Signup::render();
    let field = view.field_mut("email").expect("the email field");

    assert!(!field.set_attr("x onfocus=alert(1)", Some("y")));

    // What a template iterates over, and what `serde` hands it.
    assert!(field.attrs.is_empty());
    let json = serde_json::to_string(&view).expect("the view serializes");
    assert!(!json.contains("onfocus"), "{json}");
}

/// A custom attribute the crate already writes from the spec would render
/// twice. The derive rejects those names while the crate compiles; a name built
/// at run time is refused here, in place of making invalid markup that browsers
/// then read as the crate's own value anyway.
#[test]
fn an_attribute_the_renderer_writes_itself_cannot_be_set_a_second_time() {
    let mut view = Signup::render();
    let field = view.field_mut("email").expect("the email field");

    for name in ["name", "id", "type", "required", "VALUE"] {
        assert!(!field.set_attr(name, Some("forged")), "{name}");
    }
    assert!(field.attrs.is_empty(), "{:?}", field.attrs);

    let html = view.to_html();
    assert!(!html.contains("forged"), "{html}");
    // The control still carries exactly one of each name it was given.
    let input = html
        .lines()
        .find(|line| line.contains(r#"type="email""#))
        .expect("the email control");
    assert_eq!(input.matches(" name=").count(), 1, "{input}");
    assert_eq!(input.matches(" required").count(), 1, "{input}");

    // The `<form>` has its own set.
    assert!(!view.set_attr("method", Some("get")));
    assert!(!view.set_attr("novalidate", None));
    assert!(view.set_attr("hx-boost", Some("true")));
}
