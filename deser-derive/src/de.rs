use proc_macro2::{Span, TokenStream};
use quote::quote;

use crate::attr::{ContainerAttrs, FieldAttrs};
use crate::bound::{where_clause_with_bound, with_lifetime_bound};

pub fn derive_deserialize(input: &mut syn::DeriveInput) -> syn::Result<TokenStream> {
    match &input.data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Named(fields),
            ..
        }) => derive_struct(input, fields),
        _ => panic!("only structs with named fields are supported"),
    }
}

fn derive_struct(input: &syn::DeriveInput, fields: &syn::FieldsNamed) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let dummy = syn::Ident::new(
        &format!("_DESER_DESERIALIZE_IMPL_FOR_{}", ident),
        Span::call_site(),
    );

    let container_attrs = ContainerAttrs::of(input)?;
    let type_name = container_attrs.container_name();
    let fieldname = &fields.named.iter().map(|f| &f.ident).collect::<Vec<_>>();
    let fieldty = fields.named.iter().map(|f| &f.ty);
    let sink_fieldname = &fields
        .named
        .iter()
        .map(|f| {
            syn::Ident::new(
                &format!("field_{}", f.ident.as_ref().unwrap()),
                Span::call_site(),
            )
        })
        .collect::<Vec<_>>();
    let attrs = fields
        .named
        .iter()
        .map(FieldAttrs::of)
        .collect::<syn::Result<Vec<_>>>()?;
    let fieldstr = attrs
        .iter()
        .map(|x| x.name(&container_attrs))
        .collect::<Vec<_>>();
    let missing_field_error = attrs
        .iter()
        .map(|x| format!("Missing field '{}'", x.name(&container_attrs)))
        .collect::<Vec<_>>();

    let wrapper_generics = with_lifetime_bound(&input.generics, "'__a");
    let (wrapper_impl_generics, wrapper_ty_generics, _) = wrapper_generics.split_for_impl();
    let bound = syn::parse_quote!(::deser::Serialize);
    let bounded_where_clause = where_clause_with_bound(&input.generics, bound);

    Ok(quote! {
        #[allow(non_upper_case_globals)]
        const #dummy: () = {
            #[repr(transparent)]
            struct __SlotWrapper #impl_generics #where_clause {
                slot: ::deser::__derive::Option<#ident #ty_generics>,
            }

            impl #impl_generics ::deser::Deserialize for #ident #ty_generics #bounded_where_clause {
                fn deserialize_into(
                    __slot: &mut ::deser::__derive::Option<Self>,
                ) -> ::deser::de::SinkHandle {
                    ::deser::de::SinkHandle::to(unsafe {
                        &mut *{
                            __slot
                            as *mut ::deser::__derive::Option<Self>
                            as *mut __SlotWrapper #ty_generics
                        }
                    })
                }
            }

            impl ::deser::de::Sink for __SlotWrapper #ty_generics #bounded_where_clause {
                fn map(&mut self, __state: &::deser::de::DeserializerState)
                    -> ::deser::__derive::Result<::deser::__derive::Box<dyn ::deser::de::MapSink + '_>>
                {
                    ::deser::__derive::Ok(::deser::__derive::Box::new(__MapSink {
                        key: None,
                        #(
                            #sink_fieldname: None,
                        )*
                        out: &mut self.slot,
                    }))
                }
            }

            struct __Descriptor;

            impl ::deser::Descriptor for __Descriptor {
                fn name(&self) -> ::deser::__derive::Option<&::deser::__derive::str> {
                    ::deser::__derive::Some(#type_name)
                }
            }

            struct __MapSink #wrapper_impl_generics #where_clause {
                key: ::deser::__derive::Option<String>,
                #(
                    #sink_fieldname: ::deser::__derive::Option<#fieldty>,
                )*
                out: &'__a mut ::deser::__derive::Option<#ident #ty_generics>,
            }

            impl #wrapper_impl_generics ::deser::de::MapSink for __MapSink #wrapper_ty_generics #bounded_where_clause {
                fn descriptor(&self) -> &dyn ::deser::Descriptor {
                    &__Descriptor
                }

                fn key(&mut self) -> ::deser::__derive::Result<::deser::de::SinkHandle> {
                    ::deser::__derive::Ok(::deser::de::Deserialize::deserialize_into(&mut self.key))
                }

                fn value(&mut self) -> ::deser::__derive::Result<::deser::de::SinkHandle> {
                    match self.key.take().as_deref() {
                        #(
                            ::deser::__derive::Some(#fieldstr) => ::deser::__derive::Ok(::deser::Deserialize::deserialize_into(&mut self.#sink_fieldname)),
                        )*
                        _ => ::deser::__derive::Ok(::deser::de::SinkHandle::to(::deser::de::ignore())),
                    }
                }

                fn finish(&mut self, __state: &::deser::de::DeserializerState) -> ::deser::__derive::Result<()> {
                    *self.out = ::deser::__derive::Some(#ident {
                        #(
                            #fieldname: self.#sink_fieldname.take().ok_or_else(|| {
                                ::deser::Error::new(::deser::ErrorKind::MissingField, #missing_field_error)
                            })?,
                        )*
                    });
                    ::deser::__derive::Ok(())
                }
            }
        };
    })
}