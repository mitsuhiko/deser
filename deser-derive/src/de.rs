use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

use crate::attr::{
    ContainerAttrs, EnumVariantAttrs, FieldAttrs, Name, TypeDefault, UnnamedFieldAttrs, VariantName,
};
use crate::bound::{BoundField, where_clause_for_fields, with_de_lifetime, with_lifetime_bound};

/// Returns an expression that creates a sink handle for the slot of a field.
///
/// This calls a function that is not inlined so that the code to create the
/// sink exists once per type.
fn field_sink(ty: &syn::Type, adapter: Option<&syn::Type>, slot: TokenStream) -> TokenStream {
    match adapter {
        Some(adapter) => quote_spanned! { adapter.span()=>
            __deser::__derive::field_sink_as::<#adapter, #ty>(#slot)
        },
        None => quote! { __deser::__derive::field_sink(#slot) },
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

/// Returns a closure that validates a value with the function at the path.
///
/// The closure is passed where a `Validator` is expected.
pub(crate) fn validator(path: &syn::ExprPath) -> TokenStream {
    quote_spanned! { path.span()=>
        |__value| #path(__value).map_err(__deser::__derive::invalid_value)
    }
}

/// Returns an expression that validates the value in a slot.
fn validate_slot(path: Option<&syn::ExprPath>, slot: TokenStream) -> Option<TokenStream> {
    let validator = validator(path?);
    Some(quote! { __deser::__derive::validate_slot(#slot, #validator)? })
}

/// The generated code for updating structs in place.
struct UpdateSink {
    /// The `deserialize_update` method.
    method: TokenStream,
    /// The sink and its implementation.
    items: TokenStream,
}

pub fn derive_deserialize(input: &mut syn::DeriveInput) -> syn::Result<TokenStream> {
    if let Some(rv) = crate::forward::derive_deserialize(input)? {
        return Ok(rv);
    }
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
    let all_attrs = fields
        .named
        .iter()
        .map(FieldAttrs::of)
        .collect::<syn::Result<Vec<_>>>()?;
    if let Some(attrs) = all_attrs.iter().find(|x| x.tag()) {
        return Err(syn::Error::new_spanned(
            attrs.field(),
            "tag fields are only supported in other variants of enums",
        ));
    }
    // skipped fields are not deserialized, they are filled in when the
    // struct is built
    let attrs = all_attrs
        .iter()
        .filter(|x| !x.skip_deserializing())
        .collect::<Vec<_>>();
    let mut default_bounds = Vec::new();
    let (skipped_name, skipped_value): (Vec<_>, Vec<_>) = all_attrs
        .iter()
        .filter(|x| x.skip_deserializing())
        .map(|x| {
            let name = &x.field().ident;
            let ty = &x.field().ty;
            let value = match (x.default(), container_attrs.default()) {
                (Some(TypeDefault::Explicit(expr)), _) => expr.clone(),
                (None, Some(TypeDefault::Explicit(expr))) => quote! { (#expr).#name },
                (None, Some(TypeDefault::Implicit)) => quote! {
                    <#ident #ty_generics as __deser::__derive::Default>::default().#name
                },
                (Some(TypeDefault::Implicit), _) | (None, None) => {
                    default_bounds.push(quote! { #ty: __deser::__derive::Default });
                    quote! { <#ty as __deser::__derive::Default>::default() }
                }
            };
            (name, value)
        })
        .unzip();
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
            } else if f.default().is_some() || f.required() {
                // required fields are missing even if their type has a
                // value for missing fields
                quote! {
                    __deser::__derive::None
                }
            } else if let Some(adapter) = f.adapters().de() {
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
    let mut field_sinks = Vec::new();
    let mut field_atoms = Vec::new();
    let mut field_borrowed_atoms = Vec::new();
    let mut update_dispatch = Vec::new();
    for (index, (x, fieldname)) in attrs.iter().zip(sink_fieldname.iter()).enumerate() {
        if x.flatten() {
            continue;
        }

        let names = std::iter::once(x.name(&container_attrs))
            .chain(x.aliases(&container_attrs))
            .collect::<Vec<_>>();
        for name in &names {
            if first_duplicate_name.is_none() && !seen_names.insert(name.clone()) {
                first_duplicate_name = Some((name.display(), x.field()));
            }
        }
        let ty = &x.field().ty;
        let mut sink = field_sink(ty, x.adapters().de(), quote! { &mut self.#fieldname });
        let mut atom = atom_into(ty, x.adapters().de(), quote! { &mut self.#fieldname });
        let mut borrowed_atom =
            borrowed_atom_into(ty, x.adapters().de(), quote! { &mut self.#fieldname });
        // validated values are deserialized into an owned sink and moved
        // into the field once they were validated, atoms are validated in
        // the field directly
        if let Some(path) = x.validate() {
            let validator = validator(path);
            let owned = match x.adapters().de() {
                Some(adapter) => quote! { __deser::de::OwnedSink::deserialize_as::<#adapter>() },
                None => quote! { __deser::de::OwnedSink::deserialize() },
            };
            sink = quote! {
                __deser::__derive::validated(&mut self.#fieldname, #owned, #validator)
            };
            let validate = validate_slot(Some(path), quote! { &self.#fieldname });
            atom = quote! {{ #atom?; #validate; __deser::__derive::Ok(()) }};
            borrowed_atom = quote! {{ #borrowed_atom?; #validate; __deser::__derive::Ok(()) }};
        }
        // in updates, fields with adapters and validators are replaced
        // (after validating the new value), all others are updated
        let field_ident = &x.field().ident;
        let update = if x.adapters().de().is_none() && x.validate().is_none() {
            quote! { __deser::Deserialize::deserialize_update(&mut self.value.#field_ident) }
        } else {
            let owned = match x.adapters().de() {
                Some(adapter) => quote! { __deser::de::OwnedSink::deserialize_as::<#adapter>() },
                None => quote! { __deser::de::OwnedSink::deserialize() },
            };
            let validator = match x.validate() {
                Some(path) => {
                    let validator = validator(path);
                    quote! { __deser::__derive::Some(#validator) }
                }
                None => quote! { __deser::__derive::None },
            };
            quote! { __deser::__derive::replace_with(&mut self.value.#field_ident, #owned, #validator) }
        };
        update_dispatch.push(quote! {
            #index => #update,
        });
        key_matcher.push(Name::str_arms(
            &names,
            quote! { __deser::__derive::Some(#index) },
        ));
        field_sinks.push(quote! {
            #index => #sink,
        });
        field_atoms.push(quote! {
            #index => #atom,
        });
        field_borrowed_atoms.push(quote! {
            #index => #borrowed_atom,
        });
    }

    if let Some((first_duplicate_name, field)) = first_duplicate_name {
        return Err(syn::Error::new_spanned(
            field,
            format!("field name `{}` used more than once", first_duplicate_name),
        ));
    }

    let wrapper_generics = with_lifetime_bound(&de_generics, "'__a");
    let (wrapper_impl_generics, wrapper_ty_generics, _) = wrapper_generics.split_for_impl();
    let mut bounded_where_clause = where_clause_for_fields(
        &input.generics,
        quote!(__deser::Deserialize<'de>),
        Some(quote!(__deser::__derive::Send)),
        quote!(__deser::adapters::DeserializeAs),
        Some(quote!('de)),
        container_attrs.deserialize_bound(),
        &all_attrs
            .iter()
            .map(|x| BoundField {
                ty: &x.field().ty,
                adapter: x.adapters().de(),
                skipped: x.skip_deserializing(),
            })
            .collect::<Vec<_>>(),
    );
    // skipped fields of generic types need a default
    if container_attrs.deserialize_bound().is_none() {
        let params = input
            .generics
            .type_params()
            .map(|x| x.ident.to_string())
            .collect::<HashSet<_>>();
        for bound in default_bounds {
            let mut idents = HashSet::new();
            crate::bound::collect_idents(bound.clone(), &mut idents);
            if idents.iter().any(|x| params.contains(x)) {
                bounded_where_clause
                    .predicates
                    .push(syn::parse_quote!(#bound));
            }
        }
    }

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
                    "failed to deserialize flattened field `{}`",
                    attrs.field().ident.as_ref().unwrap()
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
                if attrs.flatten() { Some(name) } else { None }
            },
        )
        .collect::<Vec<_>>();
    // if a flattened field took a key and the value used if it did not
    let flatten_used = attrs
        .iter()
        .filter(|x| x.flatten())
        .map(|x| {
            let name = x.field().ident.as_ref().unwrap();
            syn::Ident::new(&format!("used_{}", name), Span::call_site())
        })
        .collect::<Vec<_>>();
    let flatten_initial = attrs
        .iter()
        .filter(|x| x.flatten())
        .map(|x| {
            let name = x.field().ident.as_ref().unwrap();
            syn::Ident::new(&format!("initial_{}", name), Span::call_site())
        })
        .collect::<Vec<_>>();
    let flatten_ty = attrs
        .iter()
        .filter(|x| x.flatten())
        .map(|x| &x.field().ty)
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

    // The names of the fields by index for errors about duplicate fields and
    // a bit per field to detect them.
    let field_names = attrs
        .iter()
        .map(|x| {
            if x.flatten() {
                quote! { "" }
            } else {
                let name = x.name(&container_attrs);
                quote! { #name }
            }
        })
        .collect::<Vec<_>>();
    let seen_words = attrs.len().div_ceil(64);

    // Keys are resolved to a field index directly in the key sink so that
    // deserializing a struct does not need to allocate a string per key.  The
    // names of unknown keys are only retained if they are needed: if
    // flattened fields exist (to look them up on the flattened sinks), if
    // they are rejected or if the policy for unknown fields wants them.  The
    // logic that does not depend on the fields is in `deser::__derive` so
    // that it exists once.
    let has_flatten = !flatten_fields.is_empty();
    let deny = container_attrs.deny_unknown_fields();
    let retain_unknown = has_flatten || deny;
    // without flattened fields the key sink handles unknown keys, with them
    // they are unknown if no flattened field takes them
    let unknown_field = quote! {
        __deser::__derive::unknown_field(&__key, __offset, __FIELDS, #deny, __state)?
    };
    let other_key_dispatch = if has_flatten {
        Some(quote! {
            __deser::__derive::NextField::Other(__key) => {
                let __offset = self.key.offset();
                match self.value_for_key(&__key, __state)? {
                    __deser::__derive::Some(__sink) => __sink,
                    __deser::__derive::None => {
                        #unknown_field;
                        __deser::de::SinkHandle::null()
                    }
                }
            }
        })
    } else {
        None
    };
    let other_key_atom_dispatch = if has_flatten {
        Some(quote! {
            __deser::__derive::NextField::Other(__key) => {
                let __offset = self.key.offset();
                match self.value_for_key(&__key, __state)? {
                    __deser::__derive::Some(__sink) => __deser::__derive::atom_into_handle(__sink, __atom, __state),
                    __deser::__derive::None => {
                        #unknown_field;
                        __deser::__derive::Ok(())
                    }
                }
            }
        })
    } else {
        None
    };
    let other_key_borrowed_atom_dispatch = if has_flatten {
        Some(quote! {
            __deser::__derive::NextField::Other(__key) => {
                let __offset = self.key.offset();
                match self.value_for_key(&__key, __state)? {
                    __deser::__derive::Some(__sink) => __deser::__derive::borrowed_atom_into_handle(__sink, __atom, __state),
                    __deser::__derive::None => {
                        #unknown_field;
                        __deser::__derive::Ok(())
                    }
                }
            }
        })
    } else {
        None
    };
    // the container is validated once it's complete.  The error points at
    // the start of the map.
    let (container_validate, start_field, start_init, start_set) = match container_attrs.validate()
    {
        Some(path) => {
            let validator = validator(path);
            (
                Some(quote! {
                    if let __deser::__derive::Err(__err) = (#validator)(&__value) {
                        return __deser::__derive::Err(match self.start {
                            __deser::__derive::Some(__start) if __err.offset().is_none() => __err.with_offset(__start),
                            _ => __err,
                        });
                    }
                }),
                Some(quote! { start: __deser::__derive::Option<usize>, }),
                Some(quote! { start: __deser::__derive::None, }),
                Some(quote! { self.start = __state.input_range().map(|__range| __range.start); }),
            )
        }
        None => (None, None, None, None),
    };
    // structs with flattened fields need to know if they are flattened
    // themselves (see `unclaimed_keys`), they are not started with `map`
    let (standalone_field, standalone_init, standalone_set) = if has_flatten {
        (
            Some(quote! { standalone: bool, }),
            Some(quote! { standalone: false, }),
            Some(quote! { self.standalone = true; }),
        )
    } else {
        (None, None, None)
    };
    // Structs without flattened fields are updated in place: the fields
    // that are given are updated, the others are kept.  Structs with
    // flattened fields are replaced (the default).
    let update = if has_flatten {
        None
    } else {
        let container_validate = container_attrs.validate().map(|path| {
            let validator = validator(path);
            quote! {
                if let __deser::__derive::Err(__err) = (#validator)(&*self.value) {
                    return __deser::__derive::Err(match self.start {
                        __deser::__derive::Some(__start) if __err.offset().is_none() => __err.with_offset(__start),
                        _ => __err,
                    });
                }
            }
        });
        Some(UpdateSink {
            method: quote! {
                fn deserialize_update(
                    __value: &mut Self,
                ) -> __deser::de::SinkHandle<'_, 'de> {
                    __deser::de::SinkHandle::boxed(__UpdateSink {
                        value: __value,
                        key: __deser::__derive::FieldKeySink::new(__field_index, #retain_unknown),
                        seen: [0; #seen_words],
                        #start_init
                        _marker: __deser::__derive::PhantomData,
                    })
                }
            },
            items: quote! {
                struct __UpdateSink #wrapper_impl_generics #where_clause {
                    value: &'__a mut #ident #ty_generics,
                    key: __deser::__derive::FieldKeySink,
                    seen: [u64; #seen_words],
                    #start_field
                    _marker: __deser::__derive::PhantomData<&'de ()>,
                }

                #[automatically_derived]
                impl #wrapper_impl_generics __deser::de::Sink<'de> for __UpdateSink #wrapper_ty_generics #bounded_where_clause {
                    fn expecting(&self) -> __deser::__derive::StrCow<'_> {
                        __deser::__derive::StrCow::Borrowed(#type_name)
                    }

                    fn map(&mut self, __state: &mut __deser::State)
                        -> __deser::__derive::Result<()>
                    {
                        #start_set
                        __deser::__derive::Ok(())
                    }

                    fn next_key(&mut self, __state: &mut __deser::State)
                        -> __deser::__derive::Result<__deser::de::SinkHandle<'_, 'de>>
                    {
                        self.key.reset();
                        __deser::__derive::Ok(__deser::de::SinkHandle::to(&mut self.key))
                    }

                    fn next_value(&mut self, __state: &mut __deser::State)
                        -> __deser::__derive::Result<__deser::de::SinkHandle<'_, 'de>>
                    {
                        __deser::__derive::Ok(match self.key.next_index(&mut self.seen, __FIELDS, #deny, __state)? {
                            __deser::__derive::Some(__index) => match __index {
                                #(
                                    #update_dispatch
                                )*
                                _ => __deser::de::SinkHandle::null(),
                            },
                            __deser::__derive::None => __deser::de::SinkHandle::null(),
                        })
                    }

                    fn finish(&mut self, __state: &mut __deser::State) -> __deser::__derive::Result<()> {
                        #container_validate
                        __deser::__derive::Ok(())
                    }
                }
            },
        })
    };
    let (update_method, update_items) = match update {
        Some(update) => (Some(update.method), Some(update.items)),
        None => (None, None),
    };
    // keys that flattened values took but did not use are unknown keys too
    let unclaimed_keys = if has_flatten {
        Some(quote! {
            __deser::__derive::unclaimed_keys(self.standalone, #deny, __state)?;
        })
    } else {
        None
    };

    // Takes the next field and dispatches to `$field` for fields and `$other`
    // for other keys (which are only passed on with flattened fields, without
    // them the key sink handles them).
    let dispatch = |field: TokenStream, other: Option<TokenStream>, ignore: TokenStream| {
        if has_flatten {
            quote! {
                match self.key.next_field(&mut self.seen, __FIELDS, __state)? {
                    __deser::__derive::NextField::Field(__index) => #field,
                    #other
                    __deser::__derive::NextField::Ignore => #ignore,
                }
            }
        } else {
            quote! {
                match self.key.next_index(&mut self.seen, __FIELDS, #deny, __state)? {
                    __deser::__derive::Some(__index) => #field,
                    __deser::__derive::None => #ignore,
                }
            }
        }
    };
    let next_value = dispatch(
        quote! { self.__field_sink(__index) },
        other_key_dispatch,
        quote! { __deser::de::SinkHandle::null() },
    );
    let value_atom = dispatch(
        quote! { self.__field_atom(__index, __atom, __state) },
        other_key_atom_dispatch,
        quote! { __deser::__derive::Ok(()) },
    );
    let borrowed_value_atom_dispatch = dispatch(
        quote! { self.__field_borrowed_atom(__index, __atom, __state) },
        other_key_borrowed_atom_dispatch,
        quote! { __deser::__derive::Ok(()) },
    );

    // Without generics no field can borrow from the data, so borrowed atoms
    // are deserialized like other atoms (which keeps the code small).
    let (field_borrowed_atom_fn, borrowed_value_atom) = if input.generics.params.is_empty() {
        (
            None,
            quote! {
                self.__private_value_atom(__atom, __state)
            },
        )
    } else {
        (
            Some(quote! {
                fn __field_borrowed_atom(&mut self, __index: usize, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    match __index {
                        #(
                            #field_borrowed_atoms
                        )*
                        _ => __deser::__derive::Ok(()),
                    }
                }
            }),
            borrowed_value_atom_dispatch,
        )
    };

    Ok(quote! {
        const _: () = {
            fn __field_index(__key: &__deser::__derive::str) -> __deser::__derive::Option<usize> {
                match __key {
                    #(
                        #key_matcher
                    )*
                    _ => __deser::__derive::None,
                }
            }

            const __FIELDS: &[&__deser::__derive::str] = &[#(#field_names),*];

            struct __Sink #wrapper_impl_generics #where_clause {
                slot: &'__a mut __deser::__derive::Option<#ident #ty_generics>,
                key: __deser::__derive::FieldKeySink,
                seen: [u64; #seen_words],
                #standalone_field
                #start_field
                #(
                    #sink_fieldname: #sink_fieldty,
                )*
                #(
                    #flatten_used: bool,
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
                        key: __deser::__derive::FieldKeySink::new(__field_index, #retain_unknown),
                        seen: [0; #seen_words],
                        #standalone_init
                        #start_init
                        #(
                            #sink_fieldname: #sink_defaults,
                        )*
                        #(
                            #flatten_used: false,
                        )*
                        _marker: __deser::__derive::PhantomData,
                    })
                }

                #update_method
            }

            #update_items

            impl #wrapper_impl_generics __Sink #wrapper_ty_generics #bounded_where_clause {
                fn __field_sink(&mut self, __index: usize) -> __deser::de::SinkHandle<'_, 'de> {
                    match __index {
                        #(
                            #field_sinks
                        )*
                        _ => __deser::de::SinkHandle::null(),
                    }
                }

                fn __field_atom(&mut self, __index: usize, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    match __index {
                        #(
                            #field_atoms
                        )*
                        _ => __deser::__derive::Ok(()),
                    }
                }

                #field_borrowed_atom_fn
            }

            #[automatically_derived]
            impl #wrapper_impl_generics __deser::de::Sink<'de> for __Sink #wrapper_ty_generics #bounded_where_clause {
                fn expecting(&self) -> __deser::__derive::StrCow<'_> {
                    __deser::__derive::StrCow::Borrowed(#type_name)
                }

                fn map(&mut self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    #standalone_set
                    #start_set
                    __deser::__derive::Ok(())
                }

                fn next_key(&mut self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::de::SinkHandle<'_, 'de>>
                {
                    self.key.reset();
                    __deser::__derive::Ok(__deser::de::SinkHandle::to(&mut self.key))
                }

                fn next_value(&mut self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::de::SinkHandle<'_, 'de>>
                {
                    __deser::__derive::Ok(#next_value)
                }

                fn __private_key_atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.key.key_atom(__atom, __field_index, __state)
                }

                fn __private_value_atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    #value_atom
                }

                fn __private_borrowed_key_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    // keys are only matched, they do not need to be borrowed
                    self.key.key_atom(__atom, __field_index, __state)
                }

                fn __private_borrowed_value_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    #borrowed_value_atom
                }

                fn value_for_key(&mut self, __key: &str, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::__derive::Option<__deser::de::SinkHandle<'_, 'de>>>
                {
                    if let __deser::__derive::Some(__index) = __field_index(__key) {
                        // the value is deserialized like the value of a key
                        self.key.set_index(__index);
                        return __deser::de::Sink::next_value(self, __state).map(__deser::__derive::Some);
                    }
                    #(
                        if let __deser::__derive::Some(__sink) = self.#flatten_fields.borrow_mut().value_for_key(__key, __state)? {
                            self.#flatten_used = true;
                            return __deser::__derive::Ok(__deser::__derive::Some(__sink));
                        }
                    )*
                    __deser::__derive::Ok(__deser::__derive::None)
                }

                fn finish(&mut self, __state: &mut __deser::State) -> __deser::__derive::Result<()> {
                    #![allow(unused_mut)]
                    // a flattened value that took no key is missing, the
                    // value for missing values of its type is used (`None`
                    // for options).  Types without one are finished (maps
                    // are empty, structs report missing fields).
                    #(
                        let #flatten_initial = if self.#flatten_used {
                            __deser::__derive::None
                        } else {
                            <#flatten_ty as __deser::Deserialize<'de>>::initial_value()
                        };
                        if #flatten_initial.is_none() {
                            self.#flatten_fields.borrow_mut().finish(__state)?;
                        }
                    )*
                    #unclaimed_keys
                    #(
                        let mut #sink_fieldname = self.#sink_fieldname.#field_stage1_default;
                    )*
                    #(
                        if #flatten_initial.is_some() {
                            #flatten_fields = #flatten_initial;
                        }
                    )*
                    #stage2_default
                    let __value = #ident {
                        #(
                            #fieldname: #field_take,
                        )*
                        #(
                            #skipped_name: #skipped_value,
                        )*
                    };
                    #container_validate
                    *self.slot = __deser::__derive::Some(__value);
                    __deser::__derive::Ok(())
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
    if container_attrs.deny_unknown_fields() {
        return Err(syn::Error::new(
            container_attrs.span_of("deny_unknown_fields"),
            "deny_unknown_fields has no effect on enums with only unit variants",
        ));
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
    let mut matcher = Vec::new();
    for (x, var_ident) in attrs.iter().zip(var_idents.iter()) {
        let names = std::iter::once(x.name(&container_attrs))
            .chain(x.aliases(&container_attrs))
            .collect::<Vec<_>>();
        for name in &names {
            if !seen_names.insert(name.clone()) {
                return Err(syn::Error::new_spanned(
                    x.variant(),
                    format!("variant name `{}` used more than once", name.display()),
                ));
            }
        }
        matcher.push(VariantName::tag_arms(
            &names,
            quote! { __deser::__derive::Some(#ident::#var_ident) },
        ));
    }

    if let Some(attrs) = attrs.iter().find(|x| x.default()) {
        return Err(syn::Error::new_spanned(
            attrs.variant(),
            "default variants are only supported for internally and adjacently tagged enums",
        ));
    }

    let unit_validate = container_attrs.validate().map(|path| {
        let validator = validator(path);
        quote! { (#validator)(&value)?; }
    });
    let type_name = container_attrs.container_name();
    let fallback = match attrs.iter().find(|x| x.other()) {
        // all other atoms are unknown tags too
        Some(other) => {
            let var_ident = &other.variant().ident;
            quote! { #ident::#var_ident }
        }
        None => {
            let names = attrs.iter().map(|x| x.name(&container_attrs).str_expr());
            quote! {
                return __deser::__derive::Err(
                    __deser::__derive::unknown_variant_atom(&__atom, &[#(#names),*], #type_name)
                )
            }
        }
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
                    if let __deser::Atom::Ext(_) = __atom {
                        // lowered to the fallback
                        return self.unexpected_atom(__atom, __state);
                    }
                    let __found = __deser::__derive::lookup_atom(&__atom, |__tag| match __tag {
                        #( #matcher )*
                        _ => __deser::__derive::None,
                    });
                    let value = match __found {
                        __deser::__derive::Some(value) => value,
                        __deser::__derive::None => #fallback,
                    };
                    #unit_validate
                    self.slot = __deser::__derive::Some(value);
                    __deser::__derive::Ok(())
                }

                fn expecting(&self) -> __deser::__derive::StrCow<'_> {
                    __deser::__derive::StrCow::Borrowed(#type_name)
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

    let container_attrs = ContainerAttrs::of(input)?;
    if container_attrs.deny_unknown_fields() {
        return Err(syn::Error::new(
            container_attrs.span_of("deny_unknown_fields"),
            "deny_unknown_fields is not supported on newtype structs, they are \
             deserialized like their field",
        ));
    }

    let field_attrs = UnnamedFieldAttrs::of(field)?;
    if field_attrs.tag() {
        return Err(syn::Error::new_spanned(
            field,
            "tag fields are only supported in other variants of enums",
        ));
    }
    let adapter = field_attrs.adapters().de();

    let field_type = &field.ty;
    let make_sink = match adapter {
        Some(adapter) => quote! { __deser::de::OwnedSink::deserialize_as::<#adapter>() },
        None => quote! { __deser::de::OwnedSink::deserialize() },
    };
    // newtype structs update their field, unless it's converted or validated
    let newtype_update = if adapter.is_none() && container_attrs.validate().is_none() {
        Some(quote! {
            fn deserialize_update(__value: &mut Self) -> __deser::de::SinkHandle<'_, 'de> {
                __deser::Deserialize::deserialize_update(&mut __value.0)
            }
        })
    } else {
        None
    };
    let newtype_validate_slot = validate_slot(container_attrs.validate(), quote! { __slot });
    let newtype_validate_self = validate_slot(container_attrs.validate(), quote! { self.slot });
    let atom_into = atom_into(field_type, adapter, quote! { &mut __inner });
    let borrowed_atom_into = borrowed_atom_into(field_type, adapter, quote! { &mut __inner });

    let wrapper_generics = with_lifetime_bound(&de_generics, "'__a");
    let (wrapper_impl_generics, wrapper_ty_generics, _) = wrapper_generics.split_for_impl();
    let bounded_where_clause = where_clause_for_fields(
        &input.generics,
        quote!(__deser::Deserialize<'de>),
        Some(quote!(__deser::__derive::Send)),
        quote!(__deser::adapters::DeserializeAs),
        Some(quote!('de)),
        container_attrs.deserialize_bound(),
        &[BoundField {
            ty: field_type,
            adapter,
            skipped: false,
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

                #newtype_update

                #[inline]
                fn __private_atom_into(
                    __slot: &mut __deser::__derive::Option<Self>,
                    __atom: __deser::Atom,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<()> {
                    let mut __inner = __deser::__derive::None;
                    #atom_into?;
                    *__slot = __inner.map(#ident);
                    #newtype_validate_slot;
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
                    #newtype_validate_slot;
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

                fn __private_key_atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.borrow_mut().__private_key_atom(__atom, __state)
                }

                fn __private_value_atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.borrow_mut().__private_value_atom(__atom, __state)
                }

                fn __private_borrowed_key_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.borrow_mut().__private_borrowed_key_atom(__atom, __state)
                }

                fn __private_borrowed_value_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.borrow_mut().__private_borrowed_value_atom(__atom, __state)
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
                    #newtype_validate_self;
                    __deser::__derive::Ok(())
                }

                fn expecting(&self) -> __deser::__derive::StrCow<'_> {
                    self.sink.borrow().expecting()
                }
            }
        };
    })
}
