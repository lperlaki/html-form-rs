//! `#[derive(FormValue)]` — a type carries its own control, its own default and
//! its own check, so a form that uses it says only where it goes.
//!
//! Something already knows how to turn the type into a string and back: the one
//! value it wraps, or — with `#[value(from_str)]` — the type itself, through
//! `FromStr` and `Display`. This derive writes that conversion out and hangs
//! the rest off it.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Fields, Result, Type};

use crate::attrs::{ValueAttrs, opt_str};
use crate::control::control_tokens;
use crate::form::mentions;

pub fn derive(input: DeriveInput) -> Result<TokenStream> {
    let attrs = ValueAttrs::parse(&input.attrs)?;
    let ident = &input.ident;
    let mut generics = input.generics.clone();
    let params: Vec<syn::Ident> = generics.type_params().map(|p| p.ident.clone()).collect();

    // Where the conversion comes from: the type's own `FromStr`/`Display`, or
    // the one value it wraps. The first asks nothing of the type's shape, which
    // is why it is the only one an enum or a several-field struct can use.
    let Conversion {
        implied,
        parse,
        render,
        inherited,
    } = match attrs.from_str {
        true => {
            let (_, ty_generics, _) = generics.split_for_impl();
            let whole: syn::Type = syn::parse_quote!(#ident #ty_generics);
            generics
                .make_where_clause()
                .predicates
                .push(syn::parse_quote!(#whole: ::core::str::FromStr + ::core::fmt::Display));
            Conversion::from_str()
        }
        false => {
            let (member, inner) = wrapped(&input)?;
            // As in `WebForm`: a bound only where a parameter is what has to
            // satisfy it, so a concrete inner type reports its own missing impl
            // at the field that names it.
            if mentions(inner, &params) {
                generics
                    .make_where_clause()
                    .predicates
                    .push(syn::parse_quote!(#inner: ::web_form::FormValue));
            }
            Conversion::wrapping(&member, inner)
        }
    };

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Everything `#[value(...)]` said, placed into the control the conversion
    // implies — the same merge, and the same rejections, a field goes through.
    let control = control_tokens(&attrs.constraints, &[], implied, false)?;
    let written = opt_str(&attrs.default);

    // Left off entirely when nothing was declared, so the type inherits the
    // trait's own empty check rather than a call that does nothing.
    let validate = match &attrs.validate {
        Some(path) => quote! {
            fn validate_form_value(&self) -> ::core::result::Result<(), ::web_form::FieldError> {
                ::web_form::__private::check_value(self, #path)
            }
        },
        None => TokenStream::new(),
    };

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::web_form::FormValue for #ident #ty_generics #where_clause {
            const CONTROL: ::web_form::Control = #control;

            const DEFAULT: ::core::option::Option<&'static str> =
                ::web_form::__private::or_default(#written, #inherited);

            fn parse_form_value(
                __raw: &str,
            ) -> ::core::result::Result<Self, ::web_form::ValueError> {
                #parse
            }

            fn to_form_value(&self) -> ::std::borrow::Cow<'_, str> {
                #render
            }

            #validate
        }
    })
}

/// The four things the choice of conversion decides.
struct Conversion {
    /// The control the conversion implies, before `#[value(...)]` is applied.
    implied: TokenStream,
    /// The body of `parse_form_value`.
    parse: TokenStream,
    /// The body of `to_form_value`.
    render: TokenStream,
    /// The default this type has before `#[value(default = ...)]` — the wrapped
    /// type's, where there is one to inherit.
    inherited: TokenStream,
}

impl Conversion {
    /// `#[value(from_str)]`: the type converts itself.
    fn from_str() -> Self {
        Self {
            // A type converting itself says nothing about what it looks like,
            // so it renders as text until `type = "..."` says otherwise.
            implied: quote!(::web_form::Control::TEXT),
            // The message is the adapter's, and for the same reason: what a
            // `FromStr` says went wrong is written for whoever wrote the call.
            parse: quote! {
                match ::core::primitive::str::trim(__raw).parse::<Self>() {
                    ::core::result::Result::Ok(__value) => {
                        ::core::result::Result::Ok(__value)
                    }
                    ::core::result::Result::Err(_) => ::core::result::Result::Err(
                        ::web_form::ValueError::new("a valid value"),
                    ),
                }
            },
            render: quote!(::std::borrow::Cow::Owned(
                ::std::string::ToString::to_string(self)
            )),
            inherited: quote!(::core::option::Option::None),
        }
    }

    /// The default: the one value the type wraps does the converting, and this
    /// writes it through the wrapper.
    fn wrapping(member: &Member, inner: &syn::Type) -> Self {
        let parsed = quote!(<#inner as ::web_form::FormValue>::parse_form_value(__raw)?);
        let (construct, member) = match member {
            Member::Index => (quote!(Self(#parsed)), quote!(0)),
            Member::Named(name) => (quote!(Self { #name: #parsed }), quote!(#name)),
        };
        Self {
            implied: quote!(<#inner as ::web_form::FormValue>::CONTROL),
            parse: quote!(::core::result::Result::Ok(#construct)),
            render: quote!(<#inner as ::web_form::FormValue>::to_form_value(&self.#member)),
            inherited: quote!(<#inner as ::web_form::FormValue>::DEFAULT),
        }
    }
}

/// How the one wrapped value is reached.
enum Member {
    /// A tuple struct: `self.0`.
    Index,
    Named(syn::Ident),
}

/// The single field a `FormValue` wrapper is, and how to reach it.
///
/// One field, because a form control submits one string: a type with two of
/// them has no way to say which one that string is, and a type with none has
/// nothing to convert. A struct that genuinely has several fields is either a
/// *form*, flattened into its parent with `#[field(flatten)]`, or a type that
/// converts itself — which is what `#[value(from_str)]` says, and why it is the
/// only spelling this function is not consulted for.
fn wrapped(input: &DeriveInput) -> Result<(Member, &Type)> {
    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(
            &input.ident,
            match &input.data {
                Data::Enum(_) => {
                    "`FormValue` can only be derived for a struct wrapping one value, unless \
                     `#[value(from_str)]` says the type converts itself; for a fieldless enum, \
                     whose variants are the options of a `<select>`, derive `FormChoice`"
                }
                _ => {
                    "`FormValue` can only be derived for a struct wrapping one value, unless \
                     `#[value(from_str)]` says the type converts itself"
                }
            },
        ));
    };

    let one = match &data.fields {
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            (Member::Index, &fields.unnamed[0].ty)
        }
        Fields::Named(fields) if fields.named.len() == 1 => {
            let field = &fields.named[0];
            (
                Member::Named(field.ident.clone().expect("named field")),
                &field.ty,
            )
        }
        fields => {
            return Err(Error::new_spanned(
                fields,
                "`FormValue` describes one form control, which submits one string, so it can \
                 only be derived for a struct with exactly one field; a struct with several is \
                 either a form of its own — derive `WebForm` and splice it in with \
                 `#[field(flatten)]` — or a type that converts itself, which is \
                 `#[value(from_str)]`",
            ));
        }
    };
    Ok(one)
}
