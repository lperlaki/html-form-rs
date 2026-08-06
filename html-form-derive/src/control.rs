//! How the crate builds the [`Control`] of a field or of a type out of what you
//! wrote about it.
//!
//! The macro cannot see what `<T as FormValue>::CONTROL` is. A field's control
//! can come from its Rust type, and the control of a `#[derive(FormValue)]`
//! type can come from the type it wraps. Nothing is decided here. This module
//! passes three things to `__private::control`: the implied control, the one
//! `type = "..."` names, and the declared options. It also passes every
//! attribute that belongs *inside* a control. `__private::control` is a
//! `const fn`, so an attribute with nowhere to go is a compile error where the
//! form or the type is declared.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Error, Ident, Result};

use crate::attrs::{Constraints, OptionAttr, opt_str, opt_u32, opt_usize};

/// The `Control` const for one field or one type.
///
/// `implied` is what the Rust type says. `options` is any `#[option(...)]`
/// entry written next to `choices`. `multiple` is what the shape of the field
/// implies before you write `multiple` on it. A `Vec<T>` submits more than one
/// value by nature, though the control decides whether that renders as an
/// attribute.
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
        ::html_form::__private::control(
            #implied,
            #explicit,
            #choices,
            ::html_form::__private::Overrides {
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

/// The empty control that `type = "..."` names, before any attribute goes in.
fn control_skeleton(name: &str, span: Span) -> Result<TokenStream> {
    let text = |format: &str| {
        let format = Ident::new(format, span);
        quote!(::html_form::Control::Text(::html_form::TextControl {
            format: ::html_form::TextFormat::#format,
            ..::html_form::TextControl::DEFAULT
        }))
    };
    let number = |format: &str| {
        let format = Ident::new(format, span);
        quote!(::html_form::Control::Number(::html_form::NumberControl {
            format: ::html_form::NumberFormat::#format,
            ..::html_form::NumberControl::DEFAULT
        }))
    };
    let temporal = |format: &str| {
        let format = Ident::new(format, span);
        quote!(::html_form::Control::Temporal(::html_form::TemporalControl {
            format: ::html_form::TemporalFormat::#format,
            ..::html_form::TemporalControl::DEFAULT
        }))
    };
    let choose = |style: &str| {
        let style = Ident::new(style, span);
        quote!(::html_form::Control::Choose(::html_form::ChooseControl {
            style: ::html_form::ChoiceStyle::#style,
            ..::html_form::ChooseControl::DEFAULT
        }))
    };

    Ok(match name {
        "text" => text("Text"),
        "password" => text("Password"),
        "tel" => text("Tel"),
        "search" => text("Search"),
        "url" => text("Url"),
        "email" => quote!(::html_form::Control::Text(::html_form::TextControl {
            format: ::html_form::TextFormat::Email { multiple: false },
            ..::html_form::TextControl::DEFAULT
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
        "textarea" => quote!(::html_form::Control::Textarea(
            ::html_form::TextareaControl::DEFAULT
        )),
        "file" => quote!(::html_form::Control::File(
            ::html_form::FileControl::DEFAULT
        )),
        "checkbox" => quote!(::html_form::Control::Checkbox),
        "color" => quote!(::html_form::Control::Color),
        "hidden" => quote!(::html_form::Control::Hidden),
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
            // An option with no label of its own uses its value as the label.
            // That value is text, never a key.
            let label = match &option.label {
                Some(label) => label.tokens(),
                None => {
                    let text = &option.value;
                    quote!(::html_form::Text {
                        content: ::std::borrow::Cow::Borrowed(#text),
                        is_key: false,
                    })
                }
            };
            let disabled = option.disabled;
            let group = crate::attrs::opt_text(&option.group);
            quote! {
                ::html_form::Choice {
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
