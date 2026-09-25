use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

use crate::attr::{ContainerAttrs, EnumVariantAttrs, FieldAttrs, TypeDefault, UnnamedFieldAttrs};
use crate::bound::{where_clause_for_fields, with_de_lifetime, with_lifetime_bound, BoundField};

/// Returns an expression that creates a sink handle for a slot.
fn deserialize_into(ty: &syn::Type, adapter: Option<&syn::Type>, slot: TokenStream) -> TokenStream {
    match adapter {
        // spanned so that errors about unsupported types point to the adapter
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::adapters::DeserializeAs<'de, #ty>>::deserialize_into_as(#slot)
        },
        None => quote! { __deser::Deserialize::deserialize_into(#slot) },
    }
}

/// Returns an expression that deserializes an atom into a slot.
fn atom_into(ty: &syn::Type, adapter: Option<&syn::Type>, slot: TokenStream) -> TokenStream {
    match adapter {
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::adapters::DeserializeAs<'de, #ty>>::__private_atom_into_as(#slot, __atom, __state)
        },
        None => quote! { __deser::__derive::atom_into(#slot, __atom, __state) },
    }
}

/// Returns an expression that deserializes a borrowed atom into a slot.
fn borrowed_atom_into(
    ty: &syn::Type,
    adapter: Option<&syn::Type>,
    slot: TokenStream,
) -> TokenStream {
    match adapter {
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::adapters::DeserializeAs<'de, #ty>>::__private_borrowed_atom_into_as(#slot, __atom, __state)
        },
        None => quote! { __deser::__derive::borrowed_atom_into(#slot, __atom, __state) },
    }
}

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
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let de_generics = with_de_lifetime(&input.generics)?;
    let (impl_generics, _, _) = de_generics.split_for_impl();

    let container_attrs = ContainerAttrs::of(input)?;
    let type_name = container_attrs.container_name();
    let attrs = fields
        .named
        .iter()
        .map(FieldAttrs::of)
        .collect::<syn::Result<Vec<_>>>()?;
    if let Some(attrs) = attrs.iter().find(|x| x.tag()) {
        return Err(syn::Error::new_spanned(
            attrs.field(),
            "tag fields are only supported in other variants of enums",
        ));
    }
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
                    __deser::de::OwnedSink<'de, #ty>
                }
            } else {
                quote! {
                    __deser::__derive::Option<#ty>
                }
            }
        })
        .collect::<Vec<_>>();
    let sink_defaults = attrs
        .iter()
        .map(|f| {
            if f.flatten() {
                quote! {
                    __deser::de::OwnedSink::deserialize()
                }
            } else if f.default().is_some() {
                quote! {
                    __deser::__derive::None
                }
            } else if let Some(adapter) = f.adapter() {
                let ty = &f.field().ty;
                quote! {
                    <#adapter as __deser::adapters::DeserializeAs<'de, #ty>>::initial_value_as()
                }
            } else {
                quote! {
                    __deser::de::Deserialize::initial_value()
                }
            }
        })
        .collect::<Vec<_>>();

    let mut seen_names = HashSet::new();
    let mut first_duplicate_name = None;
    let mut key_matcher = Vec::new();
    let mut key_dispatch = Vec::new();
    let mut key_atom_dispatch = Vec::new();
    let mut key_borrowed_atom_dispatch = Vec::new();
    let matcher = attrs
        .iter()
        .zip(sink_fieldname.iter())
        .enumerate()
        .filter_map(|(index, (x, fieldname))| {
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
            let ty = &x.field().ty;
            let sink = deserialize_into(ty, x.adapter(), quote! { &mut self.#fieldname });
            let atom = atom_into(ty, x.adapter(), quote! { &mut self.#fieldname });
            let borrowed_atom =
                borrowed_atom_into(ty, x.adapter(), quote! { &mut self.#fieldname });
            key_matcher.push(quote! {
                #rv => __Key::Field(#index),
            });
            key_dispatch.push(quote! {
                __Key::Field(#index) => #sink,
            });
            key_atom_dispatch.push(quote! {
                __Key::Field(#index) => #atom,
            });
            key_borrowed_atom_dispatch.push(quote! {
                __Key::Field(#index) => #borrowed_atom,
            });
            Some(quote! {
                #rv => return __deser::__derive::Ok(__deser::__derive::Some(#sink)),
            })
        })
        .collect::<Vec<_>>();

    if let Some((first_duplicate_name, field)) = first_duplicate_name {
        return Err(syn::Error::new_spanned(
            field,
            format!("field name '{}' used more than once", first_duplicate_name),
        ));
    }

    let wrapper_generics = with_lifetime_bound(&de_generics, "'__a");
    let (wrapper_impl_generics, wrapper_ty_generics, _) = wrapper_generics.split_for_impl();
    let bounded_where_clause = where_clause_for_fields(
        &input.generics,
        quote!(__deser::Deserialize<'de>),
        None,
        quote!(__deser::adapters::DeserializeAs),
        Some(quote!('de)),
        container_attrs.deserialize_bound(),
        &attrs
            .iter()
            .map(|x| BoundField {
                ty: &x.field().ty,
                adapter: x.adapter(),
            })
            .collect::<Vec<_>>(),
    );

    let field_stage1_default = attrs
        .iter()
        .map(|attrs| match attrs.default() {
            Some(TypeDefault::Implicit) => {
                quote! { take().unwrap_or_else(__deser::__derive::Default::default) }
            }
            Some(TypeDefault::Explicit(expr)) => {
                quote! { take().unwrap_or_else(|| #expr) }
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
                        __deser::__derive::Some(val) => val,
                        __deser::__derive::None => return __deser::__derive::Err(__deser::Error::new(__deser::ErrorKind::Unexpected, #error))
                    }
                }
            } else if container_attrs.default().is_some() {
                quote! { #name.unwrap() }
            } else {
                let str_name = attrs.name(&container_attrs);
                quote! {
                    match #name {
                        __deser::__derive::Some(val) => val,
                        __deser::__derive::None => return __deser::__derive::Err(__deser::__derive::new_missing_field_error(#str_name))
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
                    <#ident as __deser::__derive::Default>::default()
                },
                TypeDefault::Explicit(expr) => expr.clone(),
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

    // Keys are resolved to a field index directly in the key sink so that
    // deserializing a struct does not need to allocate a string per key.  Only
    // if flattened fields exist are unknown keys retained so that they can be
    // looked up on the flattened sinks.
    let has_flatten = !flatten_fields.is_empty();
    let other_key_variant = if has_flatten {
        Some(quote! { Other(__deser::__derive::String), })
    } else {
        None
    };
    let other_key_match = if has_flatten {
        quote! { __Key::Other(__other.into_owned()) }
    } else {
        quote! { { let _ = __other; __Key::Unknown } }
    };
    let other_key_dispatch = if has_flatten {
        Some(quote! {
            __Key::Other(__key) => match self.value_for_key(&__key, __state)? {
                __deser::__derive::Some(__sink) => __sink,
                __deser::__derive::None => __deser::de::SinkHandle::null(),
            },
        })
    } else {
        None
    };
    let other_key_atom_dispatch = if has_flatten {
        Some(quote! {
            __Key::Other(__key) => match self.value_for_key(&__key, __state)? {
                __deser::__derive::Some(__sink) => __deser::__derive::atom_into_handle(__sink, __atom, __state),
                __deser::__derive::None => __deser::__derive::Ok(()),
            },
        })
    } else {
        None
    };
    let other_key_borrowed_atom_dispatch = if has_flatten {
        Some(quote! {
            __Key::Other(__key) => match self.value_for_key(&__key, __state)? {
                __deser::__derive::Some(__sink) => __deser::__derive::borrowed_atom_into_handle(__sink, __atom, __state),
                __deser::__derive::None => __deser::__derive::Ok(()),
            },
        })
    } else {
        None
    };

    Ok(quote! {
        const _: () = {
            enum __Key {
                Unknown,
                Field(usize),
                #other_key_variant
            }

            struct __KeySink {
                key: __Key,
            }

            impl<'de> __deser::de::Sink<'de> for __KeySink {
                fn atom(
                    &mut self,
                    __atom: __deser::Atom,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<()> {
                    match __atom {
                        __deser::Atom::Str(__other) => {
                            self.key = match &__other as &__deser::__derive::str {
                                #(
                                    #key_matcher
                                )*
                                _ => #other_key_match,
                            };
                            __deser::__derive::Ok(())
                        }
                        __other => self.unexpected_atom(__other, __state),
                    }
                }

                fn expecting(&self) -> __deser::__derive::StrCow<'_> {
                    __deser::__derive::StrCow::Borrowed("string")
                }
            }

            struct __Sink #wrapper_impl_generics #where_clause {
                slot: &'__a mut __deser::__derive::Option<#ident #ty_generics>,
                key: __KeySink,
                #(
                    #sink_fieldname: #sink_fieldty,
                )*
                _marker: __deser::__derive::PhantomData<&'de ()>,
            }

            #[automatically_derived]
            impl #impl_generics __deser::Deserialize<'de> for #ident #ty_generics #bounded_where_clause {
                fn deserialize_into(
                    __slot: &mut __deser::__derive::Option<Self>,
                ) -> __deser::de::SinkHandle<'_, 'de> {
                    __deser::de::SinkHandle::boxed(__Sink {
                        slot: __slot,
                        key: __KeySink { key: __Key::Unknown },
                        #(
                            #sink_fieldname: #sink_defaults,
                        )*
                        _marker: __deser::__derive::PhantomData,
                    })
                }
            }

            #[automatically_derived]
            impl #wrapper_impl_generics __deser::de::Sink<'de> for __Sink #wrapper_ty_generics #bounded_where_clause {
                fn descriptor(&self) -> &'static dyn __deser::Descriptor {
                    &__Descriptor
                }

                fn map(&mut self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    __deser::__derive::Ok(())
                }

                fn next_key(&mut self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::de::SinkHandle<'_, 'de>>
                {
                    self.key.key = __Key::Unknown;
                    __deser::__derive::Ok(__deser::de::SinkHandle::to(&mut self.key))
                }

                fn next_value(&mut self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::de::SinkHandle<'_, 'de>>
                {
                    __deser::__derive::Ok(match __deser::__derive::replace(&mut self.key.key, __Key::Unknown) {
                        #(
                            #key_dispatch
                        )*
                        #other_key_dispatch
                        #[allow(unreachable_patterns)]
                        __Key::Unknown | __Key::Field(_) => __deser::de::SinkHandle::null(),
                    })
                }

                fn key_atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.key.key = __Key::Unknown;
                    __deser::de::Sink::atom(&mut self.key, __atom, __state)
                }

                fn value_atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    match __deser::__derive::replace(&mut self.key.key, __Key::Unknown) {
                        #(
                            #key_atom_dispatch
                        )*
                        #other_key_atom_dispatch
                        #[allow(unreachable_patterns)]
                        __Key::Unknown | __Key::Field(_) => __deser::__derive::Ok(()),
                    }
                }

                fn borrowed_key_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    // keys are only matched, they do not need to be borrowed
                    self.key.key = __Key::Unknown;
                    __deser::de::Sink::atom(&mut self.key, __atom, __state)
                }

                fn borrowed_value_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    match __deser::__derive::replace(&mut self.key.key, __Key::Unknown) {
                        #(
                            #key_borrowed_atom_dispatch
                        )*
                        #other_key_borrowed_atom_dispatch
                        #[allow(unreachable_patterns)]
                        __Key::Unknown | __Key::Field(_) => __deser::__derive::Ok(()),
                    }
                }

                fn value_for_key(&mut self, __key: &str, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::__derive::Option<__deser::de::SinkHandle<'_, 'de>>>
                {
                    match __key {
                        #(
                            #matcher
                        )*
                        __other => {
                            #(
                                if let __deser::__derive::Some(__sink) = self.#flatten_fields.borrow_mut().value_for_key(__other, __state)? {
                                    return __deser::__derive::Ok(__deser::__derive::Some(__sink));
                                }
                            )*
                        }
                    }
                    __deser::__derive::Ok(__deser::__derive::None)
                }

                fn finish(&mut self, __state: &mut __deser::State) -> __deser::__derive::Result<()> {
                    #![allow(unused_mut)]
                    #(
                        self.#flatten_fields.borrow_mut().finish(__state)?;
                    )*
                    #(
                        let mut #sink_fieldname = self.#sink_fieldname.#field_stage1_default;
                    )*
                    #stage2_default
                    *self.slot = __deser::__derive::Some(#ident {
                        #(
                            #fieldname: #field_take,
                        )*
                    });
                    __deser::__derive::Ok(())
                }
            }

            struct __Descriptor;

            impl __deser::Descriptor for __Descriptor {
                fn name(&self) -> __deser::__derive::Option<&__deser::__derive::str> {
                    __deser::__derive::Some(#type_name)
                }
            }
        };
    })
}

pub fn derive_enum(
    input: &syn::DeriveInput,
    enumeration: &syn::DataEnum,
) -> syn::Result<TokenStream> {
    let container_attrs = ContainerAttrs::of(input)?;
    if crate::enums::is_data_enum(input, &container_attrs, enumeration) {
        return crate::enums::derive_deserialize(input, enumeration, &container_attrs);
    }
    let ident = &input.ident;
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

    if let Some(attrs) = attrs.iter().find(|x| x.default()) {
        return Err(syn::Error::new_spanned(
            attrs.variant(),
            "default variants are only supported for internally and adjacently tagged enums",
        ));
    }

    let (fallback, non_str_fallback) = match attrs.iter().find(|x| x.other()) {
        Some(other) => {
            let var_ident = &other.variant().ident;
            (
                quote! { #ident::#var_ident },
                // other atoms are unknown tags too, extension values are
                // lowered first.
                quote! {
                    __other @ __deser::Atom::Ext(_) => return self.unexpected_atom(__other, __state),
                    _ => {
                        self.slot = __deser::__derive::Some(#ident::#var_ident);
                        return __deser::__derive::Ok(());
                    }
                },
            )
        }
        None => (
            quote! {
                return __deser::__derive::Err(
                    __deser::Error::new(__deser::ErrorKind::Unexpected, "unexpected value for enum")
                )
            },
            quote! {
                __other => return self.unexpected_atom(__other, __state),
            },
        ),
    };
    if attrs.iter().filter(|x| x.other()).count() > 1 {
        return Err(syn::Error::new(
            Span::call_site(),
            "only one variant can be marked as other",
        ));
    }

    Ok(quote! {
        const _: () = {
            #[repr(transparent)]
            struct __SlotWrapper {
                slot: __deser::__derive::Option<#ident>,
            }

            #[automatically_derived]
            impl<'de> __deser::de::Deserialize<'de> for #ident {
                fn deserialize_into(
                    __slot: &mut __deser::__derive::Option<Self>
                ) -> __deser::de::SinkHandle<'_, 'de> {
                    __deser::de::SinkHandle::to(unsafe {
                        &mut *{
                            __slot
                            as *mut __deser::__derive::Option<Self>
                            as *mut __SlotWrapper
                        }
                    })
                }

                #[inline]
                fn __private_atom_into(
                    __slot: &mut __deser::__derive::Option<Self>,
                    __atom: __deser::Atom,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<()> {
                    let __sink = unsafe {
                        &mut *{
                            __slot
                            as *mut __deser::__derive::Option<Self>
                            as *mut __SlotWrapper
                        }
                    };
                    __deser::de::Sink::<'de>::atom(__sink, __atom, __state)?;
                    __deser::de::Sink::<'de>::finish(__sink, __state)
                }

                #[inline]
                fn __private_borrowed_atom_into(
                    __slot: &mut __deser::__derive::Option<Self>,
                    __atom: __deser::Atom<'de>,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<()> {
                    Self::__private_atom_into(__slot, __atom, __state)
                }
            }

            impl<'de> __deser::de::Sink<'de> for __SlotWrapper {
                fn atom(
                    &mut self,
                    __atom: __deser::Atom,
                    __state: &mut __deser::State
                ) -> __deser::__derive::Result<()> {
                    let s = match __atom {
                        __deser::Atom::Str(ref s) => &s as &__deser::__derive::str,
                        #non_str_fallback
                    };
                    let value = match s {
                        #( #matcher => #ident::#var_idents, )*
                        _ => #fallback
                    };
                    self.slot = __deser::__derive::Some(value);
                    __deser::__derive::Ok(())
                }
            }
        };
    })
}

fn derive_newtype_struct(input: &syn::DeriveInput, field: &syn::Field) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let de_generics = with_de_lifetime(&input.generics)?;
    let (impl_generics, _, _) = de_generics.split_for_impl();

    // TODO: we want to report the type name here but the current descriptor
    // interface does not let us.  https://github.com/mitsuhiko/deser/issues/8
    let container_attrs = ContainerAttrs::of(input)?;
    let _type_name = container_attrs.container_name();

    let field_attrs = UnnamedFieldAttrs::of(field)?;
    if field_attrs.tag() {
        return Err(syn::Error::new_spanned(
            field,
            "tag fields are only supported in other variants of enums",
        ));
    }
    let adapter = field_attrs.adapter();

    let field_type = &field.ty;
    let make_sink = match adapter {
        Some(adapter) => quote! { __deser::de::OwnedSink::deserialize_as::<#adapter>() },
        None => quote! { __deser::de::OwnedSink::deserialize() },
    };
    let atom_into = atom_into(field_type, adapter, quote! { &mut __inner });
    let borrowed_atom_into = borrowed_atom_into(field_type, adapter, quote! { &mut __inner });

    let wrapper_generics = with_lifetime_bound(&de_generics, "'__a");
    let (wrapper_impl_generics, wrapper_ty_generics, _) = wrapper_generics.split_for_impl();
    let bounded_where_clause = where_clause_for_fields(
        &input.generics,
        quote!(__deser::Deserialize<'de>),
        None,
        quote!(__deser::adapters::DeserializeAs),
        Some(quote!('de)),
        container_attrs.deserialize_bound(),
        &[BoundField {
            ty: field_type,
            adapter,
        }],
    );

    Ok(quote! {
        const _: () = {
            struct __Sink #wrapper_impl_generics #where_clause {
                slot: &'__a mut __deser::__derive::Option<#ident #ty_generics>,
                sink: __deser::de::OwnedSink<'de, #field_type>,
            }

            #[automatically_derived]
            impl #impl_generics __deser::de::Deserialize<'de> for #ident #ty_generics #bounded_where_clause {
                fn deserialize_into(
                    __slot: &mut __deser::__derive::Option<Self>
                ) -> __deser::de::SinkHandle<'_, 'de> {
                    __deser::de::SinkHandle::boxed(__Sink {
                        slot: __slot,
                        sink: #make_sink,
                    })
                }

                #[inline]
                fn __private_atom_into(
                    __slot: &mut __deser::__derive::Option<Self>,
                    __atom: __deser::Atom,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<()> {
                    let mut __inner = __deser::__derive::None;
                    #atom_into?;
                    *__slot = __inner.map(#ident);
                    __deser::__derive::Ok(())
                }

                #[inline]
                fn __private_borrowed_atom_into(
                    __slot: &mut __deser::__derive::Option<Self>,
                    __atom: __deser::Atom<'de>,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<()> {
                    let mut __inner = __deser::__derive::None;
                    #borrowed_atom_into?;
                    *__slot = __inner.map(#ident);
                    __deser::__derive::Ok(())
                }
            }

            impl #wrapper_impl_generics __deser::de::Sink<'de> for __Sink #wrapper_ty_generics #bounded_where_clause {
                fn atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.borrow_mut().atom(__atom, __state)
                }

                fn borrowed_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.borrow_mut().borrowed_atom(__atom, __state)
                }

                fn map(&mut self, __state: &mut __deser::State) -> __deser::__derive::Result<()> {
                    self.sink.borrow_mut().map(__state)
                }

                fn seq(&mut self, __state: &mut __deser::State) -> __deser::__derive::Result<()>  {
                    self.sink.borrow_mut().seq(__state)
                }

                fn next_key(&mut self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::de::SinkHandle<'_, 'de>>
                {
                    self.sink.borrow_mut().next_key(__state)
                }

                fn next_value(&mut self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::de::SinkHandle<'_, 'de>>
                {
                    self.sink.borrow_mut().next_value(__state)
                }

                fn key_atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.borrow_mut().key_atom(__atom, __state)
                }

                fn value_atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.borrow_mut().value_atom(__atom, __state)
                }

                fn borrowed_key_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.borrow_mut().borrowed_key_atom(__atom, __state)
                }

                fn borrowed_value_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.borrow_mut().borrowed_value_atom(__atom, __state)
                }

                fn value_for_key(
                    &mut self,
                    __key: &str,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<__deser::__derive::Option<__deser::de::SinkHandle<'_, 'de>>> {
                    self.sink.borrow_mut().value_for_key(__key, __state)
                }

                fn finish(&mut self, __state: &mut __deser::State) -> __deser::__derive::Result<()> {
                    self.sink.borrow_mut().finish(__state)?;
                    *self.slot = self.sink.take().map(#ident);
                    Ok(())
                }

                fn descriptor(&self) -> &'static dyn __deser::Descriptor {
                    self.sink.borrow().descriptor()
                }

                fn expecting(&self) -> __deser::__derive::StrCow<'_> {
                    self.sink.borrow().expecting()
                }
            }
        };
    })
}
