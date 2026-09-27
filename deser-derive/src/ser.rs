use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

use crate::attr::{ContainerAttrs, Direction, EnumVariantAttrs, FieldAttrs};
use crate::bound::{BoundField, where_clause_for_fields, with_lifetime_bound};
use crate::unnamed::{NewtypeField, UnnamedField, UnnamedStruct};

/// Returns an expression that creates a serialize handle for a value.
/// Implements `__private_begin` for types which do not implement `finish`.
///
/// This is the same as the `begin_without_finish!` macro of deser.
fn begin_without_finish() -> TokenStream {
    quote! {
        #[inline]
        fn __private_begin(
            &self,
            __state: &mut __deser::State,
        ) -> __deser::__derive::Result<__deser::__derive::Begin<'_>> {
            let __shape = __deser::ser::Serialize::container_shape(self);
            __deser::__derive::Ok(__deser::__derive::Begin::chunk(
                __deser::ser::Serialize::serialize(self, __state)?,
                __shape,
                false,
            ))
        }
    }
}

pub fn serialize_handle(
    ty: &syn::Type,
    adapter: Option<&syn::Type>,
    value: TokenStream,
) -> TokenStream {
    match adapter {
        // spanned so that errors about unsupported types point to the adapter
        Some(adapter) => quote_spanned! { adapter.span()=>
            __deser::ser::SerializeHandle::to(
                __deser::__derive::SerializeAsRef::<#adapter, #ty>::new(#value)
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
///
/// The fields are all fields, including the skipped ones.
fn struct_where_clause(
    input: &syn::DeriveInput,
    container_attrs: &ContainerAttrs,
    attrs: &[FieldAttrs],
) -> syn::WhereClause {
    where_clause_for_fields(
        &input.generics,
        quote!(__deser::Serialize),
        Some(quote!(__deser::__derive::Sync)),
        quote!(__deser::adapters::SerializeAs),
        None,
        container_attrs.serialize_bound(),
        &attrs
            .iter()
            .map(|x| BoundField {
                ty: &x.field().ty,
                adapter: x.adapters().ser(),
                skipped: x.skip_serializing(),
                bound: x.bounds().get(Direction::Serialize),
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
    // with an adapter that wraps the derived implementation, both are
    // needed
    let forward = match crate::forward::derive_serialize(input)? {
        Some((rv, false)) => return Ok(rv),
        Some((rv, true)) => Some(rv),
        None => None,
    };
    let derived = derive_serialize_impl(input)?;
    Ok(quote! {
        #forward
        #derived
    })
}

/// Derives the implementation (as `Serialize` or `DerivedSerialize`).
fn derive_serialize_impl(input: &syn::DeriveInput) -> syn::Result<TokenStream> {
    if let Some(rv) = crate::transparent::derive(input, Direction::Serialize)? {
        return Ok(rv);
    }
    if let Some(st) = UnnamedStruct::of(input)? {
        return derive_unnamed_struct(input, &st);
    }
    match &input.data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Named(fields),
            ..
        }) => derive_struct(input, fields),
        syn::Data::Enum(enumeration) => derive_enum(input, enumeration),
        _ => Err(crate::unsupported_union(input)),
    }
}

fn derive_struct(input: &syn::DeriveInput, fields: &syn::FieldsNamed) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let container_attrs = ContainerAttrs::of(input, Direction::Serialize)?;
    let type_name = container_attrs.container_name();
    let all_attrs = fields
        .named
        .iter()
        .map(FieldAttrs::of)
        .collect::<syn::Result<Vec<_>>>()?;
    reject_tag_fields(&all_attrs)?;
    let bounded_where_clause = struct_where_clause(input, &container_attrs, &all_attrs);
    // skipped fields are not serialized at all
    let attrs = all_attrs
        .iter()
        .filter(|x| !x.skip_serializing())
        .collect::<Vec<_>>();

    if !attrs.iter().any(|x| x.flatten()) {
        return derive_indexed_struct(input, &container_attrs, &attrs, bounded_where_clause);
    }

    let temp_emitter = if attrs.iter().any(|x| x.flatten()) {
        Some(quote! {
            nested_emitter: __deser::__derive::Option<__deser::__derive::FlattenedStruct<'__a>>,
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
                    serialize_handle(&attrs.field().ty, attrs.adapters().ser(), quote! { &self.data.#name });
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
                            // values that forward (for instance because of
                            // an adapter on their type) are followed
                            self.nested_emitter = __deser::__derive::Some(
                                __deser::__derive::FlattenedStruct::new(&self.data.#name, __state)?
                            );
                            self.nested_emitter_exhausted = false;
                        }
                        match self.nested_emitter.as_mut().unwrap().next(__state)? {
                            __deser::__derive::None => {
                                self.index += 1;
                                self.nested_emitter_exhausted = true;
                                // the values it forwarded to were finished
                                // with the last field, now the value itself
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
    let begin_without_finish = begin_without_finish();

    let ser_trait = crate::forward::serialize_trait(&container_attrs);
    Ok(quote! {
        const _: () = {
            #[automatically_derived]
            impl #impl_generics #ser_trait for #ident #ty_generics #bounded_where_clause {
                #begin_without_finish

                fn describe(&self, __d: &mut dyn __deser::ser::Describe) {
                    __d.structure(#type_name);
                }

                fn serialize(&self, __state: &mut __deser::State) -> __deser::__derive::Result<__deser::ser::Chunk<'_>> {
                    __deser::__derive::Ok(__deser::ser::Chunk::Struct(__deser::__derive::Box::new(__StructEmitter {
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
    attrs: &[&FieldAttrs],
    bounded_where_clause: syn::WhereClause,
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
                        return __deser::__derive::Ok(__deser::__derive::StructField::Skip);
                    }
                }
            });
            let ty = &attrs.field().ty;
            let optional_skip = if container_attrs.skip_serializing_optionals() {
                let is_optional = is_optional(ty, attrs.adapters().ser(), quote! { &self.#name });
                Some(quote! {
                    if #is_optional {
                        return __deser::__derive::Ok(__deser::__derive::StructField::Skip);
                    }
                })
            } else {
                None
            };
            let handle = serialize_handle(ty, attrs.adapters().ser(), quote! { &self.#name });
            quote! {
                #index => {
                    #field_skip
                    #optional_skip
                    __deser::__derive::StructField::Field(
                        #fieldstr,
                        #handle,
                    )
                }
            }
        })
        .collect::<Vec<_>>();

    // fields with plain values (without adapters) are emitted directly, the
    // skips are the same as in `field`.
    let plain_arms = attrs
        .iter()
        .enumerate()
        .map(|(index, attrs)| {
            if attrs.adapters().ser().is_some() {
                return quote! {
                    #index => return __deser::__derive::Ok(__index),
                };
            }
            let name = &attrs.field().ident;
            let fieldstr = attrs.name(container_attrs);
            let field_skip = attrs.skip_serializing_if().map(|path| {
                quote! {
                    if #path(&self.#name) {
                        break '__field;
                    }
                }
            });
            let optional_skip = if container_attrs.skip_serializing_optionals() {
                Some(quote! {
                    if __deser::ser::Serialize::is_optional(&self.#name) {
                        break '__field;
                    }
                })
            } else {
                None
            };
            quote! {
                #index => {
                    if !__deser::Serialize::__private_is_plain_value(&self.#name) {
                        return __deser::__derive::Ok(__index);
                    }
                    '__field: {
                        #field_skip
                        #optional_skip
                        __sink.field(#fieldstr)?;
                        __deser::Serialize::__private_emit_plain(&self.#name, __sink)?;
                    }
                }
            }
        })
        .collect::<Vec<_>>();

    // the number of fields is only known if none can be skipped
    let shape = if container_attrs.skip_serializing_optionals()
        || attrs.iter().any(|x| x.skip_serializing_if().is_some())
    {
        quote! { __deser::ContainerShape::new() }
    } else {
        let len = attrs.len();
        quote! { __deser::ContainerShape::new().with_len(#len) }
    };

    let ser_trait = crate::forward::serialize_trait(container_attrs);
    Ok(quote! {
        const _: () = {
            #[automatically_derived]
            impl #impl_generics #ser_trait for #ident #ty_generics #bounded_where_clause {
                fn describe(&self, __d: &mut dyn __deser::ser::Describe) {
                    __d.structure(#type_name);
                }

                fn container_shape(&self) -> __deser::ContainerShape {
                    #shape
                }

                fn serialize(&self, __state: &mut __deser::State) -> __deser::__derive::Result<__deser::ser::Chunk<'_>> {
                    __deser::__derive::Ok(__deser::ser::Chunk::Struct(__deser::__derive::Box::new(
                        __deser::__derive::IndexedStructEmitter::new(self)
                    )))
                }

                #[inline]
                fn __private_begin(&self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::__derive::Begin<'_>>
                {
                    __deser::__derive::Ok(__deser::__derive::Begin::indexed_struct(self, #shape))
                }
            }

            #[automatically_derived]
            impl #impl_generics __deser::__derive::IndexedStruct for #ident #ty_generics #bounded_where_clause {
                fn field(&self, __index: usize, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::__derive::StructField<'_>>
                {
                    __deser::__derive::Ok(match __index {
                        #(
                            #field_arms
                        )*
                        _ => __deser::__derive::StructField::End,
                    })
                }

                fn emit_plain_fields(
                    &self,
                    mut __index: usize,
                    __sink: &mut dyn __deser::__derive::PlainSink,
                ) -> __deser::__derive::Result<usize> {
                    loop {
                        match __index {
                            #(
                                #plain_arms
                            )*
                            _ => return __deser::__derive::Ok(__deser::__derive::FIELDS_END),
                        }
                        __index += 1;
                    }
                }
            }

        };
    })
}

fn derive_enum(input: &syn::DeriveInput, enumeration: &syn::DataEnum) -> syn::Result<TokenStream> {
    let container_attrs = ContainerAttrs::of(input, Direction::Serialize)?;
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
        .map(|x| x.name(&container_attrs).str_expr())
        .collect::<Vec<_>>();
    let type_name = container_attrs.container_name();
    let chunks = attrs
        .iter()
        .map(|x| {
            if x.skip_serializing() {
                let variant = x.variant().ident.to_string();
                return quote! {
                    return __deser::__derive::Err(
                        __deser::__derive::skipped_variant(#type_name, #variant)
                    )
                };
            }
            let atom = x.name(&container_attrs).atom();
            quote! { __deser::ser::Chunk::Atom(#atom) }
        })
        .collect::<Vec<_>>();
    // if all variants are named by strings, the atom is built once and only
    // the name is matched
    let serialize_body = if attrs
        .iter()
        .all(|x| x.name(&container_attrs).as_str().is_some() && !x.skip_serializing())
    {
        quote! {
            __deser::__derive::Ok(__deser::ser::Chunk::Atom(__deser::Atom::Str(
                __deser::__derive::Cow::Borrowed(match *self {
                    #(
                        #ident::#var_idents => #names,
                    )*
                })
            )))
        }
    } else {
        quote! {
            __deser::__derive::Ok(match *self {
                #(
                    #ident::#var_idents => { #chunks }
                )*
            })
        }
    };
    let begin_without_finish = begin_without_finish();

    let ser_trait = crate::forward::serialize_trait(&container_attrs);
    Ok(quote! {
        const _: () = {
            #[automatically_derived]
            impl #ser_trait for #ident {
                #begin_without_finish

                fn describe(&self, __d: &mut dyn __deser::ser::Describe) {
                    __d.variant(&__deser::ser::Variant::new(
                        #type_name,
                        match *self {
                            #(
                                #ident::#var_idents => #names,
                            )*
                        },
                        __deser::ser::VariantKind::Unit,
                        __deser::ser::VariantRepr::External,
                    ));
                }

                fn serialize(&self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::ser::Chunk<'_>>
                {
                    #serialize_body
                }
            }
        };
    })
}

/// Derives a struct with unnamed fields or a unit struct.
///
/// The representation depends on the fields that are serialized: without
/// fields the struct is null, with one field the value of the field and
/// with more fields a sequence.
fn derive_unnamed_struct(input: &syn::DeriveInput, st: &UnnamedStruct) -> syn::Result<TokenStream> {
    let container_attrs = ContainerAttrs::of(input, Direction::Serialize)?;
    container_attrs.reject_named_only(st.kind(), Direction::Serialize)?;
    let where_clause = where_clause_for_fields(
        &input.generics,
        quote!(__deser::Serialize),
        Some(quote!(__deser::__derive::Sync)),
        quote!(__deser::adapters::SerializeAs),
        None,
        container_attrs.serialize_bound(),
        &st.bound_fields(Direction::Serialize),
    );
    let remaining = st.remaining(Direction::Serialize);
    if container_attrs.transparent() && remaining.len() != 1 {
        return Err(crate::transparent::field_count_error(
            input,
            Direction::Serialize,
        ));
    }
    match remaining[..] {
        [] => derive_unit_struct(input, &container_attrs, where_clause),
        [field] => {
            let field = NewtypeField {
                member: syn::Member::Unnamed(field.member.clone()),
                ty: field.ty(),
                adapter: field.attrs.adapters().ser(),
                convert: TokenStream::new(),
            };
            derive_newtype_struct(input, &container_attrs, &field, where_clause)
        }
        _ => derive_tuple_struct(input, &container_attrs, &remaining, where_clause),
    }
}

/// Derives a tuple struct which is serialized as sequence.
fn derive_tuple_struct(
    input: &syn::DeriveInput,
    container_attrs: &ContainerAttrs,
    fields: &[&UnnamedField],
    bounded_where_clause: syn::WhereClause,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();
    let type_name = container_attrs.container_name();

    let element_arms = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let member = &field.member;
            let handle = serialize_handle(
                field.ty(),
                field.attrs.adapters().ser(),
                quote! { &self.#member },
            );
            quote! {
                #index => #handle,
            }
        })
        .collect::<Vec<_>>();
    let len = fields.len();

    let ser_trait = crate::forward::serialize_trait(container_attrs);
    Ok(quote! {
        const _: () = {
            #[automatically_derived]
            impl #impl_generics #ser_trait for #ident #ty_generics #bounded_where_clause {
                fn describe(&self, __d: &mut dyn __deser::ser::Describe) {
                    __d.tuple_struct(#type_name);
                }

                fn container_shape(&self) -> __deser::ContainerShape {
                    __deser::ContainerShape::new().with_len(#len)
                }

                fn serialize(&self, __state: &mut __deser::State) -> __deser::__derive::Result<__deser::ser::Chunk<'_>> {
                    __deser::__derive::Ok(__deser::ser::Chunk::Seq(__deser::__derive::Box::new(
                        __deser::__derive::IndexedSeqEmitter::new(self)
                    )))
                }

                #[inline]
                fn __private_begin(&self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::__derive::Begin<'_>>
                {
                    __deser::__derive::Ok(__deser::__derive::Begin::indexed_seq(
                        self,
                        __deser::ContainerShape::new().with_len(#len),
                    ))
                }
            }

            #[automatically_derived]
            impl #impl_generics __deser::__derive::IndexedSeq for #ident #ty_generics #bounded_where_clause {
                fn element(&self, __index: usize, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::__derive::Option<__deser::ser::SerializeHandle<'_>>>
                {
                    __deser::__derive::Ok(__deser::__derive::Some(match __index {
                        #(#element_arms)*
                        _ => return __deser::__derive::Ok(__deser::__derive::None),
                    }))
                }
            }
        };
    })
}

/// Derives a unit struct (or a struct without fields that are serialized)
/// which is serialized as null.
fn derive_unit_struct(
    input: &syn::DeriveInput,
    container_attrs: &ContainerAttrs,
    where_clause: syn::WhereClause,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();
    let type_name = container_attrs.container_name();
    let begin_without_finish = begin_without_finish();

    let ser_trait = crate::forward::serialize_trait(container_attrs);
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #ser_trait for #ident #ty_generics #where_clause {
            #begin_without_finish

            fn describe(&self, __d: &mut dyn __deser::ser::Describe) {
                __d.unit_struct(#type_name);
            }

            fn serialize(&self, __state: &mut __deser::State) -> __deser::__derive::Result<__deser::ser::Chunk<'_>> {
                __deser::__derive::Ok(__deser::ser::Chunk::Atom(__deser::Atom::Null))
            }
        }
    })
}

/// Derives a newtype struct (or a struct with one field that is serialized)
/// which is serialized as the value of the field.
pub(crate) fn derive_newtype_struct(
    input: &syn::DeriveInput,
    container_attrs: &ContainerAttrs,
    field: &NewtypeField,
    bounded_where_clause: syn::WhereClause,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();
    let type_name = container_attrs.container_name();

    let adapter = field.adapter;
    let field_type = field.ty;
    let member = &field.member;
    // the value serializes through the adapter or the regular implementation
    let value = match adapter {
        Some(adapter) => quote! {
            __deser::__derive::SerializeAsRef::<#adapter, #field_type>::new(&self.#member)
        },
        None => quote! { &self.#member },
    };

    let ser_trait = crate::forward::serialize_trait(container_attrs);
    Ok(quote! {
        const _: () = {
            #[automatically_derived]
            impl #impl_generics #ser_trait for #ident #ty_generics #bounded_where_clause {
                fn container_shape(&self) -> __deser::ContainerShape {
                    __deser::ser::Serialize::container_shape(#value)
                }
                fn describe(&self, __d: &mut dyn __deser::ser::Describe) {
                    __d.newtype(#type_name);
                    __deser::ser::Serialize::describe(#value, __d)
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
                    -> __deser::__derive::Result<__deser::__derive::Begin<'_>>
                {
                    __deser::ser::Serialize::__private_begin(#value, __state)
                }
            }
        };
    })
}
