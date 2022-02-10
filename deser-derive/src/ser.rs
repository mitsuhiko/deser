use proc_macro2::{Span, TokenStream};
use quote::quote;

use crate::attr::{ContainerAttrs, EnumVariantAttrs, FieldAttrs};
use crate::bound::{where_clause_with_bound, with_lifetime_bound};

pub fn derive_serialize(input: &mut syn::DeriveInput) -> syn::Result<TokenStream> {
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
        &format!("_DESER_SERIALIZE_IMPL_FOR_{}", ident),
        Span::call_site(),
    );

    let container_attrs = ContainerAttrs::of(input)?;
    let type_name = container_attrs.container_name();
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
    let skip_if = attrs
        .iter()
        .zip(fieldname.iter())
        .map(|(attrs, name)| {
            let field_skip = if let Some(path) = attrs.skip_serializing_if() {
                quote! {
                    if #path(&self.data.#name) {
                        continue;
                    }
                }
            } else {
                quote! {}
            };
            let optional_skip = if container_attrs.skip_serializing_optionals() {
                quote! {
                    if __handle.is_optional() {
                        continue;
                    }
                }
            } else {
                quote! {}
            };
            quote! {
                #field_skip
                #optional_skip
            }
        })
        .collect::<Vec<_>>();
    let nested_emitters = attrs
        .iter()
        .filter_map(|attrs| {
            if attrs.flatten() {
                Some(syn::Ident::new(
                    &format!("emitter_{}", attrs.field().ident.as_ref().unwrap()),
                    Span::call_site(),
                ))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let nested_emitters_fields = attrs
        .iter()
        .filter_map(|attrs| {
            if attrs.flatten() {
                Some(attrs.field().ident.as_ref().unwrap())
            } else {
                None
            }
        })
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
                fn serialize(&self, __state: &::deser::ser::SerializerState) -> ::deser::__derive::Result<::deser::ser::Chunk> {
                    ::deser::__derive::Ok(::deser::ser::Chunk::Struct(Box::new(__StructEmitter {
                        data: self,
                        index: 0,
                        #(
                            #nested_emitters: match self.#nested_emitters_fields.serialize(__state)? {
                                ::deser::ser::Chunk::Struct(emitter) => emitter,
                                _ => return ::deser::__derive::Err(::deser::Error::new(
                                    ::deser::ErrorKind::Unexpected,
                                    "cannot flatten non-struct types"
                                ))
                            },
                        )*
                    })))
                }
            }

            struct __StructEmitter #wrapper_impl_generics #where_clause {
                data: &'__a #ident #ty_generics,
                index: usize,
                #(
                    #nested_emitters: Box<dyn ::deser::ser::StructEmitter + '__a>,
                )*
            }

            struct __Descriptor;

            impl ::deser::Descriptor for __Descriptor {
                fn name(&self) -> ::deser::__derive::Option<&::deser::__derive::str> {
                    ::deser::__derive::Some(#type_name)
                }
            }

            impl #wrapper_impl_generics ::deser::ser::StructEmitter for __StructEmitter #wrapper_ty_generics #bounded_where_clause {
                fn next(&mut self) -> ::deser::__derive::Option<(deser::__derive::StrCow, ::deser::ser::SerializeHandle)> {
                    loop {
                        let __index = self.index;
                        match __index {
                            #(
                                #index => {
                                    self.index = __index + 1;
                                    let __handle = ::deser::ser::SerializeHandle::to(&self.data.#fieldname);
                                    #skip_if
                                    return::deser::__derive::Some((
                                        ::deser::__derive::Cow::Borrowed(#fieldstr),
                                        __handle,
                                    ))
                                }
                            )*
                            _ => return ::deser::__derive::None,
                        }
                    }
                }
            }
        };
    })
}

fn derive_enum(input: &syn::DeriveInput, enumeration: &syn::DataEnum) -> syn::Result<TokenStream> {
    if input.generics.lt_token.is_some() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            Span::call_site(),
            "Only basic enums are supported (no generics)",
        ));
    }

    let ident = &input.ident;
    let dummy = syn::Ident::new(
        &format!("_DESER_SERIALIZE_IMPL_FOR_{}", ident),
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
            impl ::deser::Serialize for #ident {
                fn serialize(&self, __state: &::deser::ser::SerializerState)
                    -> ::deser::__derive::Result<::deser::ser::Chunk>
                {
                    ::deser::__derive::Ok(match *self {
                        #(
                            #ident::#var_idents => {
                                ::deser::ser::Chunk::Atom(::deser::Atom::Str(::deser::__derive::Cow::Borrowed(#names)))
                            }
                        )*
                    })
                }
            }
        };
    })
}
