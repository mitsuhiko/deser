//! The analysis of structs with unnamed fields and unit structs.
//!
//! How such structs are represented depends on the fields that are not
//! skipped in a direction (like the content of tuple variants): without
//! fields they are null (like unit structs), with one field they are the
//! value of the field (like newtype structs) and with more fields they are
//! sequences (tuple structs).
use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::quote;

use crate::attr::{Direction, TypeDefault, UnnamedFieldAttrs};
use crate::bound::{BoundField, collect_idents};

/// An unnamed field.
pub struct UnnamedField<'a> {
    pub field: &'a syn::Field,
    pub attrs: UnnamedFieldAttrs,
    /// The member to access the field (`0`, `1`, ...).
    pub member: syn::Index,
}

impl UnnamedField<'_> {
    pub fn ty(&self) -> &syn::Type {
        &self.field.ty
    }
}

/// The field of a struct that is serialized and deserialized like the
/// field (newtype structs and transparent structs).
pub struct NewtypeField<'a> {
    /// The member to access the field.
    pub member: syn::Member,
    pub ty: &'a syn::Type,
    /// The adapter of the field for the direction.
    pub adapter: Option<&'a syn::Type>,
    /// Converts the value of the field into the struct (deserialization
    /// only).
    pub convert: TokenStream,
}

/// A struct with unnamed fields or a unit struct.
pub struct UnnamedStruct<'a> {
    ident: &'a syn::Ident,
    unit: bool,
    pub fields: Vec<UnnamedField<'a>>,
}

impl<'a> UnnamedStruct<'a> {
    /// Returns the struct if it's a struct with unnamed fields or a unit
    /// struct.
    pub fn of(input: &'a syn::DeriveInput) -> syn::Result<Option<UnnamedStruct<'a>>> {
        let (fields, unit) = match input.data {
            syn::Data::Struct(syn::DataStruct {
                fields: syn::Fields::Unnamed(ref fields),
                ..
            }) => (Some(fields), false),
            syn::Data::Struct(syn::DataStruct {
                fields: syn::Fields::Unit,
                ..
            }) => (None, true),
            _ => return Ok(None),
        };
        let fields = fields
            .into_iter()
            .flat_map(|x| x.unnamed.iter())
            .enumerate()
            .map(|(index, field)| {
                let attrs = UnnamedFieldAttrs::of(field)?;
                if attrs.tag() {
                    return Err(syn::Error::new_spanned(
                        field,
                        "tag fields are only supported in other variants of enums",
                    ));
                }
                Ok(UnnamedField {
                    field,
                    attrs,
                    member: syn::Index::from(index),
                })
            })
            .collect::<syn::Result<Vec<_>>>()?;
        Ok(Some(UnnamedStruct {
            ident: &input.ident,
            unit,
            fields,
        }))
    }

    /// Describes the struct in errors about attributes.
    pub fn kind(&self) -> &'static str {
        match self.fields.len() {
            _ if self.unit => "unit structs",
            1 => "newtype structs",
            _ => "tuple structs",
        }
    }

    /// Returns the fields that are not skipped in the direction.
    pub fn remaining(&self, direction: Direction) -> Vec<&UnnamedField<'a>> {
        self.fields
            .iter()
            .filter(|x| !x.attrs.skipped(direction))
            .collect()
    }

    /// Returns the fields for the purpose of bound inference.
    pub fn bound_fields(&self, direction: Direction) -> Vec<BoundField<'_>> {
        self.fields
            .iter()
            .map(|x| BoundField {
                ty: x.ty(),
                adapter: x.attrs.adapters().get(direction),
                skipped: x.attrs.skipped(direction),
            })
            .collect()
    }

    /// Returns an expression that constructs the struct.
    ///
    /// The values of the fields that are not skipped when deserializing are
    /// given in order, skipped fields are filled in with their default.
    pub fn construct(&self, values: &[TokenStream]) -> TokenStream {
        let ident = self.ident;
        if self.unit {
            return quote! { #ident };
        }
        let mut values = values.iter();
        let fields = self.fields.iter().map(|field| {
            if field.attrs.skip_deserializing() {
                skipped_value(field.ty(), field.attrs.default())
            } else {
                values.next().unwrap().clone()
            }
        });
        quote! { #ident(#(#fields),*) }
    }

    /// Returns the `Default` bounds the skipped fields of generic types need
    /// when deserializing.
    pub fn default_bounds(&self, generics: &syn::Generics) -> Vec<syn::WherePredicate> {
        let fields = self
            .fields
            .iter()
            .filter(|x| x.attrs.skip_deserializing())
            .map(|x| (x.ty(), x.attrs.default()));
        default_bounds(generics, fields)
    }
}

/// Returns the value of a field that is skipped when deserializing.
pub fn skipped_value(ty: &syn::Type, default: Option<&TypeDefault>) -> TokenStream {
    match default {
        Some(TypeDefault::Explicit(expr)) => expr.clone(),
        Some(TypeDefault::Implicit) | None => {
            quote! { <#ty as __deser::__derive::Default>::default() }
        }
    }
}

/// Returns the `Default` bounds for skipped fields of generic types that
/// are filled in with `Default`.
pub fn default_bounds<'a>(
    generics: &syn::Generics,
    fields: impl Iterator<Item = (&'a syn::Type, Option<&'a TypeDefault>)>,
) -> Vec<syn::WherePredicate> {
    let params = generics
        .type_params()
        .map(|x| x.ident.to_string())
        .collect::<HashSet<_>>();
    fields
        .filter(|(_, default)| !matches!(default, Some(TypeDefault::Explicit(_))))
        .filter(|(ty, _)| {
            let mut idents = HashSet::new();
            collect_idents(quote! { #ty }, &mut idents);
            idents.iter().any(|x| params.contains(x))
        })
        .map(|(ty, _)| syn::parse_quote!(#ty: __deser::__derive::Default))
        .collect()
}
