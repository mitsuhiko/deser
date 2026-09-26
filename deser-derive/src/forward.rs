//! Derives for types with adapters on the container.
//!
//! `#[deser(as = A)]` on a struct, enum or union makes its implementations
//! forward to the adapter, the fields and variants are not used.
//! `serialize_as` and `deserialize_as` do this for one direction, the other
//! one is derived as usual.
//!
//! Attributes which only affect the directions that forward to the adapter
//! would be silently ignored, they are rejected instead.
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

use crate::attr::{
    ContainerAttrs, Direction, EnumVariantAttrs, FieldAttrs, SeenAttr, UnnamedFieldAttrs,
};
use crate::bound::with_de_lifetime;

/// Where an attribute was used.
#[derive(Copy, Clone)]
enum Level {
    Container,
    Variant,
    Field,
}

/// Returns the directions an attribute affects as `(serialize, deserialize)`.
///
/// Returns `None` for attributes that are always allowed.
fn affected_directions(level: Level, name: &str) -> Option<(bool, bool)> {
    const SER: (bool, bool) = (true, false);
    const DE: (bool, bool) = (false, true);
    const BOTH: (bool, bool) = (true, true);
    Some(match (level, name) {
        // the adapters themselves, the bounds, the crate path and the type
        // name (which describes the type) apply to forwarding as well
        (
            Level::Container,
            "as" | "serialize_as" | "deserialize_as" | "bound" | "serialize_bound"
            | "deserialize_bound" | "crate" | "rename",
        ) => return None,
        (Level::Container, "default") => DE,
        (Level::Container, "skip_serializing_optionals") => SER,
        (Level::Container, _) => BOTH,
        (Level::Variant, "alias" | "default") => DE,
        (Level::Variant, _) => BOTH,
        (Level::Field, "alias" | "default" | "deserialize_as") => DE,
        (Level::Field, "skip_serializing_if" | "serialize_as") => SER,
        (Level::Field, _) => BOTH,
    })
}

/// Rejects the attributes that have no effect because the container
/// forwards to its adapter for the given direction.
///
/// An attribute has no effect if all directions it affects forward to an
/// adapter.  Every derive only reports the attributes that affect its own
/// direction.
fn check_attrs(
    input: &syn::DeriveInput,
    container_attrs: &ContainerAttrs,
    direction: Direction,
) -> syn::Result<()> {
    let adapters = container_attrs.adapters();
    let forwards = (adapters.ser().is_some(), adapters.de().is_some());
    let mut errors: Option<syn::Error> = None;
    for (level, seen) in used_attrs(input)? {
        for attr in &seen {
            let Some((ser, de)) = affected_directions(level, &attr.name) else {
                continue;
            };
            let ignored = (!ser || forwards.0) && (!de || forwards.1);
            let relevant = match direction {
                Direction::Serialize => ser,
                Direction::Deserialize => de,
            };
            if !ignored || !relevant {
                continue;
            }
            let what = match (ser, de) {
                (true, true) => "serialized and deserialized",
                (true, false) => "serialized",
                _ => "deserialized",
            };
            let target = match level {
                Level::Container => "",
                Level::Variant => " (the variants are not used)",
                Level::Field => " (the fields are not used)",
            };
            let error = syn::Error::new(
                attr.span,
                format!(
                    "`{}` has no effect as the type is {} with an adapter{}",
                    attr.name, what, target
                ),
            );
            match errors {
                Some(ref mut errors) => errors.combine(error),
                None => errors = Some(error),
            }
        }
    }

    match errors {
        Some(errors) => Err(errors),
        None => Ok(()),
    }
}

/// Returns the attributes used on the container, its variants and fields.
fn used_attrs(input: &syn::DeriveInput) -> syn::Result<Vec<(Level, Vec<SeenAttr>)>> {
    fn push_fields(fields: &syn::Fields, rv: &mut Vec<(Level, Vec<SeenAttr>)>) -> syn::Result<()> {
        for field in fields {
            let seen = match field.ident {
                Some(_) => FieldAttrs::of(field)?.into_seen(),
                None => UnnamedFieldAttrs::of(field)?.into_seen(),
            };
            rv.push((Level::Field, seen));
        }
        Ok(())
    }

    let mut rv = vec![(Level::Container, ContainerAttrs::of(input)?.into_seen())];
    match input.data {
        syn::Data::Struct(ref data) => push_fields(&data.fields, &mut rv)?,
        syn::Data::Enum(ref data) => {
            for variant in &data.variants {
                rv.push((Level::Variant, EnumVariantAttrs::of(variant)?.into_seen()));
                push_fields(&variant.fields, &mut rv)?;
            }
        }
        syn::Data::Union(ref data) => {
            push_fields(&syn::Fields::Named(data.fields.clone()), &mut rv)?;
        }
    }
    Ok(rv)
}

/// Returns the where clause for a forwarding impl.
///
/// Custom bounds replace the inferred ones which require the adapter to
/// support the type and the type to satisfy the supertrait (`Sync` or
/// `Send`).  The inferred bounds are only needed for types with type
/// parameters.  They are not added for other types as they would make the
/// trait solver recurse forever for recursive types (the adapter of `Node`
/// in `FromInto<Vec<Node>>` requires `Node` to implement the trait which
/// is implemented if the adapter supports `Node`).
fn where_clause(
    generics: &syn::Generics,
    custom: Option<&[syn::WherePredicate]>,
    inferred: Vec<syn::WherePredicate>,
) -> syn::WhereClause {
    let predicates = match custom {
        Some(custom) => custom.to_vec(),
        None if generics.type_params().next().is_some() => inferred,
        None => Vec::new(),
    };
    let mut generics = generics.clone();
    generics.make_where_clause().predicates.extend(predicates);
    generics.where_clause.unwrap()
}

