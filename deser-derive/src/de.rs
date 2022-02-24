use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::quote;

use crate::attr::{
    ensure_no_field_attrs, ContainerAttrs, EnumVariantAttrs, FieldAttrs, TypeDefault,
};
use crate::bound::{where_clause_with_bound, with_lifetime_bound};

pub fn derive_deserialize(input: &mut syn::DeriveInput) -> syn::Result<TokenStream> {
    match &input.data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Named(fields),
            ..
        }) => derive_struct(input, fields),
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Unnamed(fields),
            ..
        }) if fields.unnamed.len() == 1 => derive_newtype_struct(input, &fields.unnamed[0]),
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
    let attrs = fields
        .named
        .iter()
        .map(FieldAttrs::of)
        .collect::<syn::Result<Vec<_>>>()?;
    let fieldname = attrs.iter().map(|x| &x.field().ident).collect::<Vec<_>>();
    let sink_fieldname = attrs
        .iter()
        .map(|x| {
            syn::Ident::new(
                &format!("field_{}", x.field().ident.as_ref().unwrap()),
                Span::call_site(),
            )
        })
        .collect::<Vec<_>>();
    let sink_fieldty = attrs
        .iter()
        .map(|f| {
            let ty = &f.field().ty;
            if f.flatten() {
                quote! {
                    ::deser::de::OwnedSink<#ty>
                }
            } else {
                quote! {
                    ::deser::__derive::Option<#ty>
                }
            }
        })
        .collect::<Vec<_>>();
    let sink_defaults = attrs
        .iter()
        .map(|f| {
            if f.flatten() {
                quote! {
                    ::deser::de::OwnedSink::deserialize()
                }
            } else if f.default().is_some() {
                quote! {
                    ::deser::__derive::None
                }
            } else {
                quote! {
                    ::deser::de::Deserialize::__private_initial_value()
                }
            }
        })
        .collect::<Vec<_>>();

    let mut seen_names = HashSet::new();
    let mut first_duplicate_name = None;
    let matcher = attrs
        .iter()
        .zip(sink_fieldname.iter())
        .filter_map(|(x, fieldname)| {
            if x.flatten() {
                return None;
            }

            let name = x.name(&container_attrs).to_string();
            if first_duplicate_name.is_none() && seen_names.contains(&name) {
                first_duplicate_name = Some((name.clone(), x.field()));
            }
            seen_names.insert(name.clone());

            let mut rv = quote! { #name };
            for alias in x.aliases() {
                let alias = alias.clone();
                if first_duplicate_name.is_none() && seen_names.contains(&alias) {
                    first_duplicate_name = Some((alias.clone(), x.field()));
                }
                seen_names.insert(alias.clone());
                rv = quote! { #rv | #alias };
            }
            Some(quote! {
                #rv => return ::deser::__derive::Ok(::deser::__derive::Some(::deser::Deserialize::deserialize_into(&mut self.#fieldname))),
            })
        })
        .collect::<Vec<_>>();

    if let Some((first_duplicate_name, field)) = first_duplicate_name {
        return Err(syn::Error::new_spanned(
            field,
            format!("field name '{}' used more than once", first_duplicate_name),
        ));
    }

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
            } else if attrs.flatten() {
                // this should never happen unless the inner deserializer fucked up
                let error = format!(
                    "Failed to deserialize flattened field '{}'",
                    attrs.name(&container_attrs)
                );
                quote! {
                    match #name {
                        ::deser::__derive::Some(val) => val,
                        ::deser::__derive::None => return ::deser::__derive::Err(::deser::Error::new(::deser::ErrorKind::Unexpected, #error))
                    }
                }
            } else if container_attrs.default().is_some() {
                quote! { #name.unwrap() }
            } else {
                let str_name = attrs.name(&container_attrs);
                quote! {
                    match #name {
                        ::deser::__derive::Some(val) => val,
                        ::deser::__derive::None => return ::deser::__derive::Err(::deser::__derive::new_missing_field_error(#str_name))
                    }
                }
            }
        })
        .collect::<Vec<_>>();
    let flatten_fields = sink_fieldname
        .iter()
        .zip(attrs.iter())
        .filter_map(
            |(name, attrs)| {
                if attrs.flatten() {
                    Some(name)
                } else {
                    None
                }
            },
        )
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
            struct __Sink #wrapper_impl_generics #where_clause {
                slot: &'__a mut ::deser::__derive::Option<#ident #ty_generics>,
                key: ::deser::__derive::Option<String>,
                #(
                    #sink_fieldname: #sink_fieldty,
                )*
            }

            #[automatically_derived]
            impl #impl_generics ::deser::Deserialize for #ident #ty_generics #bounded_where_clause {
                fn deserialize_into(
                    __slot: &mut ::deser::__derive::Option<Self>,
                ) -> ::deser::de::SinkHandle {
                    ::deser::de::SinkHandle::boxed(__Sink {
                        slot: __slot,
                        key: ::deser::__derive::None,
                        #(
                            #sink_fieldname: #sink_defaults,
                        )*
                    })
                }
            }

            #[automatically_derived]
            impl #wrapper_impl_generics ::deser::de::Sink for __Sink #wrapper_ty_generics #bounded_where_clause {
                fn map(&mut self, __state: &::deser::de::DeserializerState)
                    -> ::deser::__derive::Result<()>
                {
                    ::deser::__derive::Ok(())
                }

                fn next_key(&mut self, __state: &::deser::de::DeserializerState)
                    -> ::deser::__derive::Result<::deser::de::SinkHandle>
                {
                    ::deser::__derive::Ok(::deser::de::Deserialize::deserialize_into(&mut self.key))
                }

                fn next_value(&mut self, __state: &::deser::de::DeserializerState)
                    -> ::deser::__derive::Result<::deser::de::SinkHandle>
                {
                    let __key = self.key.take().unwrap();
                    ::deser::__derive::Ok(match self.value_for_key(&__key, __state)? {
                        ::deser::__derive::Some(__sink) => __sink,
                        ::deser::__derive::None => ::deser::de::SinkHandle::null(),
                    })
                }

                fn value_for_key(&mut self, __key: &str, __state: &::deser::de::DeserializerState)
                    -> ::deser::__derive::Result<::deser::__derive::Option<::deser::de::SinkHandle>>
                {
                    match __key {
                        #(
                            #matcher
                        )*
                        __other => {
                            #(
                                if let ::deser::__derive::Some(__sink) = self.#flatten_fields.borrow_mut().value_for_key(__other, __state)? {
                                    return ::deser::__derive::Ok(::deser::__derive::Some(__sink));
                                }
                            )*
                        }
                    }
                    ::deser::__derive::Ok(::deser::__derive::None)
                }

                fn finish(&mut self, __state: &::deser::de::DeserializerState) -> ::deser::__derive::Result<()> {
                    #![allow(unused_mut)]
                    #(
                        self.#flatten_fields.borrow_mut().finish(__state)?;
                    )*
                    #(
                        let mut #sink_fieldname = self.#sink_fieldname.#field_stage1_default;
                    )*
                    #stage2_default
                    *self.slot = ::deser::__derive::Some(#ident {
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

    let mut seen_names = HashSet::new();
    let mut first_duplicate_name = None;
    let matcher = attrs
        .iter()
        .map(|x| {
            let name = x.name(&container_attrs).to_string();
            if first_duplicate_name.is_none() && seen_names.contains(&name) {
                first_duplicate_name = Some((name.clone(), x.variant()));
            }
            seen_names.insert(name.clone());

            let mut rv = quote! {
                #name
            };
            for alias in x.aliases() {
                let alias = alias.clone();
                if first_duplicate_name.is_none() && seen_names.contains(&alias) {
                    first_duplicate_name = Some((alias.clone(), x.variant()));
                }
                seen_names.insert(alias.clone());
                rv = quote! {
                    #rv | #alias
                };
            }
            rv
        })
        .collect::<Vec<_>>();
    if let Some((first_duplicate_name, field)) = first_duplicate_name {
        return Err(syn::Error::new_spanned(
            field,
            format!(
                "variant name '{}' used more than once",
                first_duplicate_name
            ),
        ));
    }

    Ok(quote! {
        #[allow(non_upper_case_globals)]
        const #dummy: () = {
            #[repr(transparent)]
            struct __SlotWrapper {
                slot: ::deser::__derive::Option<#ident>,
            }

            #[automatically_derived]
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
                        __other => return self.unexpected_atom(__other, __state),
                    };
                    let value = match s {
                        #( #matcher => #ident::#var_idents, )*
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

fn derive_newtype_struct(input: &syn::DeriveInput, field: &syn::Field) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let dummy = syn::Ident::new(
        &format!("_DESER_DESERIALIZE_IMPL_FOR_{}", ident),
        Span::call_site(),
    );

    let container_attrs = ContainerAttrs::of(input)?;
    let _type_name = container_attrs.container_name();

    ensure_no_field_attrs(field)?;

    let field_type = &field.ty;

    let wrapper_generics = with_lifetime_bound(&input.generics, "'__a");
    let (wrapper_impl_generics, wrapper_ty_generics, _) = wrapper_generics.split_for_impl();
    let bound = syn::parse_quote!(::deser::Serialize);
    let bounded_where_clause = where_clause_with_bound(&input.generics, bound);

    Ok(quote! {
        #[allow(non_upper_case_globals)]
        const #dummy: () = {
            struct __Sink #wrapper_impl_generics #where_clause {
                slot: &'__a mut ::deser::__derive::Option<#ident #ty_generics>,
                sink: ::deser::de::OwnedSink<#field_type #ty_generics>,
            }

            #[automatically_derived]
            impl #impl_generics ::deser::de::Deserialize for #ident #ty_generics #bounded_where_clause {
                fn deserialize_into(
                    __slot: &mut ::deser::__derive::Option<Self>
                ) -> ::deser::de::SinkHandle {
                    ::deser::de::SinkHandle::boxed(__Sink {
                        slot: __slot,
                        sink: ::deser::de::OwnedSink::deserialize(),
                    })
                }
            }

            impl #wrapper_impl_generics ::deser::de::Sink for __Sink #wrapper_ty_generics #bounded_where_clause {
                fn atom(&mut self, __atom: ::deser::Atom, __state: &::deser::de::DeserializerState)
                    -> ::deser::__derive::Result<()>
                {
                    self.sink.borrow_mut().atom(__atom, __state)
                }

                fn map(&mut self, __state: &::deser::de::DeserializerState) -> ::deser::__derive::Result<()> {
                    self.sink.borrow_mut().map(__state)
                }

                fn seq(&mut self, __state: &::deser::de::DeserializerState) -> ::deser::__derive::Result<()>  {
                    self.sink.borrow_mut().seq(__state)
                }

                fn next_key(&mut self, __state: &::deser::de::DeserializerState)
                    -> ::deser::__derive::Result<::deser::de::SinkHandle>
                {
                    self.sink.borrow_mut().next_key(__state)
                }

                fn next_value(&mut self, __state: &::deser::de::DeserializerState)
                    -> ::deser::__derive::Result<::deser::de::SinkHandle>
                {
                    self.sink.borrow_mut().next_value(__state)
                }

                fn value_for_key(
                    &mut self,
                    __key: &str,
                    __state: &::deser::de::DeserializerState,
                ) -> ::deser::__derive::Result<::deser::__derive::Option<::deser::de::SinkHandle>> {
                    self.sink.borrow_mut().value_for_key(__key, __state)
                }

                fn finish(&mut self, __state: &::deser::de::DeserializerState) -> ::deser::__derive::Result<()> {
                    self.sink.borrow_mut().finish(__state)?;
                    *self.slot = self.sink.take().map(#ident);
                    Ok(())
                }

                fn expecting(&self) -> ::deser::__derive::StrCow<'_> {
                    self.sink.borrow().expecting()
                }
            }
        };
    })
}
