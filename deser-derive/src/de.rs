use proc_macro2::{Span, TokenStream};
use quote::quote;

use crate::attr::{ContainerAttrs, EnumVariantAttrs, FieldAttrs, TypeDefault};
use crate::bound::{where_clause_with_bound, with_lifetime_bound};

pub fn derive_deserialize(input: &mut syn::DeriveInput) -> syn::Result<TokenStream> {
    match &input.data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Named(fields),
            ..
        }) => derive_struct(input, fields),
        syn::Data::Enum(enumeration) => derive_enum(input, enumeration),
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

    let wrapper_generics = with_lifetime_bound(&input.generics, "'__a");
    let (wrapper_impl_generics, wrapper_ty_generics, _) = wrapper_generics.split_for_impl();
    let bound = syn::parse_quote!(::deser::Serialize);
    let bounded_where_clause = where_clause_with_bound(&input.generics, bound);

    let field_stage1_default = attrs
        .iter()
        .map(|attrs| match attrs.default() {
            Some(TypeDefault::Implicit) => {
                quote! { take().unwrap_or_else(::deser::__derive::Default::default) }
            }
            Some(TypeDefault::Explicit(path)) => {
                quote! { take().unwrap_or_else(#path) }
            }
            None => quote!(take()),
        })
        .collect::<Vec<_>>();
    let field_take = sink_fieldname
        .iter()
        .zip(attrs.iter())
        .map(|(name, attrs)| {
            if attrs.default().is_some() {
                quote! { #name }
            } else if container_attrs.default().is_some() {
                quote! { #name.unwrap() }
            } else {
                let error = format!("Missing field '{}'", attrs.name(&container_attrs));
                quote! {
                    #name.ok_or_else(|| {
                        ::deser::Error::new(::deser::ErrorKind::MissingField, #error)
                    })?
                }
            }
        })
        .collect::<Vec<_>>();

    let stage2_default = if container_attrs.default().is_some() {
        let need_container_default = sink_fieldname
            .iter()
            .zip(fieldname.iter())
            .zip(attrs.iter())
            .filter_map(|((sink_name, original_name), attrs)| {
                if attrs.default().is_none() {
                    Some((sink_name, *original_name))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if !need_container_default.is_empty() {
            let (sink_name, original_name): (Vec<_>, Vec<_>) =
                need_container_default.into_iter().unzip();
            let type_default = match container_attrs.default().unwrap() {
                TypeDefault::Implicit => quote! {
                    <#ident as ::deser::__derive::Default>::default()
                },
                TypeDefault::Explicit(path) => quote! { #path() },
            };
            Some(quote! {
                if [
                    #(
                        #sink_name.as_ref().is_none()
                    ),*
                ].iter().any(|x| *x) {
                    let __default = #type_default;
                    #(
                        #sink_name = #sink_name.or_else(|| Some(__default.#original_name));
                    )*
                }
            })
        } else {
            None
        }
    } else {
        None
    };

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
                        key: ::deser::__derive::None,
                        #(
                            #sink_fieldname: ::deser::__derive::None,
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
                        _ => ::deser::__derive::Ok(::deser::de::SinkHandle::null()),
                    }
                }

                fn finish(&mut self, __state: &::deser::de::DeserializerState) -> ::deser::__derive::Result<()> {
                    #![allow(unused_mut)]
                    #(
                        let mut #sink_fieldname = self.#sink_fieldname.#field_stage1_default;
                    )*
                    #stage2_default
                    *self.out = ::deser::__derive::Some(#ident {
                        #(
                            #fieldname: #field_take,
                        )*
                    });
                    ::deser::__derive::Ok(())
                }
            }
        };
    })
}

pub fn derive_enum(
    input: &syn::DeriveInput,
    enumeration: &syn::DataEnum,
) -> syn::Result<TokenStream> {
    if input.generics.lt_token.is_some() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            Span::call_site(),
            "Only basic enums are supported (no generics)",
        ));
    }

    let ident = &input.ident;
    let dummy = syn::Ident::new(
        &format!("_DESER_DESERIALIZE_IMPL_FOR_{}", ident),
        Span::call_site(),
    );

    let container_attrs = ContainerAttrs::of(input)?;
    let var_idents = enumeration
        .variants
        .iter()
        .map(|variant| match variant.fields {
            syn::Fields::Unit => Ok(&variant.ident),
            _ => Err(syn::Error::new_spanned(
                variant,
                "Invalid variant: only simple enum variants without fields are supported",
            )),
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let attrs = enumeration
        .variants
        .iter()
        .map(EnumVariantAttrs::of)
        .collect::<syn::Result<Vec<_>>>()?;
    let names = attrs
        .iter()
        .map(|x| x.name(&container_attrs))
        .collect::<Vec<_>>();

    Ok(quote! {
        #[allow(non_upper_case_globals)]
        const #dummy: () = {
            #[repr(transparent)]
            struct __SlotWrapper {
                slot: ::deser::__derive::Option<#ident>,
            }

            impl ::deser::de::Deserialize for #ident {
                fn deserialize_into(
                    __slot: &mut ::deser::__derive::Option<Self>
                ) -> ::deser::de::SinkHandle {
                    ::deser::de::SinkHandle::to(unsafe {
                        &mut *{
                            __slot
                            as *mut ::deser::__derive::Option<Self>
                            as *mut __SlotWrapper
                        }
                    })
                }
            }

            impl ::deser::de::Sink for __SlotWrapper {
                fn atom(
                    &mut self,
                    __atom: ::deser::Atom,
                    __state: &::deser::de::DeserializerState
                ) -> ::deser::__derive::Result<()> {
                    let s = match __atom {
                        ::deser::Atom::Str(ref s) => &s as &::deser::__derive::str,
                        __other => return Err(__other.unexpected_error(&self.expecting()))
                    };
                    let value = match s {
                        #( #names => #ident::#var_idents, )*
                        _ => return ::deser::__derive::Err(
                            ::deser::Error::new(::deser::ErrorKind::Unexpected, "unexpected value for enum")
                        )
                    };
                    self.slot = ::deser::__derive::Some(value);
                    ::deser::__derive::Ok(())
                }
            }
        };
    })
}
