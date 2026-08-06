//! `#[derive(FormValue)]`. A type carries its own control, its own default and
//! its own check, so a form that uses it says only where it goes.
//!
//! Something already knows how to turn the type into a string and back. That is
//! the one value it wraps, or, with `#[value(from_str)]`, the type itself
//! through `FromStr` and `Display`. This derive writes that conversion out and
//! builds the rest on top of it.

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

    // Where the conversion comes from: the type's own `FromStr` and `Display`,
    // or the one value it wraps. The first asks nothing of the type's shape, so
    // it is the only one an enum or a several-field struct can use.
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
            // As in `Form`: write a bound only where a parameter has to satisfy
            // it. A concrete inner type then reports its own missing impl at
            // the field that names it.
            if mentions(inner, &params) {
                generics
                    .make_where_clause()
                    .predicates
                    .push(syn::parse_quote!(#inner: ::html_form::FormValue));
            }
            Conversion::wrapping(&member, inner)
        }
    };

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Everything `#[value(...)]` said goes into the control the conversion
    // implies. This is the same merge, and the same rejections, that a field
    // goes through.
    let control = control_tokens(&attrs.constraints, &[], implied, false)?;
    let written = opt_str(&attrs.default);

    // Left out where nothing was declared, so the type takes the trait's own
    // empty check in place of a call that does nothing.
    let validate = match &attrs.validate {
        Some(path) => quote! {
            fn validate_form_value(&self) -> ::core::result::Result<(), ::html_form::FieldError> {
                ::html_form::__private::check_value(self, #path)
            }
        },
        None => TokenStream::new(),
    };

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::html_form::FormValue for #ident #ty_generics #where_clause {
            const CONTROL: ::html_form::Control = #control;

            const DEFAULT: ::core::option::Option<&'static str> =
                ::html_form::__private::or_default(#written, #inherited);

            fn parse_form_value(
                __raw: &str,
            ) -> ::core::result::Result<Self, ::html_form::ValueError> {
                #parse
            }

            fn to_form_value(&self) -> ::std::borrow::Cow<'_, str> {
                #render
            }

            #validate
        }
    })
}

/// The four things the choice of conversion settles.
struct Conversion {
    /// The control the conversion implies, before `#[value(...)]` applies.
    implied: TokenStream,
    /// The body of `parse_form_value`.
    parse: TokenStream,
    /// The body of `to_form_value`.
    render: TokenStream,
    /// The default this type has before `#[value(default = ...)]`. It is the
    /// wrapped type's default, where there is one to take.
    inherited: TokenStream,
}

impl Conversion {
    /// `#[value(from_str)]`: the type converts itself.
    fn from_str() -> Self {
        Self {
            // A type that converts itself says nothing about how it looks, so
            // it renders as text until `type = "..."` says otherwise.
            implied: quote!(::html_form::Control::TEXT),
            // The message belongs to the adapter, for the same reason. What a
            // `FromStr` reports speaks to whoever wrote the call.
            parse: quote! {
                match ::core::primitive::str::trim(__raw).parse::<Self>() {
                    ::core::result::Result::Ok(__value) => {
                        ::core::result::Result::Ok(__value)
                    }
                    ::core::result::Result::Err(_) => ::core::result::Result::Err(
                        ::html_form::ValueError::new("a valid value"),
                    ),
                }
            },
            render: quote!(::std::borrow::Cow::Owned(
                ::std::string::ToString::to_string(self)
            )),
            inherited: quote!(::core::option::Option::None),
        }
    }

    /// The default. The one value the type wraps does the conversion, and this
    /// writes it through the wrapper.
    fn wrapping(member: &Member, inner: &syn::Type) -> Self {
        let parsed = quote!(<#inner as ::html_form::FormValue>::parse_form_value(__raw)?);
        let (construct, member) = match member {
            Member::Index => (quote!(Self(#parsed)), quote!(0)),
            Member::Named(name) => (quote!(Self { #name: #parsed }), quote!(#name)),
        };
        Self {
            implied: quote!(<#inner as ::html_form::FormValue>::CONTROL),
            parse: quote!(::core::result::Result::Ok(#construct)),
            render: quote!(<#inner as ::html_form::FormValue>::to_form_value(&self.#member)),
            inherited: quote!(<#inner as ::html_form::FormValue>::DEFAULT),
        }
    }
}

/// How to reach the one wrapped value.
enum Member {
    /// A tuple struct: `self.0`.
    Index,
    Named(syn::Ident),
}

/// The single field that a `FormValue` wrapper holds, and how to reach it.
///
/// One field, because a form control submits one string. A type with two fields
/// cannot say which one that string is, and a type with none has nothing to
/// convert. A struct that really has several fields is one of two things. It is
/// a *form*, which goes into its parent with `#[field(flatten)]`. Or it is a
/// type that converts itself, which is what `#[value(from_str)]` says. That is
/// also why `#[value(from_str)]` is the one spelling that never calls this
/// function.
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
                 either a form of its own — derive `Form` and splice it in with \
                 `#[field(flatten)]` — or a type that converts itself, which is \
                 `#[value(from_str)]`",
            ));
        }
    };
    Ok(one)
}
