extern crate proc_macro;

mod attr;
mod bound;
mod de;
mod enums;
mod ser;

use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn::parse_macro_input;
use syn::spanned::Spanned;

#[proc_macro_derive(Serialize, attributes(deser))]
pub fn derive_serialize(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as syn::DeriveInput);
    expand(&mut input, ser::derive_serialize)
}

#[proc_macro_derive(Deserialize, attributes(deser))]
pub fn derive_deserialize(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as syn::DeriveInput);
    expand(&mut input, de::derive_deserialize)
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
        let import = match attr::ContainerAttrs::of(input)?.crate_path() {
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
