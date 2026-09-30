//! Derives for structs with named fields that are transparent.
//!
//! `#[deser(transparent)]` makes a struct serialize and deserialize like
//! its only field that is not skipped, like newtype structs.  Structs with
//! unnamed fields are like that without the attribute (if only one of their
//! fields is not skipped), for them the attribute only checks this.
use proc_macro2::TokenStream;
use quote::quote;

use crate::attr::{ContainerAttrs, Direction, FieldAttrs};
use crate::bound::where_clause_for_fields;
use crate::unnamed::{NewtypeField, default_bounds, skipped_value};

/// The attributes of fields that have an effect on transparent structs.
const FIELD_ATTRS: &[&str] = &[
    "as",
    "serialize_as",
    "deserialize_as",
    "skip",
    "skip_serializing",
    "skip_deserializing",
    "default",
];

/// Returns the error for transparent structs that do not have exactly one
/// field for the direction.
pub(crate) fn field_count_error(input: &syn::DeriveInput, direction: Direction) -> syn::Error {
    let what = match direction {
        Direction::Serialize => "serialized",
        Direction::Deserialize => "deserialized",
    };
    syn::Error::new_spanned(
        &input.ident,
        format!(
            "transparent structs need exactly one field that is {}",
            what
        ),
    )
}

/// Derives a transparent struct with named fields.
///
/// Returns `None` if the struct is not transparent or does not have named
/// fields.
pub(crate) fn derive(
    input: &syn::DeriveInput,
    direction: Direction,
) -> syn::Result<Option<TokenStream>> {
    let fields = match input.data {
        syn::Data::Struct(syn::DataStruct {
            fields: syn::Fields::Named(ref fields),
            ..
        }) => fields,
        _ => return Ok(None),
    };
    let container_attrs = ContainerAttrs::of(input, direction)?;
    if !container_attrs.transparent() {
        return Ok(None);
    }
    container_attrs.reject_named_only("transparent structs", direction)?;

    let all_attrs = FieldAttrs::of_all(&fields.named)?;
    let mut remaining = Vec::new();
    let mut skipped_fields = Vec::new();
    let mut bound_fields = Vec::with_capacity(all_attrs.len());
    for attrs in &all_attrs {
        for seen in FieldAttrs::of(attrs.field())?.into_seen() {
            if !FIELD_ATTRS.contains(&seen.name.as_str()) {
                return Err(syn::Error::new(
                    seen.span,
                    format!(
                        "`{}` has no effect on the fields of transparent structs",
                        seen.name
                    ),
                ));
            }
        }
        if attrs.skipped(direction) {
            skipped_fields.push(attrs);
        } else {
            remaining.push(attrs);
        }
        bound_fields.push(attrs.bound_field(direction));
    }
    let [field] = remaining[..] else {
        return Err(field_count_error(input, direction));
    };
    if direction == Direction::Deserialize && field.default().is_some() {
        return Err(syn::Error::new_spanned(
            field.field(),
            "`default` has no effect on the field of transparent structs, it cannot be missing",
        ));
    }

    let member = syn::Member::Named(field.field().ident.clone().unwrap());
    let ty = &field.field().ty;

    Ok(Some(match direction {
        Direction::Serialize => {
            let where_clause = where_clause_for_fields(
                &input.generics,
                quote!(__deser::Serialize),
                Some(quote!(__deser::__derive::Sync)),
                quote!(__deser::Serialize),
                None,
                container_attrs.serialize_bound(),
                &bound_fields,
            );
            let field = NewtypeField {
                member,
                ty,
                adapter: field.adapters().ser(),
                convert: TokenStream::new(),
            };
            crate::ser::derive_newtype_struct(input, &container_attrs, &field, where_clause)?
        }
        Direction::Deserialize => {
            let mut where_clause = where_clause_for_fields(
                &input.generics,
                quote!(__deser::Deserialize<'de>),
                Some(quote!(__deser::__derive::Send)),
                quote!(__deser::Deserialize),
                Some(quote!('de)),
                container_attrs.deserialize_bound(),
                &bound_fields,
            );
            let mut skipped_names = Vec::with_capacity(skipped_fields.len());
            let mut skipped_values = Vec::with_capacity(skipped_fields.len());
            let mut skipped_defaults = Vec::with_capacity(skipped_fields.len());
            for x in &skipped_fields {
                skipped_names.push(&x.field().ident);
                skipped_values.push(skipped_value(&x.field().ty, x.default()));
                skipped_defaults.push((&x.field().ty, x.default()));
            }
            if container_attrs.deserialize_bound().is_none() {
                where_clause
                    .predicates
                    .extend(default_bounds(&input.generics, &skipped_defaults));
            }
            let ident = &input.ident;
            let name = &field.field().ident;
            let convert = quote! {
                |__value| #ident {
                    #name: __value,
                    #(#skipped_names: #skipped_values,)*
                }
            };
            let field = NewtypeField {
                member,
                ty,
                adapter: field.adapters().de(),
                convert,
            };
            crate::de::derive_newtype_struct(input, &container_attrs, &field, where_clause)?
        }
    }))
}
