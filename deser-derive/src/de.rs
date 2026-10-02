use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

use crate::attr::{
    ContainerAttrs, Direction, FieldAttrs, Name, TypeDefault, VariantName, ident_name,
};
use crate::bound::{
    mentions_any, type_param_names, where_clause_for_fields, with_de_lifetime, with_lifetime_bound,
};
use crate::unnamed::{NewtypeField, UnnamedField, UnnamedStruct};

/// Returns an expression that creates a sink handle for the slot of a field.
///
/// The sink is created inline, which is faster for nested values than a
/// function which exists once per type.
fn field_sink(ty: &syn::Type, adapter: Option<&syn::Type>, slot: TokenStream) -> TokenStream {
    match adapter {
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::Deserialize<'de, #ty>>::deserialize_into(#slot, __state)
        },
        None => quote! { <#ty as __deser::Deserialize<'de>>::deserialize_into(#slot, __state) },
    }
}

/// Returns an expression that deserializes an atom into a slot.
fn atom_into(ty: &syn::Type, adapter: Option<&syn::Type>, slot: TokenStream) -> TokenStream {
    match adapter {
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::Deserialize<'de, #ty>>::__private_atom_into(#slot, __atom, __state)
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
            <#adapter as __deser::Deserialize<'de, #ty>>::__private_borrowed_atom_into(#slot, __atom, __state)
        },
        None => quote! { __deser::__derive::borrowed_atom_into(#slot, __atom, __state) },
    }
}

/// Returns an expression that is `true` if a field collects the values of
/// a repeated key (see `ContainerShape::set_multimap`).
fn field_collects(ty: &syn::Type, adapter: Option<&syn::Type>) -> TokenStream {
    match adapter {
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::Deserialize<'de, #ty>>::__private_collects()
        },
        None => quote! { <#ty as __deser::Deserialize<'de>>::__private_collects() },
    }
}

/// Returns an expression that is `true` if a field wants its value as raw
/// value (see `Deserialize::__private_raw`).
fn field_raw(ty: &syn::Type, adapter: Option<&syn::Type>) -> TokenStream {
    match adapter {
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::Deserialize<'de, #ty>>::__private_raw()
        },
        None => quote! { <#ty as __deser::Deserialize<'de>>::__private_raw() },
    }
}

/// Returns an expression that creates the sink of a value that is added to
/// the collection in the slot of a field.
fn field_collect_into(
    ty: &syn::Type,
    adapter: Option<&syn::Type>,
    slot: TokenStream,
) -> TokenStream {
    match adapter {
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::Deserialize<'de, #ty>>::__private_collect_into(#slot, __state)
        },
        None => {
            quote! { <#ty as __deser::Deserialize<'de>>::__private_collect_into(#slot, __state) }
        }
    }
}

/// Returns an expression that creates the sink of a value that is added to
/// the collection of a field that is updated.
fn field_collect_update(
    ty: &syn::Type,
    adapter: Option<&syn::Type>,
    field: TokenStream,
    first: TokenStream,
) -> TokenStream {
    match adapter {
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::Deserialize<'de, #ty>>::__private_collect_update(#field, #first, __state)
        },
        None => {
            quote! { <#ty as __deser::Deserialize<'de>>::__private_collect_update(#field, #first, __state) }
        }
    }
}

/// Returns an expression for the value of a collection whose key is
/// missing in a multimap.
fn field_collect_empty(ty: &syn::Type, adapter: Option<&syn::Type>) -> TokenStream {
    match adapter {
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::Deserialize<'de, #ty>>::__private_collect_empty()
        },
        None => quote! { <#ty as __deser::Deserialize<'de>>::__private_collect_empty() },
    }
}

/// The generated code for updating structs in place.
struct UpdateSink {
    /// The `deserialize_update` method.
    method: TokenStream,
    /// The sink and its implementation.
    items: TokenStream,
}

pub(crate) fn derive_deserialize(input: &mut syn::DeriveInput) -> syn::Result<TokenStream> {
    // with an adapter that wraps the derived implementation, both are
    // needed
    let forward = match crate::forward::derive_deserialize(input)? {
        Some((rv, false)) => return Ok(rv),
        Some((rv, true)) => Some(rv),
        None => None,
    };
    let derived = derive_deserialize_impl(input)?;
    Ok(quote! {
        #forward
        #derived
    })
}

/// Derives the implementation (as `Deserialize` or `DerivedDeserialize`).
fn derive_deserialize_impl(input: &syn::DeriveInput) -> syn::Result<TokenStream> {
    if let Some(rv) = crate::transparent::derive(input, Direction::Deserialize)? {
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

/// The maximum number of elements of tuples that implement the traits.
pub(crate) const MAX_TUPLE_LEN: usize = 12;

/// Derives a struct with unnamed fields or a unit struct.
///
/// The representation depends on the fields that are deserialized: without
/// fields the struct is null, with one field the value of the field and
/// with more fields a sequence.  Skipped fields are filled in with their
/// default.
fn derive_unnamed_struct(input: &syn::DeriveInput, st: &UnnamedStruct) -> syn::Result<TokenStream> {
    let container_attrs = ContainerAttrs::of(input, Direction::Deserialize)?;
    container_attrs.reject_named_only(st.kind(), Direction::Deserialize)?;
    let mut where_clause = where_clause_for_fields(
        &input.generics,
        quote!(__deser::Deserialize<'de>),
        Some(quote!(__deser::__derive::Send)),
        quote!(__deser::Deserialize),
        Some(quote!('de)),
        container_attrs.deserialize_bound(),
        &st.bound_fields(Direction::Deserialize),
    );
    if container_attrs.deserialize_bound().is_none() {
        where_clause
            .predicates
            .extend(st.default_bounds(&input.generics));
    }
    let remaining = st.remaining(Direction::Deserialize);
    if container_attrs.transparent() && remaining.len() != 1 {
        return Err(crate::transparent::field_count_error(
            input,
            Direction::Deserialize,
        ));
    }
    match remaining[..] {
        [] => derive_unit_struct(input, &container_attrs, st, where_clause),
        [field] => {
            // converts the value of the field into the struct
            let convert = if st.fields.len() == 1 {
                let ident = &input.ident;
                quote! { #ident }
            } else {
                let construct = st.construct(&[quote! { __value }]);
                quote! { |__value| #construct }
            };
            let field = NewtypeField {
                member: syn::Member::Unnamed(field.member.clone()),
                ty: field.ty(),
                adapter: field.attrs.adapters().de(),
                convert,
            };
            derive_newtype_struct(input, &container_attrs, &field, where_clause)
        }
        _ => derive_tuple_struct(input, &container_attrs, st, &remaining, where_clause),
    }
}

/// Derives a tuple struct which is deserialized from a sequence.
///
/// The sequence is deserialized as a tuple of the fields (with the adapters
/// of the fields) which is then converted into the struct.
fn derive_tuple_struct(
    input: &syn::DeriveInput,
    container_attrs: &ContainerAttrs,
    st: &UnnamedStruct,
    fields: &[&UnnamedField],
    where_clause: syn::WhereClause,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let de_generics = with_de_lifetime(&input.generics)?;
    let (impl_generics, _, _) = de_generics.split_for_impl();

    container_attrs.reject_expecting("tuple structs")?;
    if fields.len() > MAX_TUPLE_LEN {
        return Err(syn::Error::new_spanned(
            fields[MAX_TUPLE_LEN].field,
            format!(
                "tuple structs with more than {} fields are not supported",
                MAX_TUPLE_LEN
            ),
        ));
    }

    let mut types = Vec::with_capacity(fields.len());
    let mut bindings = Vec::with_capacity(fields.len());
    let mut values = Vec::with_capacity(fields.len());
    let mut adapters = Vec::with_capacity(fields.len());
    let mut has_adapters = false;
    for (idx, x) in fields.iter().enumerate() {
        let binding = syn::Ident::new(&format!("__f{}", idx), Span::call_site());
        types.push(x.ty());
        values.push(quote! { #binding });
        bindings.push(binding);
        adapters.push(match x.attrs.adapters().de() {
            Some(adapter) => {
                has_adapters = true;
                quote! { #adapter }
            }
            None => quote! { __deser::adapters::Same },
        });
    }
    let tuple_ty = quote! { (#(#types,)*) };
    let pattern = quote! { (#(#bindings,)*) };
    let owned = if has_adapters {
        quote! {
            __deser::de::OwnedSink::<#tuple_ty>::deserialize_as::<(#(#adapters,)*)>(__state)
        }
    } else {
        quote! { __deser::de::OwnedSink::<#tuple_ty>::deserialize(__state) }
    };
    // the tuple is what is expected
    let expecting = if has_adapters {
        quote! {
            <(#(#adapters,)*) as __deser::Deserialize<'de, #tuple_ty>>::expecting()
        }
    } else {
        quote! { <#tuple_ty as __deser::Deserialize<'de>>::expecting() }
    };
    let construct = st.construct(&values);
    let handle = quote! {
        __deser::__derive::mapped(
            __slot,
            #owned,
            |#pattern: #tuple_ty| __deser::__derive::Ok(#construct), __state)
    };

    let de_trait = crate::forward::deserialize_trait(container_attrs);
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #de_trait for #ident #ty_generics #where_clause {
            fn deserialize_into<'__out>(__slot: &'__out mut __deser::__derive::Option<Self>, __state: &mut __deser::State) -> __deser::de::SinkHandle<'__out, 'de> {
                #handle
            }

            fn expecting() -> __deser::__derive::StrCow<'static> {
                #expecting
            }
        }
    })
}

/// Derives a unit struct (or a struct without fields that are deserialized)
/// which is deserialized from null.
fn derive_unit_struct(
    input: &syn::DeriveInput,
    container_attrs: &ContainerAttrs,
    st: &UnnamedStruct,
    where_clause: syn::WhereClause,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let de_generics = with_de_lifetime(&input.generics)?;
    let (impl_generics, _, _) = de_generics.split_for_impl();

    let type_name = container_attrs.expecting();
    let construct = st.construct(&[]);
    let de_trait = crate::forward::deserialize_trait(container_attrs);
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #de_trait for #ident #ty_generics #where_clause {
            fn deserialize_into<'__out>(__slot: &'__out mut __deser::__derive::Option<Self>, __state: &mut __deser::State) -> __deser::de::SinkHandle<'__out, 'de> {
                __deser::__derive::atom_sink(
                    __slot,
                    |__slot: &mut __deser::__derive::Option<Self>,
                     __atom: __deser::Atom<'_>,
                     __state: &mut __deser::State| {
                        <Self as #de_trait>::__private_atom_into(__slot, __atom, __state)
                    },
                    #type_name, __state)
            }

            fn expecting() -> __deser::__derive::StrCow<'static> {
                __deser::__derive::StrCow::Borrowed(#type_name)
            }

            #[inline]
            fn __private_atom_into(
                __slot: &mut __deser::__derive::Option<Self>,
                __atom: __deser::Atom,
                __state: &mut __deser::State,
            ) -> __deser::__derive::Result<()> {
                __deser::__derive::unit_struct(&__atom, #type_name, __state)?;
                *__slot = __deser::__derive::Some(#construct);
                __deser::__derive::Ok(())
            }

            #[inline]
            fn __private_borrowed_atom_into(
                __slot: &mut __deser::__derive::Option<Self>,
                __atom: __deser::Atom<'de>,
                __state: &mut __deser::State,
            ) -> __deser::__derive::Result<()> {
                <Self as #de_trait>::__private_atom_into(__slot, __atom, __state)
            }
        }
    })
}

fn derive_struct(input: &syn::DeriveInput, fields: &syn::FieldsNamed) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let de_generics = with_de_lifetime(&input.generics)?;
    let (impl_generics, _, _) = de_generics.split_for_impl();

    let container_attrs = ContainerAttrs::of(input, Direction::Deserialize)?;
    let type_name = container_attrs.expecting();
    let all_attrs = FieldAttrs::of_all(&fields.named)?;
    // skipped fields are not deserialized, they are filled in when the
    // struct is built
    let mut attrs = Vec::with_capacity(all_attrs.len());
    let mut default_bounds = Vec::new();
    let mut skipped_name = Vec::new();
    let mut skipped_value = Vec::new();
    let mut bound_fields = Vec::with_capacity(all_attrs.len());
    for x in &all_attrs {
        if x.tag() {
            return Err(syn::Error::new_spanned(
                x.field(),
                "tag fields are only supported in other variants of enums",
            ));
        }
        bound_fields.push(x.bound_field(Direction::Deserialize));
        if !x.skip_deserializing() {
            attrs.push(x);
            continue;
        }
        let name = &x.field().ident;
        let ty = &x.field().ty;
        skipped_value.push(match (x.default(), container_attrs.default()) {
            (Some(TypeDefault::Explicit(expr)), _) => expr.clone(),
            (None, Some(TypeDefault::Explicit(expr))) => quote! { (#expr).#name },
            (None, Some(TypeDefault::Implicit)) => quote! {
                <#ident #ty_generics as __deser::__derive::Default>::default().#name
            },
            (Some(TypeDefault::Implicit), _) | (None, None) => {
                default_bounds.push(quote! { #ty: __deser::__derive::Default });
                quote! { <#ty as __deser::__derive::Default>::default() }
            }
        });
        skipped_name.push(name);
    }
    let mut fieldname = Vec::with_capacity(attrs.len());
    let mut sink_fieldname = Vec::with_capacity(attrs.len());
    let mut sink_fieldty = Vec::with_capacity(attrs.len());
    let mut sink_defaults = Vec::with_capacity(attrs.len());
    let mut has_flatten_fields = false;
    for f in &attrs {
        let ty = &f.field().ty;
        fieldname.push(&f.field().ident);
        sink_fieldname.push(syn::Ident::new(
            &format!("field_{}", ident_name(f.field().ident.as_ref().unwrap())),
            Span::call_site(),
        ));
        sink_fieldty.push(if f.flatten() {
            quote! {
                __deser::de::OwnedSink<'de, #ty>
            }
        } else {
            quote! {
                __deser::__derive::Option<#ty>
            }
        });
        sink_defaults.push(if f.flatten() {
            quote! {
                __deser::de::OwnedSink::deserialize(__state)
            }
        } else if f.default().is_some() || f.required() {
            // required fields are missing even if their type has a
            // value for missing fields
            quote! {
                __deser::__derive::None
            }
        } else if let Some(adapter) = f.adapters().de() {
            quote! {
                <#adapter as __deser::Deserialize<'de, #ty>>::initial_value()
            }
        } else {
            quote! {
                <#ty as __deser::Deserialize<'de>>::initial_value()
            }
        });
        has_flatten_fields |= f.flatten();
    }

    let mut seen_names = HashSet::new();
    let mut first_duplicate_name = None;
    let mut key_matcher = Vec::new();
    let mut field_sinks = Vec::new();
    let mut field_atoms = Vec::new();
    let mut field_borrowed_atoms = Vec::new();
    let mut field_collects_arms = Vec::new();
    let mut update_dispatch = Vec::new();
    // the update sinks of structs with flattened fields borrow the fields
    // through a pointer (see `UpdateTarget`)
    let update_through_ptr = has_flatten_fields;
    for (index, x) in attrs.iter().enumerate() {
        if x.flatten() {
            continue;
        }
        let fieldname = &sink_fieldname[index];

        let mut names = vec![x.name(&container_attrs)];
        names.extend(x.aliases(&container_attrs));
        for name in &names {
            if first_duplicate_name.is_none() && !seen_names.insert(name.clone()) {
                first_duplicate_name = Some((name.display(), x.field()));
            }
        }
        let ty = &x.field().ty;
        let adapter = x.adapters().de();
        let collects = field_collects(ty, adapter);
        let collect_into = field_collect_into(ty, adapter, quote! { &mut self.#fieldname });
        let sink = field_sink(ty, adapter, quote! { &mut self.#fieldname });
        let sink = quote! {
            if #collects && __multimap {
                #collect_into
            } else {
                #sink
            }
        };
        let atom = atom_into(ty, adapter, quote! { &mut self.#fieldname });
        let atom = quote! {
            if #collects && __state.is_multimap() {
                __deser::__derive::atom_into_handle(#collect_into, __atom, __state)
            } else {
                #atom
            }
        };
        let borrowed_atom = borrowed_atom_into(ty, adapter, quote! { &mut self.#fieldname });
        let borrowed_atom = quote! {
            if #collects && __state.is_multimap() {
                __deser::__derive::borrowed_atom_into_handle(#collect_into, __atom, __state)
            } else {
                #borrowed_atom
            }
        };
        field_collects_arms.push(quote! {
            #index => #collects,
        });
        let collect_update = field_collect_update(
            ty,
            adapter,
            quote! { __field },
            quote! { __collect == __deser::__derive::Collect::First },
        );
        // in updates, fields are updated in place, the adapters of fields
        // with adapters decide how they are updated (by default they are
        // replaced)
        let field_ident = &x.field().ident;
        let update = match x.adapters().de() {
            None => quote! { __deser::__derive::field_update(__field, __state) },
            Some(adapter) => quote_spanned! { adapter.span()=>
                <#adapter as __deser::Deserialize<'de, #ty>>::deserialize_update(__field, __state)
            },
        };
        let field_ref = if update_through_ptr {
            // SAFETY: the field is only borrowed once at a time, the
            // flattened fields are borrowed separately
            quote! { unsafe { &mut (*self.value.as_ptr()).#field_ident } }
        } else {
            // structs without flattened fields implement `UpdateFields`
            quote! { &mut self.#field_ident }
        };
        update_dispatch.push(quote! {
            #index => {
                let __field = #field_ref;
                if #collects && __collect != __deser::__derive::Collect::No {
                    #collect_update
                } else {
                    #update
                }
            }
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

    // a closure (so that it has the generics and `'de`) that is a function
    // pointer as it does not capture anything
    let collects_fn = quote! {
        |__index: usize| -> bool {
            match __index {
                #(
                    #field_collects_arms
                )*
                _ => false,
            }
        }
    };

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
        quote!(__deser::Deserialize),
        Some(quote!('de)),
        container_attrs.deserialize_bound(),
        &bound_fields,
    );
    // skipped fields of generic types need a default
    if container_attrs.deserialize_bound().is_none() {
        let params = type_param_names(&input.generics);
        for bound in default_bounds {
            if mentions_any(bound.clone(), &params) {
                bounded_where_clause
                    .predicates
                    .push(syn::parse_quote!(#bound));
            }
        }
    }

    // Required fields (without defaults) of structs without flattened
    // fields and container defaults are checked together, which avoids an
    // early return (that drops all fields taken so far) per field.
    let is_checked = |attrs: &FieldAttrs| {
        !has_flatten_fields && container_attrs.default().is_none() && attrs.default().is_none()
    };
    let mut field_stage1_default = Vec::with_capacity(attrs.len());
    let mut checked_fields = Vec::new();
    let mut checked_names = Vec::new();
    let mut field_take = Vec::with_capacity(attrs.len());
    let mut flatten_fields = Vec::new();
    // if a flattened field took a key and the value used if it did not
    let mut flatten_used = Vec::new();
    let mut flatten_initial = Vec::new();
    let mut flatten_ty = Vec::new();
    let mut flatten_ident = Vec::new();
    // the fields that take their value from the container default
    let mut default_sink_name = Vec::new();
    let mut default_original_name = Vec::new();
    // The names of the fields by index for errors about duplicate fields and
    // a bit per field to detect them.
    let mut field_names = Vec::with_capacity(attrs.len());
    // the fields that are required when errors are collected
    let mut required_field = Vec::new();
    let mut required_index = Vec::new();
    let mut required_name = Vec::new();
    for (index, attrs) in attrs.iter().enumerate() {
        let name = &sink_fieldname[index];
        field_stage1_default.push(match attrs.default() {
            Some(TypeDefault::Implicit) => {
                quote! { take().unwrap_or_else(__deser::__derive::Default::default) }
            }
            Some(TypeDefault::Explicit(expr)) => {
                quote! { take().unwrap_or_else(|| #expr) }
            }
            None => quote!(take()),
        });
        if is_checked(attrs) {
            checked_fields.push(name);
            checked_names.push(attrs.name(&container_attrs));
        }
        field_take.push(if attrs.default().is_some() || is_checked(attrs) {
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
                    __deser::__derive::None => return __deser::__derive::Err(__deser::Error::new(__deser::ErrorKind::InvalidState, #error))
                }
            }
        } else if container_attrs.default().is_some() {
            quote! { #name.unwrap() }
        } else {
            let str_name = attrs.name(&container_attrs);
            quote! {
                match #name {
                    __deser::__derive::Some(val) => val,
                    __deser::__derive::None => return __deser::__derive::Err(__deser::__derive::new_missing_field_error(#str_name, __state))
                }
            }
        });
        if attrs.flatten() {
            let ident = attrs.field().ident.as_ref().unwrap();
            flatten_fields.push(name);
            flatten_used.push(syn::Ident::new(
                &format!("used_{}", ident_name(ident)),
                Span::call_site(),
            ));
            flatten_initial.push(syn::Ident::new(
                &format!("initial_{}", ident_name(ident)),
                Span::call_site(),
            ));
            flatten_ty.push(&attrs.field().ty);
            flatten_ident.push(&attrs.field().ident);
            field_names.push(quote! { "" });
        } else {
            let name = attrs.name(&container_attrs);
            field_names.push(quote! { #name });
        }
        if attrs.default().is_none() {
            default_sink_name.push(name);
            default_original_name.push(fieldname[index]);
            if !attrs.flatten() && container_attrs.default().is_none() {
                required_field.push(name);
                required_index.push(index);
                required_name.push(attrs.name(&container_attrs));
            }
        }
    }
    let check_fields = if checked_fields.is_empty() {
        None
    } else {
        Some(quote! {
            let (#(#checked_fields,)*) = match (#(#checked_fields,)*) {
                (#(__deser::__derive::Some(#checked_fields),)*) => (#(#checked_fields,)*),
                (#(#checked_fields,)*) => return __deser::__derive::Err(__deser::__derive::missing_field(
                    &[#(#checked_fields.is_none()),*],
                    &[#(#checked_names),*],
                    __state,
                )),
            };
        })
    };

    let stage2_default = if container_attrs.default().is_some() {
        if !default_sink_name.is_empty() {
            let sink_name = &default_sink_name;
            let original_name = &default_original_name;
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
    // keys that flattened values took but did not use are unknown keys too
    let unclaimed_keys = if has_flatten {
        Some(quote! {
            __deser::__derive::unclaimed_keys(self.standalone, #deny, __state)?;
        })
    } else {
        None
    };

    // Structs are updated in place: the fields that are given are updated,
    // the others are kept.
    let update = if has_flatten {
        // Flattened fields are updated with the keys they take, those that
        // take no key are kept.  Structs with flattened fields keep the
        // sinks of the flattened fields while they update the other fields,
        // so they borrow the fields through a pointer.
        UpdateSink {
            method: quote! {
                fn deserialize_update<'__out>(__value: &'__out mut Self, __state: &mut __deser::State) -> __deser::de::SinkHandle<'__out, 'de> {
                    __deser::de::SinkHandle::arena(__UpdateSink {
                        value: __deser::__derive::UpdateTarget::new(__value),
                        key: __deser::__derive::FieldKeySink::new(__field_index, #collects_fn, #retain_unknown),
                        seen: [0; #seen_words],
                        #standalone_init
                        #(
                            #flatten_fields: __deser::__derive::None,
                            #flatten_used: false,
                        )*
                        _marker: __deser::__derive::PhantomData,
                    }, __state)
                }
            },
            items: quote! {
                struct __UpdateSink #wrapper_impl_generics #where_clause {
                    value: __deser::__derive::UpdateTarget<'__a, #ident #ty_generics>,
                    key: __deser::__derive::FieldKeySink,
                    seen: [u64; #seen_words],
                    #standalone_field
                    #(
                        #flatten_fields: __deser::__derive::Option<__deser::de::SinkHandle<'__a, 'de>>,
                        #flatten_used: bool,
                    )*
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
                        #standalone_set
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
                        __deser::__derive::Ok(match self.key.next_field(&mut self.seen, __FIELDS, __state)? {
                            __deser::__derive::NextField::Field(__index) => {
                                let __collect = self.key.collect(__index, __state);
                                match __index {
                                    #(
                                        #update_dispatch
                                    )*
                                    _ => __deser::de::SinkHandle::null(),
                                }
                            },
                            #other_key_dispatch
                            __deser::__derive::NextField::Ignore => __deser::de::SinkHandle::null(),
                        })
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
                            if self.#flatten_fields.is_none() {
                                // SAFETY: the field is only borrowed by this
                                // sink, the other fields are borrowed
                                // separately
                                let __field: &'__a mut #flatten_ty = unsafe {
                                    &mut (*self.value.as_ptr()).#flatten_ident
                                };
                                self.#flatten_fields = __deser::__derive::Some(
                                    <#flatten_ty as __deser::Deserialize<'de>>::deserialize_update(__field, __state)
                                );
                            }
                            if let __deser::__derive::Some(ref mut __flattened) = self.#flatten_fields {
                                if let __deser::__derive::Some(__sink) = __flattened.value_for_key(__key, __state)? {
                                    self.#flatten_used = true;
                                    return __deser::__derive::Ok(__deser::__derive::Some(__sink));
                                }
                            }
                        )*
                        __deser::__derive::Ok(__deser::__derive::None)
                    }

                    fn finish(&mut self, __state: &mut __deser::State) -> __deser::__derive::Result<()> {
                        // flattened values that took no key are kept
                        #(
                            if self.#flatten_used {
                                if let __deser::__derive::Some(ref mut __flattened) = self.#flatten_fields {
                                    __deser::de::Sink::finish(__flattened, __state)?;
                                }
                            }
                            self.#flatten_fields = __deser::__derive::None;
                        )*
                        #unclaimed_keys
                        __deser::__derive::Ok(())
                    }
                }
            },
        }
    } else {
        // everything but the dispatch to the fields is done by
        // `StructUpdateSink` which exists once for all structs
        UpdateSink {
            method: quote! {
                fn deserialize_update<'__out>(__value: &'__out mut Self, __state: &mut __deser::State) -> __deser::de::SinkHandle<'__out, 'de> {
                    __deser::__derive::StructUpdateSink::handle(__value, &__INFO, __state)
                }
            },
            items: quote! {
                #[automatically_derived]
                impl #impl_generics __deser::__derive::UpdateFields<'de> for #ident #ty_generics #bounded_where_clause {
                    fn collects(&self, __index: usize) -> bool {
                        #collects_fn(__index)
                    }

                    fn update_field(
                        &mut self,
                        __index: usize,
                        __collect: __deser::__derive::Collect,
                        __state: &mut __deser::State,
                    ) -> __deser::de::SinkHandle<'_, 'de> {
                        match __index {
                            #(
                                #update_dispatch
                            )*
                            _ => __deser::de::SinkHandle::null(),
                        }
                    }
                }
            },
        }
    };
    let UpdateSink {
        method: update_method,
        items: update_items,
    } = update;
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
        quote! { self.__field_sink(__index, __state.is_multimap(), __state) },
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

    // Collecting errors (see `State::set_collect_errors`): the errors of
    // the values are collected and reported once the struct is complete,
    // together with the required fields that are missing.  Fields that
    // were seen but have no value failed, they are not missing.  The
    // errors of values that flattened fields took are collected by them.
    let flatten_index = (0..flatten_fields.len()).collect::<Vec<_>>();

    // In a multimap, collections whose key is missing are empty (unless
    // they have defaults or are required).
    let (empty_field, empty_value): (Vec<_>, Vec<_>) = sink_fieldname
        .iter()
        .zip(attrs.iter())
        .filter(|(_, attrs)| {
            !attrs.flatten()
                && attrs.default().is_none()
                && !attrs.required()
                && container_attrs.default().is_none()
        })
        .map(|(name, attrs)| {
            (
                name,
                field_collect_empty(&attrs.field().ty, attrs.adapters().de()),
            )
        })
        .unzip();
    let fill_empty = if empty_field.is_empty() {
        None
    } else {
        Some(quote! {
            if __state.is_multimap() {
                #(
                    if self.#empty_field.is_none() {
                        self.#empty_field = #empty_value;
                    }
                )*
            }
        })
    };
    let (current_field, current_init, current_reset) = if has_flatten {
        (
            Some(quote! { flatten_current: usize, }),
            Some(quote! { flatten_current: usize::MAX, }),
            Some(quote! { self.flatten_current = usize::MAX; }),
        )
    } else {
        (None, None, None)
    };

    let de_trait = crate::forward::deserialize_trait(&container_attrs);
    if !has_flatten {
        let compact = CompactStruct {
            input,
            container_attrs: &container_attrs,
            attrs: &attrs,
            bindings: &sink_fieldname,
            defaults: &sink_defaults,
            skipped_name: &skipped_name,
            skipped_value: &skipped_value,
            key_matcher: &key_matcher,
            field_names: &field_names,
            update_method: &update_method,
            update_items: &update_items,
            bounded_where_clause: &bounded_where_clause,
        };
        return Ok(compact.derive());
    }
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
                #(
                    #sink_fieldname: #sink_fieldty,
                )*
                #(
                    #flatten_used: bool,
                )*
                errors: __deser::de::CollectedErrors,
                #current_field
                _marker: __deser::__derive::PhantomData<&'de ()>,
            }

            #[automatically_derived]
            impl #impl_generics #de_trait for #ident #ty_generics #bounded_where_clause {
                fn deserialize_into<'__out>(__slot: &'__out mut __deser::__derive::Option<Self>, __state: &mut __deser::State) -> __deser::de::SinkHandle<'__out, 'de> {
                    __deser::de::SinkHandle::arena(__Sink {
                        slot: __slot,
                        key: __deser::__derive::FieldKeySink::new(__field_index, #collects_fn, #retain_unknown),
                        seen: [0; #seen_words],
                        #standalone_init
                        #(
                            #sink_fieldname: #sink_defaults,
                        )*
                        #(
                            #flatten_used: false,
                        )*
                        errors: __deser::de::CollectedErrors::new(),
                        #current_init
                        _marker: __deser::__derive::PhantomData,
                    }, __state)
                }

                fn expecting() -> __deser::__derive::StrCow<'static> {
                    __deser::__derive::StrCow::Borrowed(#type_name)
                }

                #update_method
            }

            #update_items

            impl #wrapper_impl_generics __Sink #wrapper_ty_generics #bounded_where_clause {
                fn __field_sink(
                    &mut self,
                    __index: usize,
                    __multimap: bool,
                    __state: &mut __deser::State,
                ) -> __deser::de::SinkHandle<'_, 'de> {
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

                /// Returns the errors that were collected, together with the
                /// errors of the flattened fields and the missing fields.
                #[cold]
                #[inline(never)]
                fn __collected_errors(&mut self, __state: &mut __deser::State) -> __deser::Error {
                    #(
                        if self.#flatten_used || <#flatten_ty as __deser::Deserialize<'de>>::initial_value().is_none() {
                            if let __deser::__derive::Err(__err) = self.#flatten_fields.get_mut().finish(__state) {
                                self.errors.push(__err, __state);
                            }
                        }
                    )*
                    __deser::__derive::collected_errors(
                        &mut self.errors,
                        &self.seen,
                        &[#(self.#required_field.is_none()),*],
                        &[#(#required_index),*],
                        &[#(#required_name),*],
                        __state,
                    )
                }
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
                    __deser::__derive::Ok(())
                }

                fn next_key(&mut self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::de::SinkHandle<'_, 'de>>
                {
                    #current_reset
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
                    #current_reset
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
                    #current_reset
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
                    #current_reset
                    if let __deser::__derive::Some(__index) = __field_index(__key) {
                        // the value is deserialized like the value of a key
                        self.key.set_index(__index);
                        return __deser::de::Sink::next_value(self, __state).map(__deser::__derive::Some);
                    }
                    #(
                        if let __deser::__derive::Some(__sink) = self.#flatten_fields.get_mut().value_for_key(__key, __state)? {
                            self.#flatten_used = true;
                            self.flatten_current = #flatten_index;
                            return __deser::__derive::Ok(__deser::__derive::Some(__sink));
                        }
                    )*
                    __deser::__derive::Ok(__deser::__derive::None)
                }

                fn recover(&mut self, __err: __deser::Error, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    // the values that flattened fields took are theirs
                    #(
                        if self.flatten_current == #flatten_index {
                            return self.#flatten_fields.get_mut().recover(__err, __state);
                        }
                    )*
                    self.errors.collect(__err, __state)
                }

                fn finish(&mut self, __state: &mut __deser::State) -> __deser::__derive::Result<()> {
                    #![allow(unused_mut)]
                    #fill_empty
                    if !self.errors.is_empty() {
                        return __deser::__derive::Err(self.__collected_errors(__state));
                    }
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
                            self.#flatten_fields.get_mut().finish(__state)?;
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
                    #check_fields
                    let __value = #ident {
                        #(
                            #fieldname: #field_take,
                        )*
                        #(
                            #skipped_name: #skipped_value,
                        )*
                    };
                    *self.slot = __deser::__derive::Some(__value);
                    __deser::__derive::Ok(())
                }
            }

        };
    })
}

/// A struct without flattened fields.
///
/// Everything that does not depend on the types of the fields is done by
/// `StructSink` which exists once for all structs.  The derive only
/// implements `StructFields` for a struct that holds the slot and the
/// values of the fields.
struct CompactStruct<'a> {
    input: &'a syn::DeriveInput,
    container_attrs: &'a ContainerAttrs<'a>,
    attrs: &'a [&'a FieldAttrs<'a>],
    /// The names of the bindings of the values of the fields.
    bindings: &'a [syn::Ident],
    /// The initial values of the fields.
    defaults: &'a [TokenStream],
    skipped_name: &'a [&'a Option<syn::Ident>],
    skipped_value: &'a [TokenStream],
    key_matcher: &'a [TokenStream],
    field_names: &'a [TokenStream],
    update_method: &'a TokenStream,
    update_items: &'a TokenStream,
    bounded_where_clause: &'a syn::WhereClause,
}

impl CompactStruct<'_> {
    fn derive(&self) -> TokenStream {
        let input = self.input;
        let container_attrs = self.container_attrs;
        let ident = &input.ident;
        let (_, ty_generics, where_clause) = input.generics.split_for_impl();
        let de_generics = with_de_lifetime(&input.generics).unwrap();
        let (impl_generics, _, _) = de_generics.split_for_impl();
        let wrapper_generics = with_lifetime_bound(&de_generics, "'__a");
        let (wrapper_impl_generics, wrapper_ty_generics, _) = wrapper_generics.split_for_impl();
        let bounded_where_clause = self.bounded_where_clause;
        let type_name = container_attrs.expecting();
        let deny = container_attrs.deny_unknown_fields();
        let de_trait = crate::forward::deserialize_trait(container_attrs);

        // Without generics no field can borrow from the data, so borrowed
        // atoms are deserialized like other atoms.
        let borrows = !input.generics.params.is_empty();
        // Required fields (without defaults) are matched as `Some`, the
        // struct is only built if all of them have a value and no errors
        // were collected.  Otherwise `StructFinish::missing` creates the
        // error.
        let has_container_default = container_attrs.default().is_some();
        let len = self.attrs.len();
        let mut index = Vec::with_capacity(len);
        let mut slot_types = Vec::with_capacity(len);
        let mut empty_fields = Vec::new();
        let mut empty_values = Vec::new();
        let mut patterns = Vec::with_capacity(len);
        let mut takes = Vec::with_capacity(len);
        let mut fail_bindings = Vec::with_capacity(len);
        let mut missing = Vec::with_capacity(len);
        let mut fieldname = Vec::with_capacity(len);
        // the fields that take the value of the default of the container
        let mut default_binding = Vec::new();
        let mut default_name = Vec::new();
        // the fields that want raw values as bits, the last bit stands for
        // all fields from the 64th on
        let mut raw_bits = Vec::new();
        let mut raw_overflow = Vec::new();
        // the fields that collect as bits, like the raw values
        let mut collect_bits = Vec::new();
        let mut collect_overflow = Vec::new();
        for (idx, x) in self.attrs.iter().enumerate() {
            let binding = &self.bindings[idx];
            let ty = &x.field().ty;
            let adapter = x.adapters().de();
            let member = syn::Index::from(idx);
            let raw = field_raw(ty, adapter);
            let collects = field_collects(ty, adapter);
            if idx < 63 {
                raw_bits.push(quote! { ((#raw.is_some() as u64) << #idx) });
                collect_bits.push(quote! { ((#collects as u64) << #idx) });
            } else {
                raw_overflow.push(quote! { #raw.is_some() });
                collect_overflow.push(collects);
            }
            // what depends on the type of the field is done by its slot
            // (`FieldSlot`), which exists once per type and adapter
            slot_types.push(match adapter {
                Some(adapter) => quote_spanned! { adapter.span()=>
                    __deser::__derive::FieldValue<#ty, #adapter>
                },
                None => quote! { __deser::__derive::FieldValue<#ty> },
            });
            if !has_container_default && x.default().is_none() && !x.required() {
                empty_fields.push(member.clone());
                empty_values.push(field_collect_empty(ty, adapter));
            }
            index.push(member);
            fieldname.push(&x.field().ident);
            if x.default().is_none() {
                default_binding.push(binding);
                default_name.push(&x.field().ident);
            }
            if !has_container_default && x.default().is_none() {
                patterns.push(quote! { __deser::__derive::Some(#binding) });
                takes.push(quote! { #binding });
                fail_bindings.push(quote! { #binding });
                missing.push(quote! { #binding.is_none() });
                continue;
            }
            patterns.push(if has_container_default {
                quote! { mut #binding }
            } else {
                quote! { #binding }
            });
            takes.push(match x.default() {
                Some(TypeDefault::Implicit) => {
                    quote! { #binding.unwrap_or_else(__deser::__derive::Default::default) }
                }
                Some(TypeDefault::Explicit(expr)) => {
                    quote! { #binding.unwrap_or_else(|| #expr) }
                }
                None => quote! { #binding.unwrap() },
            });
            fail_bindings.push(quote! { _ });
            missing.push(quote! { false });
        }
        let defaults = self.defaults;
        // the last field is the fallback so that the match needs no arm
        // that panics (the index is always the one of a field)
        let field_fn_body = match index.split_last() {
            Some((last, rest)) => quote! {
                match __index {
                    #(#rest => &mut self.values.#rest,)*
                    _ => &mut self.values.#last,
                }
            },
            None => quote! { __deser::__derive::no_field_slot() },
        };

        // with a container default the fields without a default of their
        // own take the value of the default of the container
        let container_default = match container_attrs.default() {
            Some(default) if !default_binding.is_empty() => {
                let type_default = match default {
                    TypeDefault::Implicit => quote! {
                        <#ident #ty_generics as __deser::__derive::Default>::default()
                    },
                    TypeDefault::Explicit(expr) => expr.clone(),
                };
                let binding = &default_binding;
                let name = &default_name;
                Some(quote! {
                    if #(#binding.is_none())||* {
                        let __default = #type_default;
                        #(
                            #binding = #binding.or(__deser::__derive::Some(__default.#name));
                        )*
                    }
                })
            }
            _ => None,
        };
        let skipped_name = self.skipped_name;
        let skipped_value = self.skipped_value;
        let key_matcher = self.key_matcher;
        let field_names = self.field_names;
        let update_method = self.update_method;
        let update_items = self.update_items;

        quote! {
            const _: () = {
                fn __field_index(__key: &__deser::__derive::str) -> __deser::__derive::Option<usize> {
                    match __key {
                        #(#key_matcher)*
                        _ => __deser::__derive::None,
                    }
                }

                const __FIELDS: &[&__deser::__derive::str] = &[#(#field_names),*];

                const __INFO: __deser::__derive::StructInfo = __deser::__derive::StructInfo {
                    name: #type_name,
                    fields: __FIELDS,
                    lookup: __field_index,
                    deny: #deny,
                    borrows: #borrows,
                };

                struct __Fields #wrapper_impl_generics #where_clause {
                    slot: &'__a mut __deser::__derive::Option<#ident #ty_generics>,
                    values: (#(#slot_types,)*),
                    _marker: __deser::__derive::PhantomData<&'de ()>,
                }

                #[automatically_derived]
                impl #impl_generics #de_trait for #ident #ty_generics #bounded_where_clause {
                    fn deserialize_into<'__out>(__slot: &'__out mut __deser::__derive::Option<Self>, __state: &mut __deser::State) -> __deser::de::SinkHandle<'__out, 'de> {
                        __deser::__derive::StructSink::handle(
                            __Fields {
                                slot: __slot,
                                values: (#(__deser::__derive::FieldValue::new(#defaults),)*),
                                _marker: __deser::__derive::PhantomData,
                            },
                            &__INFO, __state)
                    }

                    fn expecting() -> __deser::__derive::StrCow<'static> {
                        __deser::__derive::StrCow::Borrowed(#type_name)
                    }

                    #update_method
                }

                #update_items

                #[automatically_derived]
                impl #wrapper_impl_generics __deser::__derive::StructFields<'de> for __Fields #wrapper_ty_generics #bounded_where_clause {
                    fn field(&mut self, __index: usize) -> &mut dyn __deser::__derive::FieldSlot<'de> {
                        #field_fn_body
                    }

                    #[inline(always)]
                    fn raw_fields() -> u64 {
                        0 #(| #raw_bits)* | (((false #(|| #raw_overflow)*) as u64) << 63)
                    }

                    #[inline(always)]
                    fn collect_fields() -> u64 {
                        0 #(| #collect_bits)* | (((false #(|| #collect_overflow)*) as u64) << 63)
                    }

                    fn finish(
                        &mut self,
                        __finish: &mut __deser::__derive::StructFinish<'_>,
                        __state: &mut __deser::State,
                    ) -> __deser::__derive::Result<()> {
                        if __state.is_multimap() {
                            #(
                                if self.values.#empty_fields.is_none() {
                                    self.values.#empty_fields.set(#empty_values);
                                }
                            )*
                        }
                        match (#(self.values.#index.take(),)*) {
                            (#(#patterns,)*) if __finish.ok() => {
                                #container_default
                                *self.slot = __deser::__derive::Some(#ident {
                                    #(#fieldname: #takes,)*
                                    #(#skipped_name: #skipped_value,)*
                                });
                                __deser::__derive::Ok(())
                            }
                            (#(#fail_bindings,)*) => __deser::__derive::Err(
                                __finish.missing(&[#(#missing),*], __state),
                            ),
                        }
                    }
                }
            };
        }
    }
}

pub(crate) fn derive_enum(
    input: &syn::DeriveInput,
    enumeration: &syn::DataEnum,
) -> syn::Result<TokenStream> {
    let container_attrs = ContainerAttrs::of(input, Direction::Deserialize)?;
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
    let (var_idents, attrs) = crate::enums::unit_variants(enumeration)?;

    let mut seen_names = HashSet::new();
    let mut matcher = Vec::new();
    let mut variant_arms = Vec::new();
    // the names of the variants that can be deserialized
    let mut names = Vec::new();
    let mut deny_unknown_fields = None;
    let mut default = None;
    let mut other = None;
    let mut other_count = 0;
    for (index, x) in attrs.iter().enumerate() {
        let var_ident = var_idents[index];
        if deny_unknown_fields.is_none() && x.deny_unknown_fields() {
            deny_unknown_fields = Some(x);
        }
        if default.is_none() && x.default() {
            default = Some(x);
        }
        if x.other() {
            other.get_or_insert(index);
            other_count += 1;
        }
        let mut variant_names = vec![x.name(&container_attrs)];
        variant_names.extend(x.aliases(&container_attrs));
        for name in &variant_names {
            if !seen_names.insert(name.clone()) {
                return Err(syn::Error::new_spanned(
                    x.variant(),
                    format!("variant name `{}` used more than once", name.display()),
                ));
            }
        }
        // the names are matched to the index of the variant by a function
        // so that the lookup (which is not generic) exists once, skipped
        // variants are unknown variants
        if x.skip_deserializing() {
            continue;
        }
        names.push(variant_names[0].str_expr());
        matcher.push(VariantName::tag_arms(
            &variant_names,
            quote! { __deser::__derive::Some(#index) },
        ));
        variant_arms.push((index, var_ident));
    }

    if let Some(attrs) = deny_unknown_fields {
        return Err(syn::Error::new_spanned(
            attrs.variant(),
            "deny_unknown_fields on variants only has an effect on struct variants \
             (and unit variants of internally tagged enums)",
        ));
    }
    if let Some(attrs) = default {
        return Err(syn::Error::new_spanned(
            attrs.variant(),
            "default variants are only supported for internally and adjacently tagged enums",
        ));
    }

    let type_name = container_attrs.expecting();
    // atoms that are not the name of a variant are the other variant (if
    // there is one), or an error with the names
    let other = match other {
        Some(index) => quote! { __deser::__derive::Some(#index) },
        None => quote! { __deser::__derive::None },
    };
    if other_count > 1 {
        return Err(syn::Error::new(
            Span::call_site(),
            "only one variant can be marked as other",
        ));
    }

    // The variants are set by their index.  The last one is the fallback so
    // that the match needs no arm that panics.
    let variant = match variant_arms.split_last() {
        Some(((_, last), rest)) => {
            let mut arms = Vec::with_capacity(rest.len());
            for (index, var_ident) in rest {
                arms.push(quote! { #index => #ident::#var_ident, });
            }
            quote! {
                match __index {
                    #(#arms)*
                    _ => #ident::#last,
                }
            }
        }
        None => quote! { __deser::__derive::unreachable!() },
    };

    // Unit enums only generate the lookup of the names and two setters, the
    // sink (`unit_enum_sink`) exists once for all types.
    let de_trait = crate::forward::deserialize_trait(&container_attrs);
    Ok(quote! {
        const _: () = {
            fn __lookup(__tag: __deser::__derive::Tag<'_>) -> __deser::__derive::Option<usize> {
                match __tag {
                    #( #matcher )*
                    _ => __deser::__derive::None,
                }
            }

            const __UNIT: __deser::__derive::UnitEnum = __deser::__derive::UnitEnum {
                lookup: __lookup,
                names: &[#(#names),*],
                expecting: #type_name,
                other: #other,
            };

            fn __set_slot(__slot: &mut __deser::__derive::Option<#ident>, __index: usize) {
                *__slot = __deser::__derive::Some(#variant);
            }

            fn __set_value(__value: &mut #ident, __index: usize) {
                *__value = #variant;
            }

            #[automatically_derived]
            impl<'de> #de_trait for #ident {
                fn deserialize_into<'__out>(
                    __slot: &'__out mut __deser::__derive::Option<Self>,
                    __state: &mut __deser::State,
                ) -> __deser::de::SinkHandle<'__out, 'de> {
                    __deser::__derive::unit_enum_sink(__slot, __set_slot, &__UNIT, __state)
                }

                fn expecting() -> __deser::__derive::StrCow<'static> {
                    __deser::__derive::StrCow::Borrowed(#type_name)
                }

                fn deserialize_update<'__out>(
                    __value: &'__out mut Self,
                    __state: &mut __deser::State,
                ) -> __deser::de::SinkHandle<'__out, 'de> {
                    __deser::__derive::unit_enum_sink(__value, __set_value, &__UNIT, __state)
                }

                #[inline]
                fn __private_atom_into(
                    __slot: &mut __deser::__derive::Option<Self>,
                    __atom: __deser::Atom,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<()> {
                    __deser::__derive::unit_enum_atom_into(__slot, __set_slot, __atom, &__UNIT)
                }

                #[inline]
                fn __private_borrowed_atom_into(
                    __slot: &mut __deser::__derive::Option<Self>,
                    __atom: __deser::Atom<'de>,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<()> {
                    __deser::__derive::unit_enum_atom_into(__slot, __set_slot, __atom, &__UNIT)
                }
            }
        };
    })
}

/// Derives a newtype struct (or a struct with one field that is
/// deserialized) which is deserialized like the field.
pub(crate) fn derive_newtype_struct(
    input: &syn::DeriveInput,
    container_attrs: &ContainerAttrs,
    field: &NewtypeField,
    bounded_where_clause: syn::WhereClause,
) -> syn::Result<TokenStream> {
    container_attrs.reject_expecting("newtype and transparent structs")?;
    let ident = &input.ident;
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let de_generics = with_de_lifetime(&input.generics)?;
    let (impl_generics, _, _) = de_generics.split_for_impl();

    let adapter = field.adapter;
    let member = &field.member;
    let field_type = field.ty;
    let convert = &field.convert;
    let make_sink = match adapter {
        Some(adapter) => quote! { __deser::de::OwnedSink::deserialize_as::<#adapter>(__state) },
        None => quote! { __deser::de::OwnedSink::deserialize(__state) },
    };
    // newtype structs update their field (the adapter decides how)
    let newtype_update = match adapter {
        None => quote! {
            fn deserialize_update<'__out>(__value: &'__out mut Self, __state: &mut __deser::State) -> __deser::de::SinkHandle<'__out, 'de> {
                <#field_type as __deser::Deserialize<'de>>::deserialize_update(&mut __value.#member, __state)
            }
        },
        Some(adapter) => quote_spanned! { adapter.span()=>
            fn deserialize_update<'__out>(__value: &'__out mut Self, __state: &mut __deser::State) -> __deser::de::SinkHandle<'__out, 'de> {
                <#adapter as __deser::Deserialize<'de, #field_type>>::deserialize_update(
                    &mut __value.#member, __state)
            }
        },
    };
    let atom_into = atom_into(field_type, adapter, quote! { &mut __inner });
    let borrowed_atom_into = borrowed_atom_into(field_type, adapter, quote! { &mut __inner });
    // the sink passes everything on to the sink of the field
    let expecting = match adapter {
        None => quote! { <#field_type as __deser::Deserialize<'de>>::expecting() },
        Some(adapter) => quote_spanned! { adapter.span()=>
            <#adapter as __deser::Deserialize<'de, #field_type>>::expecting()
        },
    };

    let wrapper_generics = with_lifetime_bound(&de_generics, "'__a");
    let (wrapper_impl_generics, wrapper_ty_generics, _) = wrapper_generics.split_for_impl();

    let de_trait = crate::forward::deserialize_trait(container_attrs);
    Ok(quote! {
        const _: () = {
            struct __Sink #wrapper_impl_generics #where_clause {
                slot: &'__a mut __deser::__derive::Option<#ident #ty_generics>,
                sink: __deser::de::OwnedSink<'de, #field_type>,
            }

            #[automatically_derived]
            impl #impl_generics #de_trait for #ident #ty_generics #bounded_where_clause {
                fn deserialize_into<'__out>(__slot: &'__out mut __deser::__derive::Option<Self>, __state: &mut __deser::State) -> __deser::de::SinkHandle<'__out, 'de> {
                    __deser::de::SinkHandle::arena(__Sink {
                        slot: __slot,
                        sink: #make_sink,
                    }, __state)
                }

                fn expecting() -> __deser::__derive::StrCow<'static> {
                    #expecting
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
                    *__slot = __inner.map(#convert);
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
                    *__slot = __inner.map(#convert);
                    __deser::__derive::Ok(())
                }
            }

            impl #wrapper_impl_generics __deser::de::Sink<'de> for __Sink #wrapper_ty_generics #bounded_where_clause {
                fn atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.get_mut().atom(__atom, __state)
                }

                fn borrowed_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.get_mut().borrowed_atom(__atom, __state)
                }

                fn map(&mut self, __state: &mut __deser::State) -> __deser::__derive::Result<()> {
                    self.sink.get_mut().map(__state)
                }

                fn seq(&mut self, __state: &mut __deser::State) -> __deser::__derive::Result<()>  {
                    self.sink.get_mut().seq(__state)
                }

                fn next_key(&mut self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::de::SinkHandle<'_, 'de>>
                {
                    self.sink.get_mut().next_key(__state)
                }

                fn next_value(&mut self, __state: &mut __deser::State)
                    -> __deser::__derive::Result<__deser::de::SinkHandle<'_, 'de>>
                {
                    self.sink.get_mut().next_value(__state)
                }

                fn __private_key_atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.get_mut().__private_key_atom(__atom, __state)
                }

                fn __private_value_atom(&mut self, __atom: __deser::Atom, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.get_mut().__private_value_atom(__atom, __state)
                }

                fn __private_borrowed_key_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.get_mut().__private_borrowed_key_atom(__atom, __state)
                }

                fn __private_borrowed_value_atom(&mut self, __atom: __deser::Atom<'de>, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.get_mut().__private_borrowed_value_atom(__atom, __state)
                }

                fn value_for_key(
                    &mut self,
                    __key: &str,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<__deser::__derive::Option<__deser::de::SinkHandle<'_, 'de>>> {
                    self.sink.get_mut().value_for_key(__key, __state)
                }

                fn recover(&mut self, __err: __deser::Error, __state: &mut __deser::State)
                    -> __deser::__derive::Result<()>
                {
                    self.sink.get_mut().recover(__err, __state)
                }

                fn finish(&mut self, __state: &mut __deser::State) -> __deser::__derive::Result<()> {
                    self.sink.get_mut().finish(__state)?;
                    *self.slot = self.sink.take().map(#convert);
                    __deser::__derive::Ok(())
                }

                fn expecting(&self) -> __deser::__derive::StrCow<'_> {
                    self.sink.get().expecting()
                }
            }
        };
    })
}
