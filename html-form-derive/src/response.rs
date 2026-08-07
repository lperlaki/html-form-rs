//! `#[form(renderer = ...)]`: the axum half of `#[derive(Form)]`.
//!
//! A form that names a renderer becomes an extractor and a response in its own
//! right. Four impls come out of it:
//!
//! * a unit struct, `{Name}Renderer`, and `Renderer<Self>` for it. That is
//!   where the named type or function is called. The attribute names a
//!   function as often as a type, and a function has no type anybody can write
//!   down, so the derive makes one that can be written down. It is zero-sized,
//!   as every renderer is, and it is what `Form<Self, {Name}Renderer>` and the
//!   rejection are named under.
//! * `HasRenderer for Self`, which names that renderer and is what a bound asks
//!   for when it means "a form that knows how it is rendered".
//! * `FromRequest<S> for Self`, which is that `Form` with the wrapper taken
//!   off, so the handler names the struct and nothing else.
//! * `IntoResponse for Self`, which is the form filled in from the value.
//!
//! The last two can be left out with `from_request = false` and
//! `into_response = false`. The first two are what the attribute *is*, so they
//! are always emitted.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Error, Generics, Ident, Result, Type, Visibility, WherePredicate};

use crate::attrs::FormAttrs;

pub fn response_tokens(
    form: &FormAttrs,
    ident: &Ident,
    vis: &Visibility,
    generics: &Generics,
    context: &Type,
) -> Result<TokenStream> {
    let Some((renderer, renderer_span)) = &form.renderer else {
        // Nothing to generate, and three keys that then describe nothing. Say
        // so where they were written rather than ignoring them.
        for (key, span) in [
            ("status", form.status.as_ref().map(|(_, span)| *span)),
            ("from_request", form.from_request.map(|(_, span)| span)),
            ("into_response", form.into_response.map(|(_, span)| span)),
        ] {
            if let Some(span) = span {
                return Err(Error::new(
                    span,
                    format!(
                        "`{key}` describes what `renderer = ...` generates, and this form names \
                         no renderer"
                    ),
                ));
            }
        }
        return Ok(quote!());
    };

    // A proc macro cannot read the features of the crate that invoked it. The
    // one it can read is its own, which `html-form`'s `axum` feature turns on.
    if !cfg!(feature = "axum") {
        return Err(Error::new(
            *renderer_span,
            "`renderer` generates axum impls; turn on the `axum` feature of `html-form`",
        ));
    }

    let wants_request = form.from_request.is_none_or(|(want, _)| want);

    // A form renders itself out of the value alone, so it can only do so with a
    // context the caller need not supply. That is what `EmptyContext` is, and a
    // concrete context that does not implement it makes the impl one the
    // compiler rejects outright rather than one nobody can call. So the default
    // follows the context, and `into_response` says otherwise where a context
    // of your own is an `EmptyContext` after all.
    let params: Vec<Ident> = generics.type_params().map(|p| p.ident.clone()).collect();
    let renders_itself = is_unit(context) || crate::form::mentions(context, &params);
    let wants_response = form.into_response.map_or(renders_itself, |(want, _)| want);

    if let Some((_, span)) = &form.status
        && !wants_response
    {
        return Err(Error::new(
            *span,
            match form.into_response {
                Some((false, _)) => "`status` is the status of the response this form makes, and \
                                     `into_response = false` leaves that impl out"
                    .to_owned(),
                _ => format!(
                    "`status` is the status of the response this form makes, and a form whose \
                     context is `{}` makes none; add `into_response` if that context is an \
                     `html_form::EmptyContext`",
                    quote!(#context),
                ),
            },
        ));
    }

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // The renderer this form is named under. It takes the form's own
    // visibility, because it is part of the rejection type the extractor
    // declares, and a private type there would not be nameable where the
    // extractor is.
    let marker = format_ident!("{ident}Renderer");
    let marker_doc = format!(
        "The [`Renderer`](html_form::axum::Renderer) that [`{ident}`] declared, and what its \
         rejection is written under.\n\n\
         `#[derive(html_form::Form)]` generated it, because a function named in \
         `#[form(renderer = ...)]` has no type anybody can write down. It is zero-sized and \
         never built: a renderer's `render` is an associated function.",
    );

    // One marker serves every instantiation of a generic form, so it declares
    // no parameters of its own. `render_with` is one call whichever of the
    // three shapes the attribute named: the marker `M` is inferred, and that is
    // what lets one key take a type, a function or a closure.
    let renderer_impl = quote! {
        #[doc = #marker_doc]
        #[automatically_derived]
        #[derive(::core::fmt::Debug, ::core::clone::Clone, ::core::marker::Copy)]
        #vis struct #marker;

        // A `const` item, so this is evaluated whether or not anything ever
        // renders the form. Inside `render` it would only be checked once
        // something instantiated it.
        const _: () = ::html_form::axum::__private::assert_zst(&#renderer);

        #[automatically_derived]
        impl #impl_generics ::html_form::axum::Renderer<#ident #ty_generics>
            for #marker #where_clause
        {
            fn render(
                __view: ::html_form::FormView,
                __context: &#context,
            ) -> impl ::html_form::axum::__private::IntoResponse {
                // Bound rather than written into the call, so a closure written
                // in the attribute needs no parentheses around it.
                let __renderer = #renderer;
                ::html_form::axum::__private::render_with::<#ident #ty_generics, _, _>(
                    __renderer,
                    __view,
                    __context,
                )
            }
        }

        #[automatically_derived]
        impl #impl_generics ::html_form::axum::HasRenderer for #ident #ty_generics #where_clause {
            type Renderer = #marker;
        }
    };

    let request_impl = wants_request.then(|| {
        // The context is read out of the request head, so it has to be an
        // extractor of its own. A form that declares none asks for `()`, which
        // axum extracts from any request and any state.
        let generics = extended(
            generics,
            true,
            [
                syn::parse_quote!(__S: ::core::marker::Send + ::core::marker::Sync),
                syn::parse_quote!(#ident #ty_generics: ::core::marker::Send),
                syn::parse_quote!(
                    #context: ::html_form::axum::__private::FromRequestParts<__S>
                        + ::core::marker::Send
                ),
            ],
        );
        let (impl_generics, _, where_clause) = generics.split_for_impl();

        quote! {
            #[automatically_derived]
            impl #impl_generics ::html_form::axum::__private::FromRequest<__S>
                for #ident #ty_generics #where_clause
            {
                // The rejection is the one `Form<T, R>` already has, renderer
                // and context rejection included, so a layer still reads what
                // went wrong as itself.
                type Rejection = ::html_form::axum::Rejection<
                    Self,
                    #marker,
                    <#context as ::html_form::axum::__private::FromRequestParts<__S>>::Rejection,
                >;

                async fn from_request(
                    __req: ::html_form::axum::__private::Request,
                    __state: &__S,
                ) -> ::core::result::Result<Self, Self::Rejection> {
                    let __form = <::html_form::axum::Form<Self, #marker> as
                        ::html_form::axum::__private::FromRequest<__S>>::from_request(
                            __req, __state,
                        )
                        .await?;
                    ::core::result::Result::Ok(::html_form::axum::Form::into_inner(__form))
                }
            }
        }
    });

    let response_impl = wants_response.then(|| {
        // The value fills the form in, and there is no context to render it
        // with beyond the one a caller need not supply.
        let generics = extended(
            generics,
            false,
            [syn::parse_quote!(#context: ::html_form::EmptyContext)],
        );
        let (impl_generics, _, where_clause) = generics.split_for_impl();

        let status = match &form.status {
            Some((status, _)) => status.tokens(),
            None => quote!(::html_form::axum::__private::StatusCode::OK),
        };

        quote! {
            #[automatically_derived]
            impl #impl_generics ::html_form::axum::__private::IntoResponse
                for #ident #ty_generics #where_clause
            {
                fn into_response(self) -> ::html_form::axum::__private::Response {
                    // A `const`, so a status that is not one is a compile
                    // error and not a panic on the first request.
                    const __STATUS: ::html_form::axum::__private::StatusCode = #status;

                    // The same page `HasRenderer` makes, under the status this
                    // form declared. A renderer sets no status of its own.
                    ::html_form::axum::__private::IntoResponse::into_response((
                        __STATUS,
                        ::html_form::axum::HasRenderer::render_form(&self),
                    ))
                }
            }
        }
    });

    Ok(quote! {
        #renderer_impl
        #request_impl
        #response_impl
    })
}

/// Whether a type is `()`, which is the context of a form that declares none.
fn is_unit(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

/// The form's own generics, with the bounds one impl adds, and `__S` where that
/// impl is the extractor. The state parameter goes last, after any the struct
/// declared itself.
fn extended(
    generics: &Generics,
    state: bool,
    extra: impl IntoIterator<Item = WherePredicate>,
) -> Generics {
    let mut generics = generics.clone();
    if state {
        let param = Ident::new("__S", Span::call_site());
        generics.params.push(syn::parse_quote!(#param));
    }
    generics.make_where_clause().predicates.extend(extra);
    generics
}
