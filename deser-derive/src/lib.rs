extern crate proc_macro;

mod attr;
mod bound;
mod de;
mod ser;

use proc_macro::TokenStream;
use syn::parse_macro_input;

#[proc_macro_derive(Serialize, attributes(deser))]
pub fn derive_serialize(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as syn::DeriveInput);
    ser::derive_serialize(&mut input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

#[proc_macro_derive(Deserialize, attributes(deser))]
pub fn derive_deserialize(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as syn::DeriveInput);
    de::derive_deserialize(&mut input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}
