//! Derives for structs with named fields that are transparent.
//!
//! `#[deser(transparent)]` makes a struct serialize and deserialize like
//! its only field that is not skipped, like newtype structs.  Structs with
//! unnamed fields are like that without the attribute (if only one of their
//! fields is not skipped), for them the attribute only checks this.
use proc_macro2::TokenStream;
use quote::quote;

use crate::attr::{ContainerAttrs, Direction, FieldAttrs};
use crate::bound::{BoundField, where_clause_for_fields};
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
pub fn field_count_error(input: &syn::DeriveInput, direction: Direction) -> syn::Error {
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
pub fn derive(input: &syn::DeriveInput, direction: Direction) -> syn::Result<Option<TokenStream>> {
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

    let all_attrs = fields
        .named
        .iter()
        .map(FieldAttrs::of)
        .collect::<syn::Result<Vec<_>>>()?;
    let skipped = |attrs: &FieldAttrs| match direction {
        Direction::Serialize => attrs.skip_serializing(),
        Direction::Deserialize => attrs.skip_deserializing(),
    };
    for attrs in &all_attrs {
        let seen = FieldAttrs::of(attrs.field())?.into_seen();
        if let Some(seen) = seen
            .iter()
            .find(|x| !FIELD_ATTRS.contains(&x.name.as_str()))
        {
            return Err(syn::Error::new(
                seen.span,
                format!(
                    "`{}` has no effect on the fields of transparent structs",
                    seen.name
                ),
            ));
        }
    }
    let remaining = all_attrs.iter().filter(|x| !skipped(x)).collect::<Vec<_>>();
    let [field] = remaining[..] else {
        return Err(field_count_error(input, direction));
    };
    if direction == Direction::Deserialize && field.default().is_some() {
        return Err(syn::Error::new_spanned(
            field.field(),
            "`default` has no effect on the field of transparent structs, it cannot be missing",
        ));
    }

    let bound_fields = all_attrs
        .iter()
        .map(|x| BoundField {
            ty: &x.field().ty,
            adapter: x.adapters().get(direction),
            skipped: skipped(x),
        })
        .collect::<Vec<_>>();
    let member = syn::Member::Named(field.field().ident.clone().unwrap());
    let ty = &field.field().ty;

    Ok(Some(match direction {
        Direction::Serialize => {
            let where_clause = where_clause_for_fields(
                &input.generics,
                quote!(__deser::Serialize),
                Some(quote!(__deser::__derive::Sync)),
                quote!(__deser::adapters::SerializeAs),
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
                quote!(__deser::adapters::DeserializeAs),
                Some(quote!('de)),
                container_attrs.deserialize_bound(),
                &bound_fields,
            );
            let skipped_fields = all_attrs.iter().filter(|x| skipped(x));
            if container_attrs.deserialize_bound().is_none() {
                where_clause.predicates.extend(default_bounds(
                    &input.generics,
                    skipped_fields.clone().map(|x| (&x.field().ty, x.default())),
                ));
            }
            let ident = &input.ident;
            let name = &field.field().ident;
            let skipped_names = skipped_fields.clone().map(|x| &x.field().ident);
            let skipped_values = skipped_fields.map(|x| skipped_value(&x.field().ty, x.default()));
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
