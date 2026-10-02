//! The derive macros of [deser](https://docs.rs/deser).
//!
//! This crate is an implementation detail of deser, use the derive macros
//! through the [`deser`](https://docs.rs/deser) crate (with the `derive`
//! feature) instead.
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]

extern crate proc_macro;

mod attr;
mod bound;
mod de;
mod enums;
mod forward;
#[cfg(feature = "open-enums")]
mod open_enum;
mod ser;
mod transparent;
mod unnamed;

use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn::parse_macro_input;
use syn::spanned::Spanned;

/// Derives [`Serialize`](ser/trait.Serialize.html) for a struct or enum.
///
/// The attributes that customize the derive are described in the
/// [`derive`](derive/index.html) module.
// the links are relative to the root of deser which inlines the macro
#[proc_macro_derive(Serialize, attributes(deser))]
pub fn derive_serialize(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as syn::DeriveInput);
    expand(&mut input, ser::derive_serialize)
}

/// Derives [`Deserialize`](de/trait.Deserialize.html) for a struct or enum.
///
/// The attributes that customize the derive are described in the
/// [`derive`](derive/index.html) module.
// the links are relative to the root of deser which inlines the macro
#[proc_macro_derive(Deserialize, attributes(deser))]
pub fn derive_deserialize(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as syn::DeriveInput);
    expand(&mut input, de::derive_deserialize)
}

/// Makes a trait an open enum.
///
/// The trait objects (`Box<dyn Trait>` and `Arc<dyn Trait>`) are serialized
/// and deserialized like enums whose variants are the implementations of
/// the trait marked with [`variant`](macro@variant).  See
/// [open enums](derive/index.html#open-enums).
// the links are relative to the root of deser which inlines the macro
#[cfg(feature = "open-enums")]
#[proc_macro_attribute]
pub fn open_enum(args: TokenStream, input: TokenStream) -> TokenStream {
    open_enum::expand_open_enum(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Makes an implementation of an open enum a variant.
///
/// Every implementation of a trait marked with
/// [`open_enum`](macro@open_enum) needs this.  See
/// [open enums](derive/index.html#open-enums).
// the links are relative to the root of deser which inlines the macro
#[cfg(feature = "open-enums")]
#[proc_macro_attribute]
pub fn variant(args: TokenStream, input: TokenStream) -> TokenStream {
    open_enum::expand_variant(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Returns the error for unions without a container adapter.
fn unsupported_union(input: &syn::DeriveInput) -> syn::Error {
    syn::Error::new_spanned(
        &input.ident,
        "unions can only be derived with a container adapter (`#[deser(as = ...)]`)",
    )
}

/// Invokes the derive and makes the deser crate available as `__deser`.
///
/// All generated code refers to deser through `__deser` so that the path to
/// the crate can be changed with `#[deser(crate = path)]`.
fn expand(
    input: &mut syn::DeriveInput,
    derive: fn(&mut syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream>,
) -> TokenStream {
    let rv = (|| -> syn::Result<proc_macro2::TokenStream> {
        let import = match attr::ContainerAttrs::of(input, attr::Direction::Serialize)?.crate_path()
        {
            // spanned so that errors point to the path
            Some(path) => quote_spanned! { path.span()=> use #path as __deser; },
            None => quote! {
                #[allow(unused_extern_crates, clippy::useless_attribute)]
                extern crate deser as __deser;
            },
        };
        let output = derive(input)?;
        Ok(quote! {
            #[doc(hidden)]
            const _: () = {
                #import
                #output
            };
        })
    })();
    rv.unwrap_or_else(|err| err.to_compile_error()).into()
}