/// Derives `Serialize` if the container has an adapter for it.
pub fn derive_serialize(input: &syn::DeriveInput) -> syn::Result<Option<TokenStream>> {
    let container_attrs = ContainerAttrs::of(input)?;
    let Some(adapter) = container_attrs.adapters().ser() else {
        return Ok(None);
    };
    check_attrs(input, &container_attrs, Direction::Serialize)?;

    let ident = &input.ident;
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();
    let type_name = container_attrs.container_name();
    let where_clause = where_clause(
        &input.generics,
        container_attrs.serialize_bound(),
        vec![
            // spanned so that errors about unsupported types point to the adapter
            syn::parse_quote_spanned! { adapter.span()=>
                #adapter: __deser::adapters::SerializeAs<#ident #ty_generics>
            },
            syn::parse_quote! { #ident #ty_generics: __deser::__derive::Sync },
        ],
    );
    let adapter = quote_spanned! { adapter.span()=>
        <#adapter as __deser::adapters::SerializeAs<Self>>
    };

    Ok(Some(quote! {
        #[automatically_derived]
        impl #impl_generics __deser::Serialize for #ident #ty_generics #where_clause {
            #[inline]
            fn serialize(&self, __state: &mut __deser::State)
                -> __deser::__derive::Result<__deser::ser::Chunk<'_>>
            {
                #adapter::serialize_as(self, __state)
            }

            #[inline]
            fn finish(&self, __state: &mut __deser::State) -> __deser::__derive::Result<()> {
                #adapter::finish_as(self, __state)
            }

            #[inline]
            fn is_optional(&self) -> bool {
                #adapter::is_optional_as(self)
            }

            #[inline]
            fn container_shape(&self) -> __deser::ContainerShape {
                #adapter::container_shape_as(self)
            }

            fn describe(&self, __d: &mut dyn __deser::ser::Describe) {
                __d.newtype(#type_name);
                #adapter::describe_as(self, __d)
            }

            #[inline]
            fn __private_begin(&self, __state: &mut __deser::State)
                -> __deser::__derive::Result<__deser::__derive::Begin<'_>>
            {
                #adapter::__private_begin_as(self, __state)
            }

            #[inline]
            fn __private_slice_as_bytes(__values: &[Self])
                -> __deser::__derive::Option<__deser::__derive::Cow<'_, [__deser::__derive::u8]>>
            {
                #adapter::__private_slice_as_bytes_as(__values)
            }
        }
    }))
}

/// Derives `Deserialize` if the container has an adapter for it.
pub fn derive_deserialize(input: &syn::DeriveInput) -> syn::Result<Option<TokenStream>> {
    let container_attrs = ContainerAttrs::of(input)?;
    let Some(adapter) = container_attrs.adapters().de() else {
        return Ok(None);
    };
    check_attrs(input, &container_attrs, Direction::Deserialize)?;

    let ident = &input.ident;
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let de_generics = with_de_lifetime(&input.generics)?;
    let (impl_generics, _, _) = de_generics.split_for_impl();
    let where_clause = where_clause(
        &input.generics,
        container_attrs.deserialize_bound(),
        vec![
            syn::parse_quote_spanned! { adapter.span()=>
                #adapter: __deser::adapters::DeserializeAs<'de, #ident #ty_generics>
            },
            syn::parse_quote! { #ident #ty_generics: __deser::__derive::Send },
        ],
    );
    let adapter = quote_spanned! { adapter.span()=>
        <#adapter as __deser::adapters::DeserializeAs<'de, Self>>
    };

    Ok(Some(quote! {
        #[automatically_derived]
        impl #impl_generics __deser::Deserialize<'de> for #ident #ty_generics #where_clause {
            #[inline]
            fn deserialize_into(__slot: &mut __deser::__derive::Option<Self>)
                -> __deser::de::SinkHandle<'_, 'de>
            {
                #adapter::deserialize_into_as(__slot)
            }

            #[inline]
            fn initial_value() -> __deser::__derive::Option<Self> {
                #adapter::initial_value_as()
            }

            #[inline]
            fn __private_atom_into(
                __slot: &mut __deser::__derive::Option<Self>,
                __atom: __deser::Atom,
                __state: &mut __deser::State,
            ) -> __deser::__derive::Result<()> {
                #adapter::__private_atom_into_as(__slot, __atom, __state)
            }

            #[inline]
            fn __private_borrowed_atom_into(
                __slot: &mut __deser::__derive::Option<Self>,
                __atom: __deser::Atom<'de>,
                __state: &mut __deser::State,
            ) -> __deser::__derive::Result<()> {
                #adapter::__private_borrowed_atom_into_as(__slot, __atom, __state)
            }

            #[inline]
            fn __private_is_bytes() -> bool {
                #adapter::__private_is_bytes_as()
            }

            #[inline]
            fn __private_vec_from_bytes(__bytes: __deser::__derive::Vec<__deser::__derive::u8>)
                -> __deser::__derive::Option<__deser::__derive::Vec<Self>>
            {
                #adapter::__private_vec_from_bytes_as(__bytes)
            }

            #[inline]
            fn __private_array_from_bytes<const __N: usize>(__bytes: &[__deser::__derive::u8])
                -> __deser::__derive::Option<[Self; __N]>
            {
                #adapter::__private_array_from_bytes_as::<__N>(__bytes)
            }
        }
    }))
}
