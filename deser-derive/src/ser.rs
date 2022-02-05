use proc_macro2::{Span, TokenStream};
use quote::quote;

use crate::attr::{ContainerAttrs, FieldAttrs};
use crate::bound::{where_clause_with_bound, with_lifetime_bound};

pub fn derive_serialize(input: &mut syn::DeriveInput) -> syn::Result<TokenStream> {
    match &input.data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Named(fields),
            ..
        }) => derive_struct(input, fields),
        _ => panic!("only struct swith named fields are supported"),
    }
}

fn derive_struct(input: &syn::DeriveInput, fields: &syn::FieldsNamed) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let type_name = ident.to_string();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let dummy = syn::Ident::new(
        &format!("_DESER_SERIALIZE_IMPL_FOR_{}", ident),
        Span::call_site(),
    );

    let container_attrs = ContainerAttrs::of(input)?;
    let fieldname = &fields.named.iter().map(|f| &f.ident).collect::<Vec<_>>();
    let attrs = fields
        .named
        .iter()
        .map(FieldAttrs::of)
        .collect::<syn::Result<Vec<_>>>()?;
    let fieldstr = attrs
        .iter()
        .map(|x| x.name(&container_attrs))
        .collect::<Vec<_>>();

    let index = 0usize..;

    let wrapper_generics = with_lifetime_bound(&input.generics, "'__a");
    let (wrapper_impl_generics, wrapper_ty_generics, _) = wrapper_generics.split_for_impl();
    let bound = syn::parse_quote!(::deser::Serialize);
    let bounded_where_clause = where_clause_with_bound(&input.generics, bound);

    Ok(quote! {
        #[allow(non_upper_case_globals)]
        const #dummy: () = {
            impl #impl_generics ::deser::Serialize for #ident #ty_generics #bounded_where_clause {
                fn descriptor(&self) -> &dyn ::deser::Descriptor {
                    &__Descriptor
                }
                fn serialize(&self, _state: &::deser::ser::SerializerState) -> ::deser::__derive::Result<::deser::ser::Chunk> {
                    ::deser::__derive::Ok(::deser::ser::Chunk::Struct(Box::new(__StructEmitter {
                        data: self,
                        index: 0,
                    })))
                }
            }

            struct __StructEmitter #wrapper_impl_generics #where_clause {
                data: &'__a #ident #ty_generics,
                index: usize,
            }

            struct __Descriptor;

            impl ::deser::Descriptor for __Descriptor {
                fn name(&self) -> ::deser::__derive::Option<&::deser::__derive::str> {
                    ::deser::__derive::Some(#type_name)
                }
            }

            impl #wrapper_impl_generics ::deser::ser::StructEmitter for __StructEmitter #wrapper_ty_generics #bounded_where_clause {
                fn next(&mut self) -> ::deser::__derive::Option<(deser::__derive::StrCow, ::deser::ser::SerializeHandle)> {
                    let index = self.index;
                    self.index = index + 1;
                    match index {
                        #(
                            #index => ::deser::__derive::Some((
                                ::deser::__derive::Cow::Borrowed(#fieldstr),
                                ::deser::ser::SerializeHandle::to(&self.data.#fieldname),
                            )),
                        )*
                        _ => ::deser::__derive::None,
                    }
                }
            }
        };
    })
}
