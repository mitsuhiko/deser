use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

use crate::attr::{ContainerAttrs, EnumVariantAttrs, FieldAttrs, UnnamedFieldAttrs};
use crate::bound::{BoundField, where_clause_for_fields, with_lifetime_bound};

/// Returns an expression that creates a serialize handle for a value.
pub fn serialize_handle(
    ty: &syn::Type,
    adapter: Option<&syn::Type>,
    value: TokenStream,
) -> TokenStream {
    match adapter {
        // spanned so that errors about unsupported types point to the adapter
        Some(adapter) => quote_spanned! { adapter.span()=>
            __deser::ser::SerializeHandle::to(
                __deser::adapters::SerializeAsRef::<#adapter, #ty>::new(#value)
            )
        },
        None => quote! { __deser::ser::SerializeHandle::to(#value) },
    }
}

/// Returns an expression that checks if a value is optional.
pub fn is_optional(ty: &syn::Type, adapter: Option<&syn::Type>, value: TokenStream) -> TokenStream {
    match adapter {
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::adapters::SerializeAs<#ty>>::is_optional_as(#value)
        },
        None => quote! { __deser::ser::Serialize::is_optional(#value) },
    }
}

/// Returns the where clause for the fields of a struct.
fn struct_where_clause(
    input: &syn::DeriveInput,
    container_attrs: &ContainerAttrs,
    attrs: &[FieldAttrs],
) -> syn::WhereClause {
    where_clause_for_fields(
        &input.generics,
        quote!(__deser::Serialize),
        None,
        quote!(__deser::adapters::SerializeAs),
        None,
        container_attrs.serialize_bound(),
        &attrs
            .iter()
            .map(|x| BoundField {
                ty: &x.field().ty,
                adapter: x.adapter(),
            })
            .collect::<Vec<_>>(),
    )
}

/// Rejects tag fields outside of enums.
fn reject_tag_fields(attrs: &[FieldAttrs]) -> syn::Result<()> {
    match attrs.iter().find(|x| x.tag()) {
        Some(attrs) => Err(syn::Error::new_spanned(
            attrs.field(),
            "tag fields are only supported in other variants of enums",
        )),
        None => Ok(()),
    }
}

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
    reject_tag_fields(&attrs)?;

    if !attrs.iter().any(|x| x.flatten()) {
        return derive_indexed_struct(input, &container_attrs, &attrs);
    }

    let temp_emitter = if attrs.iter().any(|x| x.flatten()) {
        Some(quote! {
            nested_emitter: __deser::__derive::Option<__deser::__derive::Box<dyn __deser::ser::StructEmitter + '__a>>,
            nested_emitter_exhausted: bool,
        })
    } else {
        None
    };
    let temp_emitter_init = if attrs.iter().any(|x| x.flatten()) {
        Some(quote! {
            nested_emitter: __deser::__derive::None,
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
                let handle =
                    serialize_handle(&attrs.field().ty, attrs.adapter(), quote! { &self.data.#name });
                quote! {
                    #index => {
                        self.index = __index + 1;
                        #field_skip
                        let __handle = #handle;
                        #optional_skip
                        return __deser::__derive::Ok(__deser::__derive::Some((
                            __deser::__derive::Cow::Borrowed(#fieldstr),
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
                            self.nested_emitter = match __deser::ser::Serialize::serialize(&self.data.#name, __state)? {
                                __deser::ser::Chunk::Struct(__inner) => {
                                    Some(__inner)
                                }
                                _ => return __deser::__derive::Err(__deser::Error::new(
                                    __deser::ErrorKind::Unexpected,
                                    "unable to flatten on struct into struct"
                                ))
                            };
                            self.nested_emitter_exhausted = false;
                        }
                        match self.nested_emitter.as_mut().unwrap().next(__state)? {
                            __deser::__derive::None => {
                                self.index += 1;
                                self.nested_emitter_exhausted = true;
                                __deser::ser::Serialize::finish(&self.data.#name, __state)?;
                                continue;
                            }
                            // we need this transmute here because of limitations in the borrow
                            // checker.  The borrow checker does not understand that the borrow
                            // does not continue into the next loop iteration.  If polonius ever
                            // makes it into Rust this can go.
                            //
                            // This can be validated with `-Zpolonius`
                            __deser::__derive::Some((__key, __handle)) => {
                                #optional_skip
                                return __deser::__derive::Ok(__deser::__derive::Some(unsafe {
                                    ::std::mem::transmute::<
                                        (__deser::__derive::StrCow<'_>, __deser::ser::SerializeHandle<'_>),
                                        (__deser::__derive::StrCow<'_>, __deser::ser::SerializeHandle<'_>),
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
    let bounded_where_clause = struct_where_clause(input, &container_attrs, &attrs);

    Ok(quote! {
            const _: () = {
                #[automatically_derived]
                impl #impl_generics __deser::Serialize for #ident #ty_generics #bounded_where_clause {
                    fn descriptor(&self) -> &'static dyn __deser::Descriptor {
                        &__Descriptor
                    }
    __deser::__begin_without_finish!();

                    fn serialize(&self, __state: &mut __deser::State) -> __deser::__derive::Result<__deser::ser::Chunk<'_>> {
                        __deser::__derive::Ok(__deser::ser::Chunk::Struct(Box::new(__StructEmitter {
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

                impl __deser::Descriptor for __Descriptor {
                    fn name(&self) -> __deser::__derive::Option<&__deser::__derive::str> {
                        __deser::__derive::Some(#type_name)
                    }
                }

                #[automatically_derived]
                impl #wrapper_impl_generics __deser::ser::StructEmitter for __StructEmitter #wrapper_ty_generics #bounded_where_clause {
                    fn next(&mut self, __state: &mut __deser::State)
                        -> __deser::__derive::Result<__deser::__derive::Option<(__deser::__derive::StrCow<'_>, __deser::ser::SerializeHandle<'_>)>>
                    {
                        #[allow(clippy::never_loop)]
                        loop {
                            let __index = self.index;
                            match __index {
                                #(
                                    #state_handler
                                )*
                                _ => return __deser::__derive::Ok(__deser::__derive::None),
                            }
                        }
                    }
                }
            };
        })
}

/// Derives a struct without flattened fields.
///
/// Such structs provide their fields by index which lets the driver
/// serialize them without allocating an emitter.
fn derive_indexed_struct(
    input: &syn::DeriveInput,
    container_attrs: &ContainerAttrs,
    attrs: &[FieldAttrs],
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();
    let type_name = container_attrs.container_name();

    let field_arms = attrs
        .iter()
        .enumerate()
        .map(|(index, attrs)| {
            let name = &attrs.field().ident;
            let fieldstr = attrs.name(container_attrs);
            let field_skip = attrs.skip_serializing_if().map(|path| {
                quote! {
                    if #path(&self.#name) {
                        return __deser::__derive::Ok(__deser::ser::StructField::Skip);
                    }
                }
            });
            let ty = &attrs.field().ty;
            let optional_skip = if container_attrs.skip_serializing_optionals() {
                let is_optional = is_optional(ty, attrs.adapter(), quote! { &self.#name });
                Some(quote! {
                    if #is_optional {
                        return __deser::__derive::Ok(__deser::ser::StructField::Skip);
                    }
                })
            } else {
                None
            };
            let handle = serialize_handle(ty, attrs.adapter(), quote! { &self.#name });
            quote! {
                #index => {
                    #field_skip
                    #optional_skip
                    __deser::ser::StructField::Field(
                        #fieldstr,
                        #handle,
                    )
                }
            }
        })
        .collect::<Vec<_>>();

    let bounded_where_clause = struct_where_clause(input, container_attrs, attrs);

    Ok(quote! {
        const _: () = {
            #[automatically_derived]
            impl #impl_generics __deser::Serialize for #ident #ty_generics #bounded_where_clause {
                fn descriptor(&self) -> &'static dyn __deser::Descriptor {
                    &__Descriptor
                }

                fn serialize(&self, __state: &mut __deser::State) -> __deser::__derive::Result<__deser::ser::Chunk<'_>> {
                    __deser::__derive::Ok(__deser::ser::Chunk::Struct(__deser::__derive::Box::new(
                        __deser::ser::IndexedStructEmitter::new(self)
                    )))
                }

                #[inline]
                fn __private_begin(&self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::ser::Begin<'_>>
                {
                    __deser::__derive::Ok(__deser::ser::Begin::indexed_struct(self, &__Descriptor))
                }
            }

            #[automatically_derived]
            impl #impl_generics __deser::ser::IndexedStruct for #ident #ty_generics #bounded_where_clause {
                fn field(&self, __index: usize, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::ser::StructField<'_>>
                {
                    __deser::__derive::Ok(match __index {
                        #(
                            #field_arms
                        )*
                        _ => __deser::ser::StructField::End,
                    })
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

fn derive_enum(input: &syn::DeriveInput, enumeration: &syn::DataEnum) -> syn::Result<TokenStream> {
    let container_attrs = ContainerAttrs::of(input)?;
    if crate::enums::is_data_enum(input, &container_attrs, enumeration) {
        return crate::enums::derive_serialize(input, enumeration, &container_attrs);
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
    let names = attrs
        .iter()
        .map(|x| x.name(&container_attrs))
        .collect::<Vec<_>>();

    Ok(quote! {
            const _: () = {
                #[automatically_derived]
                impl __deser::Serialize for #ident {
    __deser::__begin_without_finish!();

                    fn serialize(&self, __state: &mut __deser::State)
                        -> __deser::__derive::Result<__deser::ser::Chunk<'_>>
                    {
                        __deser::__derive::Ok(match *self {
                            #(
                                #ident::#var_idents => {
                                    __deser::ser::Chunk::Atom(__deser::Atom::Str(__deser::__derive::Cow::Borrowed(#names)))
                                }
                            )*
                        })
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

    let field_attrs = UnnamedFieldAttrs::of(field)?;
    if field_attrs.tag() {
        return Err(syn::Error::new_spanned(
            field,
            "tag fields are only supported in other variants of enums",
        ));
    }
    let adapter = field_attrs.adapter();
    let field_type = &field.ty;
    // the value serializes through the adapter or the regular implementation
    let value = match adapter {
        Some(adapter) => quote! {
            __deser::adapters::SerializeAsRef::<#adapter, #field_type>::new(&self.0)
        },
        None => quote! { &self.0 },
    };

    let bounded_where_clause = where_clause_for_fields(
        &input.generics,
        quote!(__deser::Serialize),
        None,
        quote!(__deser::adapters::SerializeAs),
        None,
        container_attrs.serialize_bound(),
        &[BoundField {
            ty: field_type,
            adapter,
        }],
    );

    Ok(quote! {
        const _: () = {
            #[automatically_derived]
            impl #impl_generics __deser::Serialize for #ident #ty_generics #bounded_where_clause {
                fn descriptor(&self) -> &'static dyn __deser::Descriptor {
                    __deser::ser::Serialize::descriptor(#value)
                }
                fn serialize(&self, __state: &mut __deser::State) -> __deser::__derive::Result<__deser::ser::Chunk<'_>> {
                    __deser::ser::Serialize::serialize(#value, __state)
                }
                fn finish(&self, __state: &mut __deser::State) -> __deser::__derive::Result<()> {
                    __deser::ser::Serialize::finish(#value, __state)
                }
                fn is_optional(&self) -> bool {
                    __deser::ser::Serialize::is_optional(#value)
                }
                #[inline]
                fn __private_begin(&self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::ser::Begin<'_>>
                {
                    __deser::ser::Serialize::__private_begin(#value, __state)
                }
            }
        };
    })
}
