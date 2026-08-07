//! `#[derive(FormChoice)]`. A fieldless enum becomes a `<select>`.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, Result};

use crate::attrs::{ChoiceAttrs, opt_text};

pub fn derive(input: DeriveInput) -> Result<TokenStream> {
    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(
            input.generics,
            "`FormChoice` cannot be derived for a generic type",
        ));
    }

    let Data::Enum(data) = &input.data else {
        return Err(Error::new_spanned(
            &input.ident,
            "`FormChoice` can only be derived for enums",
        ));
    };
    if data.variants.is_empty() {
        return Err(Error::new_spanned(
            &input.ident,
            "`FormChoice` needs at least one variant",
        ));
    }

    let ident = &input.ident;
    let mut choices = Vec::new();
    let mut parse_arms = Vec::new();
    let mut render_arms = Vec::new();
    let mut accepted = Vec::new();

    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(Error::new_spanned(
                &variant.fields,
                "`FormChoice` variants cannot have fields: a form control submits one string",
            ));
        }
        let attrs = ChoiceAttrs::parse(&variant.attrs)?;
        let variant_ident = &variant.ident;
        let value = attrs
            .value
            .clone()
            .unwrap_or_else(|| kebab_case(&variant_ident.to_string()));
        // A label you write may be an i18n key. A label derived from the
        // variant's own name is always literal text.
        let label = match &attrs.label {
            Some(label) => label.tokens(),
            None => {
                let text = title_case(&variant_ident.to_string());
                quote!(::html_form::Text {
                    content: ::std::borrow::Cow::Borrowed(#text),
                    is_key: false,
                })
            }
        };
        let disabled = attrs.disabled;
        let group = opt_text(&attrs.group);

        choices.push(quote! {
            ::html_form::Choice {
                value: ::std::borrow::Cow::Borrowed(#value),
                label: #label,
                disabled: #disabled,
                group: #group,
            }
        });
        parse_arms.push(quote!(#value => ::core::result::Result::Ok(Self::#variant_ident)));
        render_arms.push(quote!(Self::#variant_ident => #value));
        accepted.push(value);
    }

    let expected = accepted.join(", ");

    Ok(quote! {
        #[automatically_derived]
        impl ::html_form::FormValue for #ident {
            const CONTROL: ::html_form::Control =
                ::html_form::Control::Choose(::html_form::ChooseControl {
                    choices: &[#(#choices),*],
                    ..::html_form::ChooseControl::DEFAULT
                });

            fn parse_form_value(
                __raw: &str,
            ) -> ::core::result::Result<Self, ::html_form::ValueError> {
                match __raw {
                    #(#parse_arms,)*
                    _ => ::core::result::Result::Err(::html_form::ValueError::new(
                        ::std::format!("one of: {}", #expected),
                    )),
                }
            }

            fn to_form_value(&self) -> ::std::borrow::Cow<'static, str> {
                ::std::borrow::Cow::Borrowed(match self {
                    #(#render_arms,)*
                })
            }
        }
    })
}

/// `NewYork` → `new-york`
fn kebab_case(ident: &str) -> String {
    let mut out = String::with_capacity(ident.len() + 4);
    for (index, ch) in ident.char_indices() {
        if ch.is_uppercase() {
            if index != 0 {
                out.push('-');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// `NewYork` → `New York`
fn title_case(ident: &str) -> String {
    let mut out = String::with_capacity(ident.len() + 4);
    for (index, ch) in ident.char_indices() {
        if ch.is_uppercase() && index != 0 {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}
