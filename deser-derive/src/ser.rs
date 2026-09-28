use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

use crate::attr::{ContainerAttrs, Direction, FieldAttrs};
use crate::bound::{where_clause_for_fields, with_lifetime_bound};
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
    let mut bound_fields = Vec::with_capacity(attrs.len());
    for x in attrs {
        bound_fields.push(x.bound_field(Direction::Serialize));
    }
    where_clause_for_fields(
        &input.generics,
        quote!(__deser::Serialize),
        Some(quote!(__deser::__derive::Sync)),
        quote!(__deser::adapters::SerializeAs),
        None,
        container_attrs.serialize_bound(),
        &bound_fields,
    )
}

/// Rejects tag fields outside of enums.
fn reject_tag_fields(attrs: &[FieldAttrs]) -> syn::Result<()> {
    for x in attrs {
        if x.tag() {
            return Err(syn::Error::new_spanned(
                x.field(),
                "tag fields are only supported in other variants of enums",
            ));
        }
    }
    Ok(())
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
    let all_attrs = FieldAttrs::of_all(&fields.named)?;
    reject_tag_fields(&all_attrs)?;
    let bounded_where_clause = struct_where_clause(input, &container_attrs, &all_attrs);
    // skipped fields are not serialized at all
    let mut attrs = Vec::with_capacity(all_attrs.len());
    let mut has_flatten = false;
    for x in &all_attrs {
        if !x.skip_serializing() {
            attrs.push(x);
            has_flatten |= x.flatten();
        }
    }

    if !has_flatten {
        return derive_indexed_struct(input, &container_attrs, &attrs, bounded_where_clause);
    }

    let temp_emitter = quote! {
        nested_emitter: __deser::__derive::Option<__deser::__derive::FlattenedStruct<'__a>>,
        nested_emitter_exhausted: bool,
    };
    let temp_emitter_init = quote! {
        nested_emitter: __deser::__derive::None,
        nested_emitter_exhausted: true,
    };
    let mut state_handler = Vec::with_capacity(attrs.len());
    for (index, attrs) in attrs.iter().enumerate() {
        state_handler.push({
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
                                    ::core::mem::transmute::<
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
        });
    }

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
                    __deser::__derive::Ok(__deser::ser::Chunk::structure(__StructEmitter {
                        data: self,
                        index: 0,
                        #temp_emitter_init
                    }, __state))
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

    let field_names = attrs.iter().map(|x| x.name(container_attrs));
    let mut field_arms = Vec::with_capacity(attrs.len());
    let mut plain_arms = Vec::with_capacity(attrs.len());
    let mut has_plain = false;
    let mut has_skip_if = false;
    for (index, attrs) in attrs.iter().enumerate() {
        has_skip_if |= attrs.skip_serializing_if().is_some();
        field_arms.push({
            let name = &attrs.field().ident;
            let fieldstr = attrs.name(container_attrs);
            let field_skip = attrs.skip_serializing_if().map(|path| {
                quote! {
                    if #path(&self.#name) {
                        return __deser::__derive::StructField::Skip;
                    }
                }
            });
            let ty = &attrs.field().ty;
            let optional_skip = if container_attrs.skip_serializing_optionals() {
                let is_optional = is_optional(ty, attrs.adapters().ser(), quote! { &self.#name });
                Some(quote! {
                    if #is_optional {
                        return __deser::__derive::StructField::Skip;
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
        });

        // Fields with plain values (without adapters) are emitted directly by
        // `emit_plain_field` which exists once per type of field, the skips are
        // the same as in `field`.
        if attrs.adapters().ser().is_some() {
            plain_arms.push(quote! {
                #index => return __deser::__derive::Ok(__index),
            });
            continue;
        }
        has_plain = true;
        plain_arms.push({
            let name = &attrs.field().ident;
            let fieldstr = attrs.name(container_attrs);
            let mut skip = Vec::new();
            if let Some(path) = attrs.skip_serializing_if() {
                skip.push(quote! { #path(&self.#name) });
            }
            if container_attrs.skip_serializing_optionals() {
                skip.push(quote! { __deser::ser::Serialize::is_optional(&self.#name) });
            }
            let emit = quote! {
                __deser::__derive::emit_plain_field(&self.#name, #fieldstr, __sink)
            };
            if skip.is_empty() {
                quote! { #index => #emit, }
            } else {
                quote! {
                    #index => if #(#skip)||* {
                        __deser::__derive::Ok(true)
                    } else {
                        #emit
                    },
                }
            }
        });
    }

    // without fields that can be plain the default (which emits nothing) is
    // used
    let emit_plain_fields = if !has_plain {
        None
    } else {
        Some(quote! {
            fn emit_plain_fields(
                &self,
                mut __index: usize,
                __sink: &mut dyn __deser::__derive::PlainSink,
            ) -> __deser::__derive::Result<usize> {
                loop {
                    let __emitted = match __index {
                        #(#plain_arms)*
                        _ => return __deser::__derive::Ok(__deser::__derive::FIELDS_END),
                    };
                    match __emitted {
                        __deser::__derive::Ok(true) => __index += 1,
                        __deser::__derive::Ok(false) => return __deser::__derive::Ok(__index),
                        __deser::__derive::Err(__err) => return __deser::__derive::Err(__err),
                    }
                }
            }
        })
    };

    // the number of fields is only known if none can be skipped
    let shape = if container_attrs.skip_serializing_optionals() || has_skip_if {
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
                    const __FIELDS: &[&str] = &[#(#field_names),*];
                    __d.structure(#type_name);
                    __d.fields(__FIELDS);
                }

                fn container_shape(&self) -> __deser::ContainerShape {
                    #shape
                }

                fn serialize(&self, __state: &mut __deser::State) -> __deser::__derive::Result<__deser::ser::Chunk<'_>> {
                    __deser::__derive::Ok(__deser::ser::Chunk::structure(__deser::__derive::IndexedStructEmitter::new(self), __state))
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
                fn field(&self, __index: usize) -> __deser::__derive::StructField<'_> {
                    match __index {
                        #(
                            #field_arms
                        )*
                        _ => __deser::__derive::StructField::End,
                    }
                }

                #emit_plain_fields
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
    let (var_idents, attrs) = crate::enums::unit_variants(enumeration)?;
    let type_name = container_attrs.container_name();
    let mut names = Vec::with_capacity(attrs.len());
    let mut atoms = Vec::with_capacity(attrs.len());
    let mut index_arms = Vec::with_capacity(attrs.len());
    for (index, x) in attrs.iter().enumerate() {
        let name = x.name(&container_attrs);
        names.push(name.str_expr());
        atoms.push(if x.skip_serializing() {
            let variant = x.variant().ident.to_string();
            quote! { __deser::__derive::UnitName::Skipped(#variant) }
        } else {
            name.unit_name()
        });
        let var_ident = var_idents[index];
        index_arms.push(quote! { #ident::#var_ident => #index, });
    }

    // Unit enums only generate the names and a function which returns the
    // index of a variant, the rest exists once for all types.
    let ser_trait = crate::forward::serialize_trait(&container_attrs);
    Ok(quote! {
        const _: () = {
            const __VARIANTS: __deser::__derive::UnitVariants = __deser::__derive::UnitVariants {
                type_name: #type_name,
                names: &[#(#names),*],
                atoms: &[#(#atoms),*],
            };

            fn __index(__value: &#ident) -> usize {
                match *__value {
                    #(#index_arms)*
                }
            }

            #[automatically_derived]
            impl #ser_trait for #ident {
                #[inline]
                fn __private_begin(
                    &self,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<__deser::__derive::Begin<'_>> {
                    __deser::__derive::begin_unit(&__VARIANTS, __index(self))
                }

                fn describe(&self, __d: &mut dyn __deser::ser::Describe) {
                    __deser::__derive::describe_unit(__d, &__VARIANTS, __index(self))
                }

                fn serialize(&self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::ser::Chunk<'_>>
                {
                    __deser::__derive::serialize_unit(&__VARIANTS, __index(self))
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

    let mut element_arms = Vec::with_capacity(fields.len());
    for (index, field) in fields.iter().enumerate() {
        let member = &field.member;
        let handle = serialize_handle(
            field.ty(),
            field.attrs.adapters().ser(),
            quote! { &self.#member },
        );
        element_arms.push(quote! {
            #index => #handle,
        });
    }
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
                    __deser::__derive::Ok(__deser::ser::Chunk::seq(__deser::__derive::IndexedSeqEmitter::new(self), __state))
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
