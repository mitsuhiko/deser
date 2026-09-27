//! Derives for enums with data.
//!
//! Supported are the four representations known from serde:
//!
//! * externally tagged (the default): `"Unit"`, `{"Variant": content}`
//! * internally tagged (`tag`): `{"tag": "Variant", ...fields}`
//! * adjacently tagged (`tag` and `content`): `{"tag": "Variant", "content": content}`
//! * untagged (`untagged`): `content`
//!
//! The content of unit variants is null, of newtype variants the inner value,
//! of tuple variants a sequence and of struct variants a map.  The heavy
//! lifting is done by support code in `deser::__derive`.
//!
//! The variant marked with `#[deser(other)]` receives unknown tags.  It can
//! have a field marked with `#[deser(tag)]` that receives the tag, the
//! content of such a variant is made up of the remaining fields.
use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::quote;

use crate::attr::{
    Adapters, ContainerAttrs, Direction, EnumVariantAttrs, FieldAttrs, FieldBounds, Name,
    RenameAll, TypeDefault, UnnamedFieldAttrs, VariantName,
};
use crate::bound::{
    BoundField, collect_idents, collect_lifetimes, turbofish_without_lifetimes,
    where_clause_for_fields, with_lifetime_bound,
};

#[derive(Copy, Clone)]
enum Repr<'a> {
    External,
    Internal { tag: &'a Name },
    Adjacent { tag: &'a Name, content: &'a Name },
    Untagged,
}

/// The shape of a variant in Rust.
enum Shape {
    Unit,
    Tuple,
    Named,
}

/// The content of a variant (all fields except the tag field).
enum Content {
    Unit,
    Newtype(usize),
    Tuple(Vec<usize>),
    Struct(Vec<usize>),
}

struct FieldInfo<'a> {
    field: &'a syn::Field,
    adapters: Adapters,
    bounds: FieldBounds,
    tag: bool,
    skip_serializing: bool,
    skip_deserializing: bool,
    // skipped when deserializing and filled in with `Default`
    needs_default: bool,
    // the value of unnamed fields that are skipped when deserializing (the
    // helper structs of struct variants fill in named fields)
    default: Option<TypeDefault>,
    binding: syn::Ident,
}

