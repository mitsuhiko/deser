use proc_macro2::{Span, TokenStream};
use quote::quote;

use crate::attr::{ensure_no_field_attrs, ContainerAttrs, EnumVariantAttrs, FieldAttrs};
use crate::bound::{where_clause_with_bound, with_lifetime_bound};

pub fn derive_serialize(input: &mut syn::DeriveInput) -> syn::Result<TokenStream> {
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

    let container_attrs = ContainerAttrs::of(input)?;
    let type_name = container_attrs.container_name();
    let attrs = fields
        .named
        .iter()
        .map(FieldAttrs::of)
        .collect::<syn::Result<Vec<_>>>()?;

    let temp_emitter = if attrs.iter().any(|x| x.flatten()) {
        Some(quote! {
            nested_emitter: ::deser::__derive::Option<::deser::__derive::Box<dyn ::deser::ser::StructEmitter + '__a>>,
            nested_emitter_exhausted: bool,
        })
    } else {
        None
    };
    let temp_emitter_init = if attrs.iter().any(|x| x.flatten()) {
        Some(quote! {
            nested_emitter: ::deser::__derive::None,
            nested_emitter_exhausted: true,
        })
    } else {
        None
    };
    let state_handler = attrs
        .iter()
        .enumerate()
        .map(|(index, attrs)| {
            let name = &attrs.field().ident;
            let optional_skip = if container_attrs.skip_serializing_optionals() {
                quote! {
                    if __handle.is_optional() {
                        continue;
                    }
                }
            } else {
                quote! {}
            };
            if !attrs.flatten() {
                let fieldstr = attrs.name(&container_attrs);
                let field_skip = if let Some(path) = attrs.skip_serializing_if() {
                    quote! {
                        if #path(&self.data.#name) {
                            continue;
                        }
                    }
                } else {
                    quote! {}
                };
                quote! {
                    #index => {
                        self.index = __index + 1;
                        #field_skip
                        let __handle = ::deser::ser::SerializeHandle::to(&self.data.#name);
                        #optional_skip
                        return ::deser::__derive::Ok(::deser::__derive::Some((
                            ::deser::__derive::Cow::Borrowed(#fieldstr),
                            __handle,
                        )));
                    }
                }
            } else {
                let field_skip = if let Some(path) = attrs.skip_serializing_if() {
                    quote! {
                        if #path(&self.data.#name) {
                            self.index += 1;
                            continue;
                        }
                    }
                } else {
                    quote! {}
                };
                quote! {
                    #index => {
                        #field_skip
                        if self.nested_emitter_exhausted {
                            self.nested_emitter = match ::deser::ser::Serialize::serialize(&self.data.#name, __state)? {
                                ::deser::ser::Chunk::Struct(__inner) => {
                                    Some(__inner)
                                }
                                _ => return ::deser::__derive::Err(::deser::Error::new(
                                    ::deser::ErrorKind::Unexpected,
                                    "unable to flatten on struct into struct"
                                ))
                            };
                            self.nested_emitter_exhausted = false;
                        }
                        match self.nested_emitter.as_mut().unwrap().next(__state)? {
                            ::deser::__derive::None => {
                                self.index += 1;
                                self.nested_emitter_exhausted = true;
                                ::deser::ser::Serialize::finish(&self.data.#name, __state)?;
                                continue;
                            }
                            // we need this transmute here because of limitations in the borrow
                            // checker.  The borrow checker does not understand that the borrow
                            // does not continue into the next loop iteration.  If polonius ever
                            // makes it into Rust this can go.
                            //
                            // This can be validated with `-Zpolonius`
                            ::deser::__derive::Some((__key, __handle)) => {
                                #optional_skip
                                return ::deser::__derive::Ok(::deser::__derive::Some(unsafe {
                                    ::std::mem::transmute::<
                                        (::deser::__derive::StrCow<'_>, ::deser::ser::SerializeHandle<'_>),
                                        (::deser::__derive::StrCow<'_>, ::deser::ser::SerializeHandle<'_>),
                                    >((
                                        __key,
                                        __handle
                                    ))
                                }))
                            }
                        }
                    }
                }
            }
        })
        .collect::<Vec<_>>();

    let wrapper_generics = with_lifetime_bound(&input.generics, "'__a");
    let (wrapper_impl_generics, wrapper_ty_generics, _) = wrapper_generics.split_for_impl();
    let bound = syn::parse_quote!(::deser::Serialize);
    let bounded_where_clause = where_clause_with_bound(&input.generics, bound);

    Ok(quote! {
        const _: () = {
            #[automatically_derived]
            impl #impl_generics ::deser::Serialize for #ident #ty_generics #bounded_where_clause {
                fn descriptor(&self) -> &dyn ::deser::Descriptor {
                    &__Descriptor
                }
                fn serialize(&self, __state: &::deser::ser::SerializerState) -> ::deser::__derive::Result<::deser::ser::Chunk<'_>> {
                    ::deser::__derive::Ok(::deser::ser::Chunk::Struct(Box::new(__StructEmitter {
                        data: self,
                        index: 0,
                        #temp_emitter_init
                    })))
                }
            }

            struct __StructEmitter #wrapper_impl_generics #where_clause {
                data: &'__a #ident #ty_generics,
                index: usize,
                #temp_emitter
            }

            struct __Descriptor;

            impl ::deser::Descriptor for __Descriptor {
                fn name(&self) -> ::deser::__derive::Option<&::deser::__derive::str> {
                    ::deser::__derive::Some(#type_name)
                }
            }

            #[automatically_derived]
            impl #wrapper_impl_generics ::deser::ser::StructEmitter for __StructEmitter #wrapper_ty_generics #bounded_where_clause {
                fn next(&mut self, __state: &::deser::ser::SerializerState)
                    -> ::deser::__derive::Result<::deser::__derive::Option<(::deser::__derive::StrCow<'_>, ::deser::ser::SerializeHandle<'_>)>>
                {
                    #[allow(clippy::never_loop)]
                    loop {
                        let __index = self.index;
                        match __index {
                            #(
                                #state_handler
                            )*
                            _ => return ::deser::__derive::Ok(::deser::__derive::None),
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

    let container_attrs = ContainerAttrs::of(input)?;
    if let Some(tag) = container_attrs.tag() {
        return derive_tagged_enum(input, enumeration, &container_attrs, tag);
    }
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
        const _: () = {
            #[automatically_derived]
            impl ::deser::Serialize for #ident {
                fn serialize(&self, __state: &::deser::ser::SerializerState)
                    -> ::deser::__derive::Result<::deser::ser::Chunk<'_>>
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

/// Derives serialization for internally tagged enums.
///
/// The variants are serialized as structs with the tag as first field.
fn derive_tagged_enum(
    input: &syn::DeriveInput,
    enumeration: &syn::DataEnum,
    container_attrs: &ContainerAttrs,
    tag: &str,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let type_name = container_attrs.container_name();
    let mut arms = Vec::new();

    for variant in &enumeration.variants {
        let attrs = EnumVariantAttrs::of(variant)?;
        let var_ident = &variant.ident;
        let name = attrs.name(container_attrs).to_string();
        let tag_item = quote! {
            0 => ::deser::__derive::Some((
                ::deser::__derive::Cow::Borrowed(#tag),
                ::deser::ser::SerializeHandle::to(&#name),
            )),
        };

        match &variant.fields {
            syn::Fields::Named(fields) => {
                let mut bindings = Vec::new();
                let mut field_arms = Vec::new();
                for (idx, field) in fields.named.iter().enumerate() {
                    let field_attrs = FieldAttrs::of(field)?;
                    if field_attrs.flatten() {
                        return Err(syn::Error::new_spanned(
                            field,
                            "flatten is not supported in tagged enum variants",
                        ));
                    }
                    let binding = &field.ident;
                    let index = idx + 1;
                    let field_name = field_attrs.plain_name().to_string();
                    let field_skip = field_attrs.skip_serializing_if().map(|path| {
                        quote! {
                            if #path(#binding) {
                                continue;
                            }
                        }
                    });
                    let optional_skip = if container_attrs.skip_serializing_optionals() {
                        Some(quote! {
                            if ::deser::ser::Serialize::is_optional(#binding) {
                                continue;
                            }
                        })
                    } else {
                        None
                    };
                    bindings.push(binding);
                    field_arms.push(quote! {
                        #index => {
                            #field_skip
                            #optional_skip
                            ::deser::__derive::Some((
                                ::deser::__derive::Cow::Borrowed(#field_name),
                                ::deser::ser::SerializeHandle::to(#binding),
                            ))
                        }
                    });
                }
                arms.push(quote! {
                    #ident::#var_ident { #(ref #bindings,)* } => match __index {
                        #tag_item
                        #(#field_arms)*
                        _ => ::deser::__derive::None,
                    },
                });
            }
            syn::Fields::Unit => {
                arms.push(quote! {
                    #ident::#var_ident => match __index {
                        #tag_item
                        _ => ::deser::__derive::None,
                    },
                });
            }
            syn::Fields::Unnamed(_) => {
                return Err(syn::Error::new_spanned(
                    variant,
                    "tagged enums only support unit and struct variants",
                ))
            }
        }
    }

    Ok(quote! {
        const _: () = {
            struct __Descriptor;

            impl ::deser::Descriptor for __Descriptor {
                fn name(&self) -> ::deser::__derive::Option<&::deser::__derive::str> {
                    ::deser::__derive::Some(#type_name)
                }
            }

            struct __TaggedEmitter<'__a> {
                data: &'__a #ident,
                index: usize,
            }

            #[automatically_derived]
            impl ::deser::Serialize for #ident {
                fn descriptor(&self) -> &dyn ::deser::Descriptor {
                    &__Descriptor
                }

                fn serialize(
                    &self,
                    __state: &::deser::ser::SerializerState,
                ) -> ::deser::__derive::Result<::deser::ser::Chunk<'_>> {
                    ::deser::__derive::Ok(::deser::ser::Chunk::Struct(::deser::__derive::Box::new(
                        __TaggedEmitter {
                            data: self,
                            index: 0,
                        },
                    )))
                }
            }

            impl<'__a> ::deser::ser::StructEmitter for __TaggedEmitter<'__a> {
                fn next(
                    &mut self,
                    __state: &::deser::ser::SerializerState,
                ) -> ::deser::__derive::Result<
                    ::deser::__derive::Option<(
                        ::deser::__derive::StrCow<'_>,
                        ::deser::ser::SerializeHandle<'_>,
                    )>,
                > {
                    #[allow(clippy::never_loop)]
                    loop {
                        let __index = self.index;
                        self.index += 1;
                        return ::deser::__derive::Ok(match *self.data {
                            #(#arms)*
                        });
                    }
                }
            }
        };
    })
}

fn derive_newtype_struct(input: &syn::DeriveInput, field: &syn::Field) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();

    // TODO: we want to report the type name here but the current descriptor
    // interface does not let us.  https://github.com/mitsuhiko/deser/issues/8
    let container_attrs = ContainerAttrs::of(input)?;
    let _type_name = container_attrs.container_name();

    ensure_no_field_attrs(field)?;

    let bound = syn::parse_quote!(::deser::Serialize);
    let bounded_where_clause = where_clause_with_bound(&input.generics, bound);

    Ok(quote! {
        const _: () = {
            #[automatically_derived]
            impl #impl_generics ::deser::Serialize for #ident #ty_generics #bounded_where_clause {
                fn descriptor(&self) -> &dyn ::deser::Descriptor {
                    self.0.descriptor()
                }
                fn serialize(&self, __state: &::deser::ser::SerializerState) -> ::deser::__derive::Result<::deser::ser::Chunk<'_>> {
                    ::deser::ser::Serialize::serialize(&self.0, __state)
                }
                fn finish(&self, __state: &::deser::ser::SerializerState) -> ::deser::__derive::Result<()> {
                    ::deser::ser::Serialize::finish(&self.0, __state)
                }
                fn is_optional(&self) -> bool {
                    ::deser::ser::Serialize::is_optional(&self.0)
                }
            }
        };
    })
}
