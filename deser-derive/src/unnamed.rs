//! The analysis of structs with unnamed fields and unit structs.
//!
//! How such structs are represented depends on the fields that are not
//! skipped in a direction (like the content of tuple variants): without
//! fields they are null (like unit structs), with one field they are the
//! value of the field (like newtype structs) and with more fields they are
//! sequences (tuple structs).
use proc_macro2::TokenStream;
use quote::quote;

use crate::attr::{Direction, TypeDefault, UnnamedFieldAttrs};
use crate::bound::{BoundField, mentions_any, type_param_names};

/// An unnamed field.
pub(crate) struct UnnamedField<'a> {
    pub field: &'a syn::Field,
    pub attrs: UnnamedFieldAttrs,
    /// The member to access the field (`0`, `1`, ...).
    pub member: syn::Index,
}

impl UnnamedField<'_> {
    pub(crate) fn ty(&self) -> &syn::Type {
        &self.field.ty
    }
}

/// The field of a struct that is serialized and deserialized like the
/// field (newtype structs and transparent structs).
pub(crate) struct NewtypeField<'a> {
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
pub(crate) struct UnnamedStruct<'a> {
    ident: &'a syn::Ident,
    unit: bool,
    pub fields: Vec<UnnamedField<'a>>,
}

impl<'a> UnnamedStruct<'a> {
    /// Returns the struct if it's a struct with unnamed fields or a unit
    /// struct.
    pub(crate) fn of(input: &'a syn::DeriveInput) -> syn::Result<Option<UnnamedStruct<'a>>> {
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
        let mut rv = Vec::new();
        if let Some(fields) = fields {
            for (index, field) in fields.unnamed.iter().enumerate() {
                let attrs = UnnamedFieldAttrs::of(field)?;
                if attrs.tag() {
                    return Err(syn::Error::new_spanned(
                        field,
                        "tag fields are only supported in other variants of enums",
                    ));
                }
                rv.push(UnnamedField {
                    field,
                    attrs,
                    member: syn::Index::from(index),
                });
            }
        }
        Ok(Some(UnnamedStruct {
            ident: &input.ident,
            unit,
            fields: rv,
        }))
    }

    /// Describes the struct in errors about attributes.
    pub(crate) fn kind(&self) -> &'static str {
        match self.fields.len() {
            _ if self.unit => "unit structs",
            1 => "newtype structs",
            _ => "tuple structs",
        }
    }

    /// Returns the fields that are not skipped in the direction.
    pub(crate) fn remaining(&self, direction: Direction) -> Vec<&UnnamedField<'a>> {
        let mut rv = Vec::new();
        for field in &self.fields {
            if !field.attrs.skipped(direction) {
                rv.push(field);
            }
        }
        rv
    }

    /// Returns the fields for the purpose of bound inference.
    pub(crate) fn bound_fields(&self, direction: Direction) -> Vec<BoundField<'_>> {
        let mut rv = Vec::with_capacity(self.fields.len());
        for field in &self.fields {
            rv.push(BoundField {
                ty: field.ty(),
                adapter: field.attrs.adapters().get(direction),
                skipped: field.attrs.skipped(direction),
                bound: field.attrs.bounds().get(direction),
                higher_ranked: false,
            });
        }
        rv
    }

    /// Returns an expression that constructs the struct.
    ///
    /// The values of the fields that are not skipped when deserializing are
    /// given in order, skipped fields are filled in with their default.
    pub(crate) fn construct(&self, values: &[TokenStream]) -> TokenStream {
        let ident = self.ident;
        if self.unit {
            return quote! { #ident };
        }
        let mut values = values.iter();
        let mut fields = Vec::with_capacity(self.fields.len());
        for field in &self.fields {
            fields.push(if field.attrs.skip_deserializing() {
                skipped_value(field.ty(), field.attrs.default())
            } else {
                values.next().unwrap().clone()
            });
        }
        quote! { #ident(#(#fields),*) }
    }

    /// Returns the `Default` bounds the skipped fields of generic types need
    /// when deserializing.
    pub(crate) fn default_bounds(&self, generics: &syn::Generics) -> Vec<syn::WherePredicate> {
        let mut fields = Vec::new();
        for field in &self.fields {
            if field.attrs.skip_deserializing() {
                fields.push((field.ty(), field.attrs.default()));
            }
        }
        default_bounds(generics, &fields)
    }
}

/// Returns the value of a field that is skipped when deserializing.
pub(crate) fn skipped_value(ty: &syn::Type, default: Option<&TypeDefault>) -> TokenStream {
    match default {
        Some(TypeDefault::Explicit(expr)) => expr.clone(),
        Some(TypeDefault::Implicit) | None => {
            quote! { <#ty as __deser::__derive::Default>::default() }
        }
    }
}

/// Returns the `Default` bounds for skipped fields of generic types that
/// are filled in with `Default`.
pub(crate) fn default_bounds(
    generics: &syn::Generics,
    fields: &[(&syn::Type, Option<&TypeDefault>)],
) -> Vec<syn::WherePredicate> {
    let params = type_param_names(generics);
    let mut rv = Vec::new();
    for &(ty, default) in fields {
        if !matches!(default, Some(TypeDefault::Explicit(_)))
            && mentions_any(quote! { #ty }, &params)
        {
            rv.push(syn::parse_quote!(#ty: __deser::__derive::Default));
        }
    }
    rv
}