impl<'a> FieldInfo<'a> {
    fn ty(&self) -> &'a syn::Type {
        &self.field.ty
    }

    /// Returns the type the field is deserialized as.
    fn de_ty(&self) -> TokenStream {
        let ty = self.ty();
        match self.adapters.de() {
            Some(adapter) => quote! { __deser::adapters::As<#ty, #adapter> },
            None => quote! { #ty },
        }
    }

    /// Converts a value of the type returned by `de_ty` into the field value.
    fn unwrap(&self, value: TokenStream) -> TokenStream {
        if self.adapters.de().is_some() {
            quote! { #value.into_inner() }
        } else {
            value
        }
    }

    /// Returns a serialize handle for the bound field.
    fn ser_handle(&self) -> TokenStream {
        let binding = &self.binding;
        crate::ser::serialize_handle(self.ty(), self.adapters.ser(), quote! { #binding })
    }

    /// Returns a reference to a serializable for the bound field.
    fn ser_value(&self) -> TokenStream {
        let binding = &self.binding;
        let ty = self.ty();
        match self.adapters.ser() {
            Some(adapter) => quote! {
                __deser::__derive::SerializeAsRef::<#adapter, #ty>::new(#binding)
            },
            None => quote! { #binding },
        }
    }

    /// Returns an expression that checks if the bound field is optional.
    fn is_optional(&self) -> TokenStream {
        let binding = &self.binding;
        crate::ser::is_optional(self.ty(), self.adapters.ser(), quote! { #binding })
    }
}

struct VariantInfo<'a> {
    variant: &'a syn::Variant,
    ident: &'a syn::Ident,
    name: VariantName,
    names: Vec<VariantName>,
    other: bool,
    default: bool,
    deny_unknown_fields: bool,
    skip_serializing: bool,
    skip_deserializing: bool,
    // an untagged variant of a tagged enum (or a variant of an untagged
    // enum)
    untagged: bool,
    shape: Shape,
    fields: Vec<FieldInfo<'a>>,
    tag_field: Option<usize>,
    content: Content,
    // the name style of the fields of struct variants
    fields_rename_all: Option<RenameAll>,
}

impl<'a> VariantInfo<'a> {
    fn helper(&self) -> syn::Ident {
        syn::Ident::new(&format!("__Variant{}", self.ident), Span::call_site())
    }

    /// Returns the pattern that binds all fields by reference.
    fn pattern(&self, enum_ident: &syn::Ident) -> TokenStream {
        let var_ident = self.ident;
        let bindings = self.fields.iter().map(|x| &x.binding);
        match self.shape {
            Shape::Unit => quote! { #enum_ident::#var_ident },
            Shape::Tuple => quote! { #enum_ident::#var_ident(#(ref #bindings),*) },
            Shape::Named => {
                let names = self.fields.iter().map(|x| &x.field.ident);
                quote! { #enum_ident::#var_ident { #(#names: ref #bindings),* } }
            }
        }
    }

    /// Constructs the variant from values for all fields.
    fn construct(&self, enum_ident: &syn::Ident, values: &[TokenStream]) -> TokenStream {
        let var_ident = self.ident;
        match self.shape {
            Shape::Unit => quote! { #enum_ident::#var_ident },
            Shape::Tuple => quote! { #enum_ident::#var_ident(#(#values),*) },
            Shape::Named => {
                let names = self.fields.iter().map(|x| &x.field.ident);
                quote! { #enum_ident::#var_ident { #(#names: #values),* } }
            }
        }
    }

    /// Returns the fields which make up the content.
    fn content_fields(&self) -> Vec<&FieldInfo<'a>> {
        match self.content {
            Content::Unit => Vec::new(),
            Content::Newtype(idx) => vec![&self.fields[idx]],
            Content::Tuple(ref idxs) | Content::Struct(ref idxs) => {
                idxs.iter().map(|&idx| &self.fields[idx]).collect()
            }
        }
    }

    /// Returns the pattern that matches the variant without binding fields.
    fn wildcard_pattern(&self, enum_ident: &syn::Ident) -> TokenStream {
        let var_ident = self.ident;
        match self.shape {
            Shape::Unit => quote! { #enum_ident::#var_ident },
            Shape::Tuple => quote! { #enum_ident::#var_ident(..) },
            Shape::Named => quote! { #enum_ident::#var_ident { .. } },
        }
    }

    fn tag_field(&self) -> Option<&FieldInfo<'a>> {
        self.tag_field.map(|idx| &self.fields[idx])
    }
}

/// Returns the name of a generic parameter (without the `'` of lifetimes).
fn param_name(param: &syn::GenericParam) -> String {
    match param {
        syn::GenericParam::Lifetime(param) => param.lifetime.ident.to_string(),
        syn::GenericParam::Type(param) => param.ident.to_string(),
        syn::GenericParam::Const(param) => param.ident.to_string(),
    }
}

/// Returns `true` if the parameter is used in the tokens.
///
/// `idents` and `lifetimes` are the identifiers and lifetimes of the tokens.
fn param_used(
    param: &syn::GenericParam,
    idents: &HashSet<String>,
    lifetimes: &HashSet<String>,
) -> bool {
    match param {
        syn::GenericParam::Lifetime(_) => lifetimes.contains(&param_name(param)),
        _ => idents.contains(&param_name(param)),
    }
}

/// Returns the generic parameters of the generics that appear in the types.
fn used_params<'a>(
    generics: &'a syn::Generics,
    types: &[TokenStream],
) -> Vec<&'a syn::GenericParam> {
    let mut idents = HashSet::new();
    let mut lifetimes = HashSet::new();
    for ty in types {
        collect_idents(ty.clone(), &mut idents);
        collect_lifetimes(ty.clone(), &mut lifetimes);
    }
    generics
        .params
        .iter()
        .filter(|param| param_used(param, &idents, &lifetimes))
        .collect()
}

/// Returns `true` if the tokens only refer to the given parameters (of the
/// parameters of the generics).
fn only_uses(generics: &syn::Generics, tokens: TokenStream, params: &[&syn::GenericParam]) -> bool {
    let mut idents = HashSet::new();
    let mut lifetimes = HashSet::new();
    collect_idents(tokens.clone(), &mut idents);
    collect_lifetimes(tokens, &mut lifetimes);
    generics
        .params
        .iter()
        .filter(|param| param_used(param, &idents, &lifetimes))
        .all(|param| params.iter().any(|x| param_name(x) == param_name(param)))
}

/// Returns the predicates which only refer to the given parameters.
fn filter_predicates<'a>(
    generics: &syn::Generics,
    predicates: impl IntoIterator<Item = &'a syn::WherePredicate>,
    params: &[&syn::GenericParam],
) -> Vec<&'a syn::WherePredicate> {
    predicates
        .into_iter()
        .filter(|predicate| only_uses(generics, quote! { #predicate }, params))
        .collect()
}

/// Returns the declaration of the parameters of a helper struct.
///
/// The helper struct takes the parameters of the enum that its fields use,
/// with the bounds of the enum that only refer to them.
fn helper_params_decl(generics: &syn::Generics, params: &[&syn::GenericParam]) -> TokenStream {
    let decls = params.iter().map(|param| match param {
        syn::GenericParam::Lifetime(param) => {
            let lifetime = &param.lifetime;
            let bounds = param
                .bounds
                .iter()
                .filter(|x| only_uses(generics, quote! { #x }, params))
                .collect::<Vec<_>>();
            if bounds.is_empty() {
                quote! { #lifetime }
            } else {
                quote! { #lifetime: #(#bounds)+* }
            }
        }
        syn::GenericParam::Type(param) => {
            let ident = &param.ident;
            let bounds = param
                .bounds
                .iter()
                .filter(|x| only_uses(generics, quote! { #x }, params))
                .collect::<Vec<_>>();
            if bounds.is_empty() {
                quote! { #ident }
            } else {
                quote! { #ident: #(#bounds)+* }
            }
        }
        syn::GenericParam::Const(param) => {
            let ident = &param.ident;
            let ty = &param.ty;
            quote! { const #ident: #ty }
        }
    });
    quote! { #(#decls),* }
}

/// Returns the arguments for the parameters of a helper struct.
fn helper_params_args(params: &[&syn::GenericParam]) -> TokenStream {
    let args = params.iter().map(|param| match param {
        syn::GenericParam::Lifetime(param) => {
            let lifetime = &param.lifetime;
            quote! { #lifetime }
        }
        syn::GenericParam::Type(param) => {
            let ident = &param.ident;
            quote! { #ident }
        }
        syn::GenericParam::Const(param) => {
            let ident = &param.ident;
            quote! { #ident }
        }
    });
    quote! { #(#args),* }
}

/// Returns the where clause predicates of the generics which only refer to
/// the given parameters.
fn helper_where_clause(generics: &syn::Generics, params: &[&syn::GenericParam]) -> TokenStream {
    let where_clause = match generics.where_clause {
        Some(ref where_clause) => where_clause,
        None => return TokenStream::new(),
    };
    let predicates = filter_predicates(generics, &where_clause.predicates, params);
    if predicates.is_empty() {
        TokenStream::new()
    } else {
        quote! { where #(#predicates),* }
    }
}

/// Returns `true` if the enum needs the support for enums with data.
///
/// Enums with only unit variants and no special representation are handled
/// by the simpler string based derive.
pub fn is_data_enum(
    input: &syn::DeriveInput,
    container_attrs: &ContainerAttrs,
    enumeration: &syn::DataEnum,
) -> bool {
    container_attrs.tag().is_some()
        || container_attrs.untagged()
        || !input.generics.params.is_empty()
        || enumeration.variants.iter().any(|x| {
            !matches!(x.fields, syn::Fields::Unit)
                || EnumVariantAttrs::of(x).is_ok_and(|x| x.untagged())
        })
}

fn repr<'a>(container_attrs: &'a ContainerAttrs) -> Repr<'a> {
    match (container_attrs.tag(), container_attrs.content()) {
        (Some(tag), Some(content)) => Repr::Adjacent { tag, content },
        (Some(tag), None) => Repr::Internal { tag },
        (None, _) if container_attrs.untagged() => Repr::Untagged,
        (None, _) => Repr::External,
    }
}

fn collect_fields(variant: &syn::Variant) -> syn::Result<(Shape, Vec<FieldInfo<'_>>)> {
    let mut fields = Vec::new();
    let shape = match variant.fields {
        syn::Fields::Unit => Shape::Unit,
        syn::Fields::Unnamed(ref unnamed) => {
            for (idx, field) in unnamed.unnamed.iter().enumerate() {
                let attrs = UnnamedFieldAttrs::of(field)?;
                fields.push(FieldInfo {
                    field,
                    adapters: attrs.adapters().clone(),
                    bounds: attrs.bounds().clone(),
                    tag: attrs.tag(),
                    skip_serializing: attrs.skip_serializing(),
                    skip_deserializing: attrs.skip_deserializing(),
                    needs_default: attrs.skip_deserializing()
                        && matches!(attrs.default(), None | Some(TypeDefault::Implicit)),
                    default: attrs.default().cloned(),
                    binding: syn::Ident::new(&format!("__f{}", idx), Span::call_site()),
                });
            }
            Shape::Tuple
        }
        syn::Fields::Named(ref named) => {
            for field in named.named.iter() {
                let attrs = FieldAttrs::of(field)?;
                fields.push(FieldInfo {
                    field,
                    adapters: attrs.adapters().clone(),
                    bounds: attrs.bounds().clone(),
                    tag: attrs.tag(),
                    skip_serializing: attrs.skip_serializing(),
                    skip_deserializing: attrs.skip_deserializing(),
                    needs_default: attrs.skip_deserializing()
                        && matches!(attrs.default(), None | Some(TypeDefault::Implicit)),
                    default: None,
                    binding: syn::Ident::new(
                        &format!("__field_{}", field.ident.as_ref().unwrap()),
                        Span::call_site(),
                    ),
                });
            }
            Shape::Named
        }
    };
    Ok((shape, fields))
}

/// Collects the variants of an enum.
///
/// The content of tuple variants is made up of the fields that are not
/// skipped in the direction.
fn collect_variants<'a>(
    enumeration: &'a syn::DataEnum,
    container_attrs: &ContainerAttrs,
    direction: Direction,
) -> syn::Result<Vec<VariantInfo<'a>>> {
    let mut rv = Vec::new();
    let mut seen_names = HashSet::new();
    let mut seen_other = false;
    let mut seen_default = false;
    let repr = repr(container_attrs);

    for variant in &enumeration.variants {
        let attrs = EnumVariantAttrs::of(variant)?;
        let name = attrs.name(container_attrs);
        if attrs.untagged() && matches!(repr, Repr::Untagged) {
            return Err(syn::Error::new_spanned(
                variant,
                "untagged has no effect on variants of untagged enums",
            ));
        }
        // untagged variants of tagged enums are represented like the
        // variants of untagged enums
        let repr = if attrs.untagged() {
            Repr::Untagged
        } else {
            repr
        };
        let mut names = Vec::new();
        // untagged variants are not selected by their name
        let tags = if attrs.untagged() {
            Vec::new()
        } else {
            std::iter::once(name.clone())
                .chain(attrs.aliases(container_attrs))
                .collect()
        };
        for name in tags {
            if !seen_names.insert(name.clone()) {
                return Err(syn::Error::new_spanned(
                    variant,
                    format!("variant name `{}` used more than once", name.display()),
                ));
            }
            names.push(name);
        }

        if attrs.other() {
            if seen_other {
                return Err(syn::Error::new_spanned(
                    variant,
                    "only one variant can be marked as other",
                ));
            }
            if matches!(repr, Repr::Untagged) {
                return Err(syn::Error::new_spanned(
                    variant,
                    "other is not supported for untagged enums",
                ));
            }
            seen_other = true;
        }

        if attrs.default() {
            if seen_default {
                return Err(syn::Error::new_spanned(
                    variant,
                    "only one variant can be marked as default",
                ));
            }
            if !matches!(repr, Repr::Internal { .. } | Repr::Adjacent { .. }) {
                return Err(syn::Error::new_spanned(
                    variant,
                    "default variants are only supported for internally and adjacently tagged enums",
                ));
            }
            seen_default = true;
        }

        let (shape, fields) = collect_fields(variant)?;

        let mut tag_field = None;
        for (idx, field) in fields.iter().enumerate() {
            if !field.tag {
                continue;
            }
            if !attrs.other() {
                return Err(syn::Error::new_spanned(
                    field.field,
                    "tag fields are only supported in other variants",
                ));
            }
            if tag_field.is_some() {
                return Err(syn::Error::new_spanned(
                    field.field,
                    "only one field can be marked as tag",
                ));
            }
            tag_field = Some(idx);
        }

        let content_idxs = (0..fields.len())
            .filter(|&idx| Some(idx) != tag_field)
            .collect::<Vec<_>>();
        // skipped unnamed fields are not part of the content
        let unnamed_idxs = content_idxs
            .iter()
            .copied()
            .filter(|&idx| match direction {
                Direction::Serialize => !fields[idx].skip_serializing,
                Direction::Deserialize => !fields[idx].skip_deserializing,
            })
            .collect::<Vec<_>>();
        let content = match shape {
            Shape::Unit => Content::Unit,
            Shape::Tuple => match unnamed_idxs.len() {
                0 => Content::Unit,
                // `()` has nothing to merge with the tag of internally
                // tagged enums, the variant is a unit variant.
                1 if matches!(repr, Repr::Internal { .. })
                    && !attrs.other()
                    && is_unit_type(fields[unnamed_idxs[0]].ty())
                    && !fields[unnamed_idxs[0]].adapters.any() =>
                {
                    Content::Unit
                }
                1 => Content::Newtype(unnamed_idxs[0]),
                len if len > crate::de::MAX_TUPLE_LEN => {
                    return Err(syn::Error::new_spanned(
                        variant,
                        format!(
                            "tuple variants with more than {} fields are not supported",
                            crate::de::MAX_TUPLE_LEN
                        ),
                    ));
                }
                _ => Content::Tuple(unnamed_idxs),
            },
            Shape::Named if tag_field.is_some() && content_idxs.is_empty() => Content::Unit,
            Shape::Named => Content::Struct(content_idxs),
        };

        if matches!(content, Content::Tuple(_)) && matches!(repr, Repr::Internal { .. }) {
            return Err(syn::Error::new_spanned(
                variant,
                "internally tagged enums do not support tuple variants",
            ));
        }

        rv.push(VariantInfo {
            variant,
            ident: &variant.ident,
            name,
            names,
            other: attrs.other(),
            default: attrs.default(),
            deny_unknown_fields: attrs.deny_unknown_fields(),
            untagged: matches!(repr, Repr::Untagged),
            skip_serializing: attrs.skip_serializing(),
            skip_deserializing: attrs.skip_deserializing(),
            shape,
            fields,
            tag_field,
            content,
            fields_rename_all: attrs.fields_rename_all(container_attrs),
        });
    }

    Ok(rv)
}

/// Returns `true` if the type is written as `()`.
fn is_unit_type(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Tuple(tuple) => tuple.elems.is_empty(),
        syn::Type::Paren(paren) => is_unit_type(&paren.elem),
        syn::Type::Group(group) => is_unit_type(&group.elem),
        _ => false,
    }
}

/// Defines the name of the type which is used in error messages (see
/// `expecting`).
fn type_name_const(container_attrs: &ContainerAttrs) -> TokenStream {
    let type_name = container_attrs.expecting();
    quote! {
        const __TYPE_NAME: &__deser::__derive::str = #type_name;
    }
}

/// Returns the fields of all variants for the purpose of bound inference.
///
/// The fields of skipped variants count as skipped.
fn bound_fields<'b>(variants: &'b [VariantInfo], direction: Direction) -> Vec<BoundField<'b>> {
    variants
        .iter()
        .flat_map(|info| info.fields.iter().map(move |field| (info, field)))
        .map(|(info, field)| BoundField {
            ty: field.ty(),
            adapter: field.adapters.get(direction),
            skipped: match direction {
                Direction::Serialize => info.skip_serializing || field.skip_serializing,
                Direction::Deserialize => info.skip_deserializing || field.skip_deserializing,
            },
            bound: field.bounds.get(direction),
        })
        .collect()
}

pub fn derive_deserialize(
    input: &syn::DeriveInput,
    enumeration: &syn::DataEnum,
    container_attrs: &ContainerAttrs,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let repr = repr(container_attrs);
    let all_variants = collect_variants(enumeration, container_attrs, Direction::Deserialize)?;
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let de_generics = crate::bound::with_de_lifetime(&input.generics)?;
    let (impl_generics, _, _) = de_generics.split_for_impl();
    // the builders of the variants live as long as the slot of the enum
    // (`'__a`), which the enum and its parameters outlive
    let builder_generics = with_lifetime_bound(&de_generics, "'__a");
    let (builder_impl_generics, _, _) = builder_generics.split_for_impl();
    // lifetimes are inferred, the builders have more than the enum
    let turbofish = turbofish_without_lifetimes(&input.generics);
    let mut where_clause = where_clause_for_fields(
        &input.generics,
        quote!(__deser::Deserialize<'de>),
        Some(quote!(__deser::__derive::Send)),
        quote!(__deser::adapters::DeserializeAs),
        Some(quote!('de)),
        container_attrs.deserialize_bound(),
        &bound_fields(&all_variants, Direction::Deserialize),
    );
    // skipped variants cannot be deserialized, they are unknown variants
    let variants = all_variants
        .into_iter()
        .filter(|x| !x.skip_deserializing)
        .collect::<Vec<_>>();
    if container_attrs.deserialize_bound().is_none() {
        // skipped fields of generic types need a default, the helper
        // structs of the variants require it
        let params = input
            .generics
            .type_params()
            .map(|x| x.ident.to_string())
            .collect::<HashSet<_>>();
        for field in variants.iter().flat_map(|x| x.fields.iter()) {
            if !field.needs_default {
                continue;
            }
            let ty = field.ty();
            let mut idents = HashSet::new();
            collect_idents(quote! { #ty }, &mut idents);
            if idents.iter().any(|x| params.contains(x)) {
                where_clause
                    .predicates
                    .push(syn::parse_quote!(#ty: __deser::__derive::Default));
            }
        }
    }
    let enum_ty = quote! { #ident #ty_generics };

    let mut helpers = Vec::new();
    let mut builders = Vec::new();
    for info in &variants {
        let var_ident = info.ident;
        let helper = info.helper();
        let content_fields = info.content_fields();

        // struct variants (and unit variants of internally tagged enums which
        // are maps) are deserialized through a helper struct that uses the
        // regular struct derive.
        let needs_helper = match info.content {
            Content::Struct(_) => true,
            Content::Unit => matches!(repr, Repr::Internal { .. }) && !info.other && !info.untagged,
            _ => false,
        };
        if info.deny_unknown_fields && !needs_helper {
            return Err(syn::Error::new_spanned(
                info.variant,
                "deny_unknown_fields on variants only has an effect on struct variants \
                 (and unit variants of internally tagged enums)",
            ));
        }
        // helper structs only take the parameters they use
        let helper_params = match info.content {
            Content::Struct(_) => used_params(
                &input.generics,
                &content_fields
                    .iter()
                    .map(|x| {
                        let ty = x.ty();
                        let adapter = x.adapters.de();
                        quote! { #ty #adapter }
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => Vec::new(),
        };
        let helper_ty = if helper_params.is_empty() {
            quote! { #helper }
        } else {
            let args = helper_params_args(&helper_params);
            quote! { #helper<#args> }
        };
        if needs_helper {
            let helper_name = var_ident.to_string();
            let fields = content_fields
                .iter()
                .map(|field| {
                    let deser_attrs = field
                        .field
                        .attrs
                        .iter()
                        .filter(|x| x.path().is_ident("deser"));
                    let name = &field.field.ident;
                    let ty = field.ty();
                    quote! { #(#deser_attrs)* #name: #ty, }
                })
                .collect::<Vec<_>>();
            // the helper needs the same bounds on its parameters as the enum
            let helper_decl = if helper_params.is_empty() {
                quote! { #helper }
            } else {
                let params = helper_params_decl(&input.generics, &helper_params);
                quote! { #helper<#params> }
            };
            let helper_where = helper_where_clause(&input.generics, &helper_params);
            // the helper is derived with the same crate path and the custom
            // bounds that apply to its parameters.
            let helper_crate = container_attrs
                .crate_path()
                .map(|path| quote! { #[deser(crate = #path)] });
            let helper_bound = container_attrs.deserialize_bound().map(|bound| {
                let predicates = filter_predicates(&input.generics, bound, &helper_params);
                quote! { #[deser(deserialize_bound(#(#predicates),*))] }
            });
            let helper_rename_all = info.fields_rename_all.map(|style| {
                let style = style.as_str();
                quote! { #[deser(rename_all = #style)] }
            });
            let helper_deny = if container_attrs.deny_unknown_fields() || info.deny_unknown_fields {
                Some(quote! { #[deser(deny_unknown_fields)] })
            } else {
                None
            };
            helpers.push(quote! {
                #[derive(__deser::Deserialize)]
                #[deser(rename = #helper_name)]
                #helper_crate
                #helper_bound
                #helper_rename_all
                #helper_deny
                struct #helper_decl #helper_where {
                    #(#fields)*
                }
            });
        }

        // the content is deserialized as a single value which is bound to a
        // pattern that makes the values of the content fields available.
        let mut values = vec![TokenStream::new(); info.fields.len()];
        let (content_ty, content_pattern) = match info.content {
            Content::Unit if needs_helper => {
                // fields of unit variants are `()` (see `collect_variants`)
                for (value, field) in values.iter_mut().zip(&info.fields) {
                    if !field.skip_deserializing {
                        *value = quote! { () };
                    }
                }
                (helper_ty.clone(), quote! { _ })
            }
            Content::Unit if info.other => {
                (quote! { __deser::__derive::IgnoredContent }, quote! { _ })
            }
            Content::Unit => (quote! { () }, quote! { _ }),
            Content::Newtype(idx) => {
                let field = &info.fields[idx];
                values[idx] = field.unwrap(quote! { __content });
                (field.de_ty(), quote! { __content })
            }
            Content::Tuple(ref idxs) => {
                let mut types = Vec::new();
                let mut bindings = Vec::new();
                for &idx in idxs {
                    let field = &info.fields[idx];
                    let binding = &field.binding;
                    values[idx] = field.unwrap(quote! { #binding });
                    types.push(field.de_ty());
                    bindings.push(binding);
                }
                (quote! { (#(#types,)*) }, quote! { (#(#bindings,)*) })
            }
            Content::Struct(ref idxs) => {
                for &idx in idxs {
                    let name = &info.fields[idx].field.ident;
                    values[idx] = quote! { __content.#name };
                }
                (helper_ty.clone(), quote! { __content })
            }
        };

        // skipped unnamed fields are filled in with their default
        if matches!(info.shape, Shape::Tuple) {
            for (value, field) in values.iter_mut().zip(&info.fields) {
                if field.skip_deserializing {
                    *value = crate::unnamed::skipped_value(field.ty(), field.default.as_ref());
                }
            }
        }

        let builder = match info.tag_field() {
            Some(tag_field) => {
                let tag_ty = tag_field.de_ty();
                values[info.tag_field.unwrap()] = tag_field.unwrap(quote! { __tag });
                let construct = info.construct(ident, &values);
                quote! {
                    __deser::__derive::OtherVariant::<#tag_ty, #content_ty, #enum_ty>::boxed(
                        |__tag: #tag_ty, #content_pattern: #content_ty| #construct
                    )
                }
            }
            None if info.other && matches!(info.content, Content::Unit) => {
                let construct = info.construct(ident, &values);
                quote! { __deser::__derive::IgnoredVariant::<#enum_ty>::boxed(|| #construct) }
            }
            None => {
                let construct = info.construct(ident, &values);
                quote! {
                    __deser::__derive::Variant::<#content_ty, #enum_ty>::boxed(
                        |#content_pattern: #content_ty| #construct
                    )
                }
            }
        };
        builders.push(builder);
    }

    let type_name_const = type_name_const(container_attrs);
    let builder_ty = quote! {
        __deser::__derive::BoxedVariant<'__a, 'de, #enum_ty>
    };

    // makes a function for a special variant
    let special_variant = |fn_name: &str, predicate: fn(&VariantInfo) -> bool| {
        let fn_ident = syn::Ident::new(fn_name, Span::call_site());
        match variants
            .iter()
            .zip(builders.iter())
            .find(|(info, _)| predicate(info))
        {
            Some((_, builder)) => (
                quote! {
                    #[allow(clippy::type_complexity, clippy::multiple_bound_locations)]
                    fn #fn_ident #builder_impl_generics () -> #builder_ty #where_clause {
                        #builder
                    }
                },
                quote! {
                    __deser::__derive::Some(
                        #fn_ident #turbofish as __deser::__derive::VariantMaker<'_, 'de, #enum_ty>
                    )
                },
            ),
            None => (quote! {}, quote! { __deser::__derive::None }),
        }
    };

    let variants_table = {
        let arms = variants
            .iter()
            .zip(builders.iter())
            .filter(|(info, _)| !info.other && !info.untagged)
            .map(|(info, builder)| {
                VariantName::tag_arms(&info.names, quote! { __deser::__derive::Some(#builder) })
            });
        let (other_fn, other) = special_variant("__other", |info| info.other);
        let (default_fn, default) = special_variant("__default", |info| info.default);
        let names = variants
            .iter()
            .filter(|info| !info.other && !info.untagged)
            .map(|info| info.name.str_expr());
        (
            quote! {
                #[allow(clippy::type_complexity, clippy::multiple_bound_locations)]
                fn __lookup #builder_impl_generics (
                    __tag: __deser::__derive::Tag<'_>,
                ) -> __deser::__derive::Option<#builder_ty> #where_clause {
                    match __tag {
                        #(#arms)*
                        _ => __deser::__derive::None,
                    }
                }

                #other_fn
                #default_fn
            },
            quote! {
                __deser::__derive::Variants {
                    lookup: __lookup #turbofish,
                    other: #other,
                    default: #default,
                    names: &[#(#names),*],
                }
            },
        )
    };

    let (support, handle) = match repr {
        Repr::External => {
            let unit_arms = variants
                .iter()
                .filter(|info| {
                    matches!(info.content, Content::Unit) && !info.other && !info.untagged
                })
                .map(|info| {
                    // the fields of unit variants are skipped
                    let values = info
                        .fields
                        .iter()
                        .map(|field| {
                            crate::unnamed::skipped_value(field.ty(), field.default.as_ref())
                        })
                        .collect::<Vec<_>>();
                    let construct = info.construct(ident, &values);
                    VariantName::tag_arms(
                        &info.names,
                        quote! { __deser::__derive::Some(#construct) },
                    )
                });
            let (table_support, table) = variants_table;
            (
                quote! {
                    #table_support

                    #[allow(clippy::multiple_bound_locations)]
                    fn __unit #impl_generics (
                        __tag: __deser::__derive::Tag<'_>,
                    ) -> __deser::__derive::Option<#enum_ty> #where_clause {
                        match __tag {
                            #(#unit_arms)*
                            _ => __deser::__derive::None,
                        }
                    }
                },
                quote! {
                    __deser::__derive::ExternallyTaggedSink::handle(
                        __slot,
                        __TYPE_NAME,
                        #table,
                        __unit #turbofish,
                    )
                },
            )
        }
        Repr::Internal { tag } => {
            let tag_key = enum_key(tag, container_attrs.tag_aliases());
            let (table_support, table) = variants_table;
            (
                table_support,
                quote! {
                    __deser::__derive::InternallyTaggedSink::handle(
                        __slot,
                        #tag_key,
                        __TYPE_NAME,
                        #table,
                    )
                },
            )
        }
        Repr::Adjacent { tag, content } => {
            let tag_key = enum_key(tag, container_attrs.tag_aliases());
            let content_key = enum_key(content, container_attrs.content_aliases());
            let (table_support, table) = variants_table;
            let deny = container_attrs.deny_unknown_fields();
            (
                table_support,
                quote! {
                    __deser::__derive::AdjacentlyTaggedSink::handle(
                        __slot,
                        #tag_key,
                        #content_key,
                        __TYPE_NAME,
                        #table,
                        #deny,
                    )
                },
            )
        }
        Repr::Untagged => (
            quote! {},
            quote! {
                __deser::__derive::untagged_handle(__slot, __TYPE_NAME, __candidate #turbofish)
            },
        ),
    };

    // the untagged variants are tried in order (all variants of untagged
    // enums)
    let candidates = variants
        .iter()
        .zip(builders.iter())
        .filter(|(info, _)| info.untagged)
        .map(|(_, builder)| builder)
        .collect::<Vec<_>>();
    let candidate_support = if candidates.is_empty() {
        None
    } else {
        let indexes = 0..candidates.len();
        Some(quote! {
            #[allow(clippy::type_complexity, clippy::multiple_bound_locations)]
            fn __candidate #builder_impl_generics (
                __index: usize,
            ) -> __deser::__derive::Option<#builder_ty> #where_clause {
                match __index {
                    #(#indexes => __deser::__derive::Some(#candidates),)*
                    _ => __deser::__derive::None,
                }
            }
        })
    };

    // tagged enums with untagged variants try the untagged variants if the
    // tagged representation fails
    let (support, handle) = if !matches!(repr, Repr::Untagged) && candidate_support.is_some() {
        let slot_generics = crate::bound::with_slot_lifetime(&de_generics);
        let (slot_impl_generics, _, _) = slot_generics.split_for_impl();
        (
            quote! {
                #support

                #[allow(clippy::type_complexity, clippy::multiple_bound_locations)]
                fn __tagged #slot_impl_generics (
                    __slot: &'__s mut __deser::__derive::Option<#enum_ty>,
                ) -> __deser::de::SinkHandle<'__s, 'de> #where_clause {
                    #handle
                }
            },
            quote! {
                __deser::__derive::untagged_fallback(
                    __slot,
                    __tagged #turbofish,
                    __candidate #turbofish,
                )
            },
        )
    } else {
        (support, handle)
    };

    // validated enums deserialize into an owned sink which is validated
    // once it finished
    let (validated_support, handle) = match container_attrs.validate() {
        Some(path) => {
            let validator = crate::de::validator(path);
            let slot_generics = crate::bound::with_slot_lifetime(&de_generics);
            let (slot_impl_generics, _, _) = slot_generics.split_for_impl();
            (
                quote! {
                    #[allow(clippy::type_complexity, clippy::multiple_bound_locations)]
                    fn __unvalidated #slot_impl_generics (
                        __slot: &'__s mut __deser::__derive::Option<#enum_ty>,
                    ) -> __deser::de::SinkHandle<'__s, 'de> #where_clause {
                        #handle
                    }
                },
                quote! {
                    __deser::__derive::validated_with(__slot, __unvalidated #turbofish, #validator)
                },
            )
        }
        None => (quote! {}, handle),
    };

    Ok(quote! {
        const _: () = {
            #(#helpers)*

            #type_name_const

            #support

            #candidate_support

            #validated_support

            #[automatically_derived]
            impl #impl_generics __deser::Deserialize<'de> for #ident #ty_generics #where_clause {
                fn deserialize_into(
                    __slot: &mut __deser::__derive::Option<Self>,
                ) -> __deser::de::SinkHandle<'_, 'de> {
                    #handle
                }
            }
        };
    })
}

/// Returns an `EnumKey` for the tag or content key of a tagged enum.
fn enum_key(name: &Name, aliases: &[Name]) -> TokenStream {
    quote! {
        __deser::__derive::EnumKey {
            name: #name,
            aliases: &[#(#aliases),*],
        }
    }
}

/// Builds a `FieldsSer` for the content fields of a struct variant.
fn fields_ser(
    info: &VariantInfo,
    container_attrs: &ContainerAttrs,
    tag: Option<(&Name, TokenStream)>,
) -> syn::Result<TokenStream> {
    let all_attrs = info
        .content_fields()
        .into_iter()
        .map(|field| Ok((field, FieldAttrs::of(field.field)?)))
        .collect::<syn::Result<Vec<_>>>()?;
    // variants with flattened fields merge the fields of the flattened
    // values when they are serialized
    let flatten = all_attrs
        .iter()
        .any(|(_, attrs)| attrs.flatten() && !attrs.skip_serializing());
    let field = |name: TokenStream, handle: TokenStream| {
        if flatten {
            quote! { __deser::__derive::FieldSer::Field(#name, #handle) }
        } else {
            quote! { (#name, #handle) }
        }
    };
    let tag_push = tag.map(|(tag, handle)| {
        let field = field(quote! { #tag }, handle);
        quote! {
            __fields.push(#field);
        }
    });
    let mut pushes = Vec::new();
    for (field_info, attrs) in &all_attrs {
        if attrs.skip_serializing() {
            continue;
        }
        let binding = &field_info.binding;
        let mut conditions = Vec::new();
        if let Some(path) = attrs.skip_serializing_if() {
            conditions.push(quote! { #path(#binding) });
        }
        let push = if attrs.flatten() {
            // the fields of flattened values are checked by `FlatFieldsSer`
            quote! {
                __fields.push(__deser::__derive::FieldSer::Flatten(#binding));
            }
        } else {
            if container_attrs.skip_serializing_optionals() {
                conditions.push(field_info.is_optional());
            }
            let name = attrs.variant_field_name(Direction::Serialize, info.fields_rename_all);
            let field = field(quote! { #name }, field_info.ser_handle());
            quote! {
                __fields.push(#field);
            }
        };
        pushes.push(if conditions.is_empty() {
            push
        } else {
            quote! {
                if !(#(#conditions)||*) {
                    #push
                }
            }
        });
    }
    let fields = if flatten {
        let skip_optionals = container_attrs.skip_serializing_optionals();
        quote! {
            __deser::__derive::FlatFieldsSer {
                fields: __fields,
                skip_optionals: #skip_optionals,
            }
        }
    } else {
        quote! { __deser::__derive::FieldsSer(__fields) }
    };
    Ok(quote! {
        {
            let mut __fields = __deser::__derive::Vec::new();
            #tag_push
            #(#pushes)*
            #fields
        }
    })
}

/// Returns a serialize handle for the content of a variant.
fn content_handle(
    info: &VariantInfo,
    container_attrs: &ContainerAttrs,
) -> syn::Result<TokenStream> {
    Ok(match info.content {
        Content::Unit => quote! { __deser::ser::SerializeHandle::to(&()) },
        Content::Newtype(idx) => info.fields[idx].ser_handle(),
        Content::Tuple(_) => {
            let handles = info.content_fields().into_iter().map(|x| x.ser_handle());
            quote! {
                __deser::ser::SerializeHandle::boxed(__deser::__derive::SeqSer(
                    __deser::__derive::Vec::from([#(#handles),*])
                ))
            }
        }
        Content::Struct(_) => {
            let fields = fields_ser(info, container_attrs, None)?;
            quote! { __deser::ser::SerializeHandle::boxed(#fields) }
        }
    })
}

pub fn derive_serialize(
    input: &syn::DeriveInput,
    enumeration: &syn::DataEnum,
    container_attrs: &ContainerAttrs,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let repr = repr(container_attrs);
    let variants = collect_variants(enumeration, container_attrs, Direction::Serialize)?;
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();
    let where_clause = where_clause_for_fields(
        &input.generics,
        quote!(__deser::Serialize),
        Some(quote!(__deser::__derive::Sync)),
        quote!(__deser::adapters::SerializeAs),
        None,
        container_attrs.serialize_bound(),
        &bound_fields(&variants, Direction::Serialize),
    );

    let type_name = container_attrs.container_name();
    // untagged variants of tagged enums are represented like the variants
    // of untagged enums
    let repr_of = |info: &VariantInfo| {
        if info.untagged { Repr::Untagged } else { repr }
    };
    let mut describe_arms = Vec::new();
    for info in &variants {
        let repr = repr_of(info);
        let repr_tokens = match repr {
            Repr::External => quote! { __deser::ser::VariantRepr::External },
            Repr::Internal { tag } => quote! { __deser::ser::VariantRepr::Internal { tag: #tag } },
            Repr::Adjacent { tag, content } => quote! {
                __deser::ser::VariantRepr::Adjacent { tag: #tag, content: #content }
            },
            Repr::Untagged => quote! { __deser::ser::VariantRepr::Untagged },
        };
        if info.skip_serializing {
            let pattern = info.wildcard_pattern(ident);
            describe_arms.push(quote! { #pattern => {} });
            continue;
        }
        let name = info.name.str_expr();
        let kind = match info.content {
            Content::Unit => quote! { __deser::ser::VariantKind::Unit },
            Content::Newtype(_) => quote! { __deser::ser::VariantKind::Newtype },
            Content::Tuple(_) => quote! { __deser::ser::VariantKind::Tuple },
            Content::Struct(_) => quote! { __deser::ser::VariantKind::Struct },
        };
        let describe_variant = quote! {
            __d.variant(&__deser::ser::Variant::new(#type_name, #name, #kind, #repr_tokens));
        };
        describe_arms.push(match (&repr, &info.content) {
            // untagged newtype variants serialize as their content and
            // internally tagged ones merge its fields with the tag, the
            // content describes itself as well
            (Repr::Untagged | Repr::Internal { .. }, Content::Newtype(idx)) => {
                let pattern = info.pattern(ident);
                let value = info.fields[*idx].ser_value();
                quote! {
                    #pattern => {
                        #describe_variant
                        __deser::ser::Serialize::describe(#value, __d);
                    }
                }
            }
            _ => {
                let pattern = info.wildcard_pattern(ident);
                quote! { #pattern => { #describe_variant } }
            }
        });
    }

    // the number of entries of the maps is fixed for these representations
    let len_of = |info: &VariantInfo| match repr_of(info) {
        Repr::External => Some(1usize),
        Repr::Adjacent { .. } if matches!(info.content, Content::Unit) => Some(1),
        Repr::Adjacent { .. } => Some(2),
        _ => None,
    };
    let container_shape = if variants.iter().all(|info| len_of(info) == Some(1)) {
        quote! { __deser::ContainerShape::new().with_len(1) }
    } else if variants.iter().all(|info| len_of(info).is_none()) {
        quote! { __deser::ContainerShape::new() }
    } else {
        let arms = variants.iter().map(|info| {
            let pattern = info.wildcard_pattern(ident);
            match len_of(info) {
                Some(len) => quote! { #pattern => __deser::ContainerShape::new().with_len(#len), },
                None => quote! { #pattern => __deser::ContainerShape::new(), },
            }
        });
        quote! { match *self { #(#arms)* } }
    };

    let mut arms = Vec::new();
    for info in &variants {
        if info.skip_serializing {
            let pattern = info.wildcard_pattern(ident);
            let variant = info.ident.to_string();
            arms.push(quote! {
                #pattern => {
                    return __deser::__derive::Err(
                        __deser::__derive::skipped_variant(#type_name, #variant)
                    );
                }
            });
            continue;
        }
        let pattern = info.pattern(ident);

        // the value of the tag, other variants can provide it with a field
        let tag_handle = match info.tag_field() {
            Some(field) => field.ser_handle(),
            None => info.name.ser_handle(),
        };
        let is_unit = matches!(info.content, Content::Unit);
        let chunk = match repr_of(info) {
            Repr::External if is_unit => match info.tag_field() {
                Some(_) => quote! { __deser::ser::Chunk::Forward(#tag_handle) },
                None => {
                    let atom = info.name.atom();
                    quote! { __deser::ser::Chunk::Atom(#atom) }
                }
            },
            Repr::External => {
                let content = content_handle(info, container_attrs)?;
                match (info.tag_field(), info.name.as_str()) {
                    (None, Some(name)) => quote! {
                        __deser::__derive::FieldsSer(__deser::__derive::Vec::from([(#name, #content)]))
                            .into_chunk()
                    },
                    _ => quote! {
                        __deser::__derive::EntrySer::new(#tag_handle, #content).into_chunk()
                    },
                }
            }
            Repr::Internal { tag } => match info.content {
                Content::Unit => quote! {
                    __deser::__derive::FieldsSer(__deser::__derive::Vec::from([(#tag, #tag_handle)]))
                        .into_chunk()
                },
                Content::Struct(_) => {
                    let fields = fields_ser(info, container_attrs, Some((tag, tag_handle)))?;
                    quote! { #fields.into_chunk() }
                }
                Content::Newtype(idx) => {
                    let inner = info.fields[idx].ser_value();
                    quote! {
                        __deser::__derive::TaggedNewtype::new(#tag, #tag_handle, #inner).into_chunk()
                    }
                }
                Content::Tuple(_) => unreachable!(),
            },
            Repr::Adjacent { tag, .. } if is_unit => quote! {
                __deser::__derive::FieldsSer(__deser::__derive::Vec::from([(#tag, #tag_handle)]))
                    .into_chunk()
            },
            Repr::Adjacent { tag, content } => {
                let content_handle = content_handle(info, container_attrs)?;
                quote! {
                    __deser::__derive::FieldsSer(__deser::__derive::Vec::from([
                        (#tag, #tag_handle),
                        (#content, #content_handle),
                    ]))
                    .into_chunk()
                }
            }
            Repr::Untagged => match info.content {
                Content::Unit => quote! { __deser::ser::Chunk::Atom(__deser::Atom::Null) },
                Content::Newtype(idx) => {
                    let value = info.fields[idx].ser_value();
                    quote! { __deser::ser::Serialize::serialize(#value, __state)? }
                }
                Content::Tuple(_) => {
                    let handles = info.content_fields().into_iter().map(|x| x.ser_handle());
                    quote! {
                        __deser::__derive::SeqSer(__deser::__derive::Vec::from([#(#handles),*]))
                            .into_chunk()
                    }
                }
                Content::Struct(_) => {
                    let fields = fields_ser(info, container_attrs, None)?;
                    quote! { #fields.into_chunk() }
                }
            },
        };
        arms.push(quote! { #pattern => #chunk, });
    }

    Ok(quote! {
        const _: () = {
            #[automatically_derived]
            impl #impl_generics __deser::Serialize for #ident #ty_generics #where_clause {
                fn describe(&self, __d: &mut dyn __deser::ser::Describe) {
                    match *self {
                        #(#describe_arms)*
                    }
                }

                fn container_shape(&self) -> __deser::ContainerShape {
                    #container_shape
                }

                fn serialize(
                    &self,
                    __state: &mut __deser::State,
                ) -> __deser::__derive::Result<__deser::ser::Chunk<'_>> {
                    __deser::__derive::Ok(match *self {
                        #(#arms)*
                    })
                }
            }
        };
    })
}
