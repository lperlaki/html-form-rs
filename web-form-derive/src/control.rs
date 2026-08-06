//! Assembling a field's or a type's [`Control`] out of what was written about
//! it.
//!
//! The macro cannot see what `<T as FormValue>::CONTROL` is — a field's control
//! can come from its Rust type, and a `#[derive(FormValue)]` type's from the
//! type it wraps — so nothing is decided here. The three ingredients (the
//! implied control, the one `type = "..."` names, the declared options) and
//! every attribute that belongs *inside* a control are handed to
//! `__private::control`, which is a `const fn`: an attribute that has nowhere
//! to go is a compile error at the point the form or the type is declared.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Error, Ident, Result};

use crate::attrs::{Constraints, OptionAttr, opt_str, opt_u32, opt_usize};

/// The `Control` const for one field or one type.
///
/// `implied` is what the Rust type says, `options` any `#[option(...)]` entries
/// written alongside `choices`, and `multiple` what the shape of the field
/// implies before `multiple` is written on it — a `Vec<T>` submits repeatedly
/// by nature, though whether that renders as an attribute depends on the
/// control.
pub fn control_tokens(
    constraints: &Constraints,
    options: &[OptionAttr],
    implied: TokenStream,
    multiple: bool,
) -> Result<TokenStream> {
    let explicit = match &constraints.kind {
        Some((name, span)) => {
            let skeleton = control_skeleton(name, *span)?;
            quote!(::core::option::Option::Some(#skeleton))
        }
        None => quote!(::core::option::Option::None),
    };

    let choices = choices_tokens(constraints, options)?;
    let multiple = constraints.multiple.unwrap_or(multiple);

    let pattern = opt_str(&constraints.pattern);
    let minlength = opt_usize(&constraints.minlength);
    let maxlength = opt_usize(&constraints.maxlength);
    let min = opt_str(&constraints.min);
    let max = opt_str(&constraints.max);
    let step = opt_str(&constraints.step);
    let accept = opt_str(&constraints.accept);
    let rows = opt_u32(&constraints.rows);
    let cols = opt_u32(&constraints.cols);

    Ok(quote! {
        ::web_form::__private::control(
            #implied,
            #explicit,
            #choices,
            ::web_form::__private::Overrides {
                pattern: #pattern,
                minlength: #minlength,
                maxlength: #maxlength,
                min: #min,
                max: #max,
                step: #step,
                accept: #accept,
                rows: #rows,
                cols: #cols,
                multiple: #multiple,
            },
        )
    })
}

/// The empty control that `type = "..."` names, with no attribute placed in it
/// yet.
fn control_skeleton(name: &str, span: Span) -> Result<TokenStream> {
    let text = |format: &str| {
        let format = Ident::new(format, span);
        quote!(::web_form::Control::Text(::web_form::TextControl {
            format: ::web_form::TextFormat::#format,
            ..::web_form::TextControl::DEFAULT
        }))
    };
    let number = |format: &str| {
        let format = Ident::new(format, span);
        quote!(::web_form::Control::Number(::web_form::NumberControl {
            format: ::web_form::NumberFormat::#format,
            ..::web_form::NumberControl::DEFAULT
        }))
    };
    let temporal = |format: &str| {
        let format = Ident::new(format, span);
        quote!(::web_form::Control::Temporal(::web_form::TemporalControl {
            format: ::web_form::TemporalFormat::#format,
            ..::web_form::TemporalControl::DEFAULT
        }))
    };
    let choose = |style: &str| {
        let style = Ident::new(style, span);
        quote!(::web_form::Control::Choose(::web_form::ChooseControl {
            style: ::web_form::ChoiceStyle::#style,
            ..::web_form::ChooseControl::DEFAULT
        }))
    };

    Ok(match name {
        "text" => text("Text"),
        "password" => text("Password"),
        "tel" => text("Tel"),
        "search" => text("Search"),
        "url" => text("Url"),
        "email" => quote!(::web_form::Control::Text(::web_form::TextControl {
            format: ::web_form::TextFormat::Email { multiple: false },
            ..::web_form::TextControl::DEFAULT
        })),
        "number" => number("Number"),
        "range" => number("Range"),
        "date" => temporal("Date"),
        "time" => temporal("Time"),
        "datetime-local" | "datetime_local" => temporal("DatetimeLocal"),
        "month" => temporal("Month"),
        "week" => temporal("Week"),
        "select" => choose("Select"),
        "radio" => choose("Radio"),
        "textarea" => quote!(::web_form::Control::Textarea(
            ::web_form::TextareaControl::DEFAULT
        )),
        "file" => quote!(::web_form::Control::File(::web_form::FileControl::DEFAULT)),
        "checkbox" => quote!(::web_form::Control::Checkbox),
        "color" => quote!(::web_form::Control::Color),
        "hidden" => quote!(::web_form::Control::Hidden),
        other => {
            return Err(Error::new(
                span,
                format!(
                    "unknown field type `{other}`; expected one of: {}",
                    KIND_NAMES.join(", ")
                ),
            ));
        }
    })
}

const KIND_NAMES: &[&str] = &[
    "text",
    "password",
    "email",
    "url",
    "tel",
    "search",
    "number",
    "range",
    "checkbox",
    "radio",
    "date",
    "time",
    "datetime-local",
    "month",
    "week",
    "color",
    "file",
    "hidden",
    "textarea",
    "select",
];

/// `Option<&'static [Choice]>` from `#[option(...)]` entries or `choices = ...`.
fn choices_tokens(constraints: &Constraints, options: &[OptionAttr]) -> Result<TokenStream> {
    if !options.is_empty() {
        if constraints.choices.is_some() {
            return Err(Error::new(
                Span::call_site(),
                "use either `#[option(...)]` entries or `choices = ...`, not both",
            ));
        }
        let items = options.iter().map(|option| {
            let value = &option.value;
            // An option with no label of its own is labelled by its value,
            // which is text, never a key.
            let label = match &option.label {
                Some(label) => label.tokens(),
                None => {
                    let text = &option.value;
                    quote!(::web_form::Text {
                        content: ::std::borrow::Cow::Borrowed(#text),
                        is_key: false,
                    })
                }
            };
            let disabled = option.disabled;
            let group = crate::attrs::opt_text(&option.group);
            quote! {
                ::web_form::Choice {
                    value: ::std::borrow::Cow::Borrowed(#value),
                    label: #label,
                    disabled: #disabled,
                    group: #group,
                }
            }
        });
        return Ok(quote!(::core::option::Option::Some(&[#(#items),*])));
    }
    Ok(match &constraints.choices {
        Some(path) => quote!(::core::option::Option::Some(#path)),
        None => quote!(::core::option::Option::None),
    })
}
