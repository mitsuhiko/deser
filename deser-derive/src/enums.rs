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

use crate::attr::{ContainerAttrs, EnumVariantAttrs, FieldAttrs, UnnamedFieldAttrs};
use crate::bound::{BoundField, collect_idents, where_clause_for_fields};

#[derive(Copy, Clone)]
enum Repr<'a> {
    External,
    Internal { tag: &'a str },
    Adjacent { tag: &'a str, content: &'a str },
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
    adapter: Option<syn::Type>,
    tag: bool,
    binding: syn::Ident,
}

impl<'a> FieldInfo<'a> {
    fn ty(&self) -> &'a syn::Type {
        &self.field.ty
    }

    /// Returns the type the field is deserialized as.
    fn de_ty(&self) -> TokenStream {
        let ty = self.ty();
        match self.adapter {
            Some(ref adapter) => quote! { __deser::adapters::As<#ty, #adapter> },
            None => quote! { #ty },
        }
    }

    /// Converts a value of the type returned by `de_ty` into the field value.
    fn unwrap(&self, value: TokenStream) -> TokenStream {
        if self.adapter.is_some() {
            quote! { #value.into_inner() }
        } else {
            value
        }
    }

    /// Returns a serialize handle for the bound field.
    fn ser_handle(&self) -> TokenStream {
        let binding = &self.binding;
        crate::ser::serialize_handle(self.ty(), self.adapter.as_ref(), quote! { #binding })
    }

    /// Returns a reference to a serializable for the bound field.
    fn ser_value(&self) -> TokenStream {
        let binding = &self.binding;
        let ty = self.ty();
        match self.adapter {
            Some(ref adapter) => quote! {
                __deser::adapters::SerializeAsRef::<#adapter, #ty>::new(#binding)
            },
            None => quote! { #binding },
        }
    }

    /// Returns an expression that checks if the bound field is optional.
    fn is_optional(&self) -> TokenStream {
        let binding = &self.binding;
        crate::ser::is_optional(self.ty(), self.adapter.as_ref(), quote! { #binding })
    }
}

struct VariantInfo<'a> {
    ident: &'a syn::Ident,
    name: String,
    names: Vec<String>,
    other: bool,
    default: bool,
    shape: Shape,
    fields: Vec<FieldInfo<'a>>,
    tag_field: Option<usize>,
    content: Content,
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

    fn tag_field(&self) -> Option<&FieldInfo<'a>> {
        self.tag_field.map(|idx| &self.fields[idx])
    }
}

/// Returns the type parameters of the generics that appear in the types.
fn used_type_params<'a>(
    generics: &'a syn::Generics,
    types: &[TokenStream],
) -> Vec<&'a syn::TypeParam> {
    let mut idents = HashSet::new();
    for ty in types {
        collect_idents(ty.clone(), &mut idents);
    }
    generics
        .type_params()
        .filter(|param| idents.contains(&param.ident.to_string()))
        .collect()
}

/// Returns the predicates which only refer to the given type parameters.
fn filter_predicates<'a>(
    generics: &syn::Generics,
    predicates: impl IntoIterator<Item = &'a syn::WherePredicate>,
    params: &[&syn::TypeParam],
) -> Vec<&'a syn::WherePredicate> {
    let allowed = params
        .iter()
        .map(|x| x.ident.to_string())
        .collect::<HashSet<_>>();
    predicates
        .into_iter()
        .filter(|predicate| {
            let mut idents = HashSet::new();
            collect_idents(quote! { #predicate }, &mut idents);
            generics
                .type_params()
                .map(|x| x.ident.to_string())
                .filter(|x| idents.contains(x))
                .all(|x| allowed.contains(&x))
        })
        .collect()
}

/// Returns the where clause predicates of the generics which only refer to
/// the given type parameters.
fn helper_where_clause(generics: &syn::Generics, params: &[&syn::TypeParam]) -> TokenStream {
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

fn check_generics(generics: &syn::Generics) -> syn::Result<()> {
    if let Some(lifetime) = generics.lifetimes().next() {
        return Err(syn::Error::new_spanned(
            lifetime,
            "enums with lifetime parameters are not supported",
        ));
    }
    if let Some(param) = generics.const_params().next() {
        return Err(syn::Error::new_spanned(
            param,
            "enums with const parameters are not supported",
        ));
    }
    Ok(())
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
        || enumeration
            .variants
            .iter()
            .any(|x| !matches!(x.fields, syn::Fields::Unit))
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
                    adapter: attrs.adapter().cloned(),
                    tag: attrs.tag(),
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
                    adapter: attrs.adapter().cloned(),
                    tag: attrs.tag(),
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

fn collect_variants<'a>(
    enumeration: &'a syn::DataEnum,
    container_attrs: &ContainerAttrs,
) -> syn::Result<Vec<VariantInfo<'a>>> {
    let mut rv = Vec::new();
    let mut seen_names = HashSet::new();
    let mut seen_other = false;
    let mut seen_default = false;
    let repr = repr(container_attrs);

    for variant in &enumeration.variants {
        let attrs = EnumVariantAttrs::of(variant)?;
        let name = attrs.name(container_attrs).to_string();
        let mut names = Vec::new();
        for name in std::iter::once(name.clone()).chain(attrs.aliases().iter().cloned()) {
            if !seen_names.insert(name.clone()) {
                return Err(syn::Error::new_spanned(
                    variant,
                    format!("variant name '{}' used more than once", name),
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
        let content = match shape {
            Shape::Unit => Content::Unit,
            Shape::Tuple => match content_idxs.len() {
                0 => Content::Unit,
                1 => Content::Newtype(content_idxs[0]),
                _ => Content::Tuple(content_idxs),
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
            ident: &variant.ident,
            name,
            names,
            other: attrs.other(),
            default: attrs.default(),
            shape,
            fields,
            tag_field,
            content,
        });
    }

    Ok(rv)
}

/// Defines the name of the type which is used in error messages.
fn type_name_const(container_attrs: &ContainerAttrs) -> TokenStream {
    let type_name = container_attrs.container_name();
    quote! {
        const __TYPE_NAME: &__deser::__derive::str = #type_name;
    }
}

/// Returns the fields of all variants for the purpose of bound inference.
fn bound_fields<'b>(variants: &'b [VariantInfo]) -> Vec<BoundField<'b>> {
    variants
        .iter()
        .flat_map(|info| info.fields.iter())
        .map(|field| BoundField {
            ty: field.ty(),
            adapter: field.adapter.as_ref(),
        })
        .collect()
}

pub fn derive_deserialize(
    input: &syn::DeriveInput,
    enumeration: &syn::DataEnum,
    container_attrs: &ContainerAttrs,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    check_generics(&input.generics)?;
    let repr = repr(container_attrs);
    let variants = collect_variants(enumeration, container_attrs)?;
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let de_generics = crate::bound::with_de_lifetime(&input.generics)?;
    let (impl_generics, _, _) = de_generics.split_for_impl();
    let turbofish = ty_generics.as_turbofish();
    let mut where_clause = where_clause_for_fields(
        &input.generics,
        quote!(__deser::Deserialize<'de> + 'static),
        Some(quote!('static)),
        quote!(__deser::adapters::DeserializeAs),
        Some(quote!('de)),
        container_attrs.deserialize_bound(),
        &bound_fields(&variants),
    );
    // the deserializer boxes variant builders so the type parameters must
    // be 'static even with custom bounds.
    if container_attrs.deserialize_bound().is_some() {
        for param in input.generics.type_params() {
            let param = &param.ident;
            where_clause
                .predicates
                .push(syn::parse_quote!(#param: 'static));
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
            Content::Unit => matches!(repr, Repr::Internal { .. }) && !info.other,
            _ => false,
        };
        // helper structs only take the type parameters they use
        let helper_params = match info.content {
            Content::Struct(_) => used_type_params(
                &input.generics,
                &content_fields
                    .iter()
                    .map(|x| {
                        let ty = x.ty();
                        let adapter = &x.adapter;
                        quote! { #ty #adapter }
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => Vec::new(),
        };
        let helper_args = helper_params.iter().map(|x| &x.ident).collect::<Vec<_>>();
        let helper_ty = if helper_args.is_empty() {
            quote! { #helper }
        } else {
            quote! { #helper<#(#helper_args),*> }
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
            // the helper needs the same bounds on its parameters as the enum,
            // plus `'static` as the enum's deserialize impl requires it.
            let helper_decl = if helper_params.is_empty() {
                quote! { #helper }
            } else {
                let params = helper_params.iter().map(|param| {
                    let ident = &param.ident;
                    let bounds = param.bounds.iter();
                    quote! { #ident: 'static #(+ #bounds)* }
                });
                quote! { #helper<#(#params),*> }
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
            helpers.push(quote! {
                #[derive(__deser::Deserialize)]
                #[deser(rename = #helper_name)]
                #helper_crate
                #helper_bound
                struct #helper_decl #helper_where {
                    #(#fields)*
                }
            });
        }

        // the content is deserialized as a single value which is bound to a
        // pattern that makes the values of the content fields available.
        let mut values = vec![TokenStream::new(); info.fields.len()];
        let (content_ty, content_pattern) = match info.content {
            Content::Unit if needs_helper => (helper_ty.clone(), quote! { _ }),
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
        __deser::__derive::BoxedVariant<'de, #enum_ty>
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
                    fn #fn_ident #impl_generics () -> #builder_ty #where_clause {
                        #builder
                    }
                },
                quote! {
                    __deser::__derive::Some(
                        #fn_ident #turbofish as __deser::__derive::VariantMaker<'de, #enum_ty>
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
            .filter(|(info, _)| !info.other)
            .map(|(info, builder)| {
                let names = &info.names;
                quote! { #(#names)|* => __deser::__derive::Some(#builder), }
            });
        let (other_fn, other) = special_variant("__other", |info| info.other);
        let (default_fn, default) = special_variant("__default", |info| info.default);
        (
            quote! {
                #[allow(clippy::type_complexity, clippy::multiple_bound_locations)]
                fn __lookup #impl_generics (
                    __tag: &__deser::__derive::str,
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
                }
            },
        )
    };

    let (support, handle) = match repr {
        Repr::External => {
            let unit_arms = variants
                .iter()
                .filter(|info| matches!(info.content, Content::Unit) && !info.other)
                .map(|info| {
                    let names = &info.names;
                    let construct = info.construct(ident, &[]);
                    quote! { #(#names)|* => __deser::__derive::Some(#construct), }
                });
            let (table_support, table) = variants_table;
            (
                quote! {
                    #table_support

                    #[allow(clippy::multiple_bound_locations)]
                    fn __unit #impl_generics (
                        __name: &__deser::__derive::str,
                    ) -> __deser::__derive::Option<#enum_ty> #where_clause {
                        match __name {
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
            let (table_support, table) = variants_table;
            (
                table_support,
                quote! {
                    __deser::__derive::InternallyTaggedSink::handle(
                        __slot,
                        #tag,
                        __TYPE_NAME,
                        #table,
                    )
                },
            )
        }
        Repr::Adjacent { tag, content } => {
            let (table_support, table) = variants_table;
            (
                table_support,
                quote! {
                    __deser::__derive::AdjacentlyTaggedSink::handle(
                        __slot,
                        #tag,
                        #content,
                        __TYPE_NAME,
                        #table,
                    )
                },
            )
        }
        Repr::Untagged => {
            let indexes = 0..builders.len();
            (
                quote! {
                    #[allow(clippy::type_complexity, clippy::multiple_bound_locations)]
                    fn __candidate #impl_generics (
                        __index: usize,
                    ) -> __deser::__derive::Option<#builder_ty> #where_clause {
                        match __index {
                            #(#indexes => __deser::__derive::Some(#builders),)*
                            _ => __deser::__derive::None,
                        }
                    }
                },
                quote! {
                    __deser::__derive::untagged_handle(__slot, __TYPE_NAME, __candidate #turbofish)
                },
            )
        }
    };

    Ok(quote! {
        const _: () = {
            #(#helpers)*

            #type_name_const

            #support

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

/// Builds a `FieldsSer` for the content fields of a struct variant.
fn fields_ser(
    info: &VariantInfo,
    container_attrs: &ContainerAttrs,
    tag: Option<(&str, TokenStream)>,
) -> syn::Result<TokenStream> {
    let tag_push = tag.map(|(tag, handle)| {
        quote! {
            __fields.push((#tag, #handle));
        }
    });
    let mut pushes = Vec::new();
    for field in info.content_fields() {
        let attrs = FieldAttrs::of(field.field)?;
        if attrs.flatten() {
            return Err(syn::Error::new_spanned(
                field.field,
                "flatten is not supported in enum variants",
            ));
        }
        let name = attrs.plain_name().to_string();
        let binding = &field.binding;
        let mut conditions = Vec::new();
        if let Some(path) = attrs.skip_serializing_if() {
            conditions.push(quote! { #path(#binding) });
        }
        if container_attrs.skip_serializing_optionals() {
            conditions.push(field.is_optional());
        }
        let handle = field.ser_handle();
        let push = quote! {
            __fields.push((#name, #handle));
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
    Ok(quote! {
        {
            let mut __fields = __deser::__derive::Vec::new();
            #tag_push
            #(#pushes)*
            __deser::__derive::FieldsSer(__fields)
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
    check_generics(&input.generics)?;
    let repr = repr(container_attrs);
    let variants = collect_variants(enumeration, container_attrs)?;
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();
    let where_clause = where_clause_for_fields(
        &input.generics,
        quote!(__deser::Serialize),
        None,
        quote!(__deser::adapters::SerializeAs),
        None,
        container_attrs.serialize_bound(),
        &bound_fields(&variants),
    );

    let type_name = container_attrs.container_name();
    let repr_tokens = match repr {
        Repr::External => quote! { __deser::ser::VariantRepr::External },
        Repr::Internal { tag } => quote! { __deser::ser::VariantRepr::Internal { tag: #tag } },
        Repr::Adjacent { tag, content } => quote! {
            __deser::ser::VariantRepr::Adjacent { tag: #tag, content: #content }
        },
        Repr::Untagged => quote! { __deser::ser::VariantRepr::Untagged },
    };
    let mut describe_arms = Vec::new();
    for info in &variants {
        let name = &info.name;
        let var_ident = info.ident;
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
                let pattern = match info.shape {
                    Shape::Unit => quote! { #ident::#var_ident },
                    Shape::Tuple => quote! { #ident::#var_ident(..) },
                    Shape::Named => quote! { #ident::#var_ident { .. } },
                };
                quote! { #pattern => { #describe_variant } }
            }
        });
    }

    let mut arms = Vec::new();
    for info in &variants {
        let name = &info.name;
        let pattern = info.pattern(ident);

        // the value of the tag, other variants can provide it with a field
        let tag_handle = match info.tag_field() {
            Some(field) => field.ser_handle(),
            None => quote! { __deser::ser::SerializeHandle::to(&#name) },
        };
        let is_unit = matches!(info.content, Content::Unit);
        let chunk = match repr {
            Repr::External if is_unit => match info.tag_field() {
                Some(_) => quote! { __deser::ser::Chunk::Forward(#tag_handle) },
                None => quote! {
                    __deser::ser::Chunk::Atom(__deser::Atom::Str(__deser::__derive::Cow::Borrowed(#name)))
                },
            },
            Repr::External => {
                let content = content_handle(info, container_attrs)?;
                match info.tag_field() {
                    Some(_) => quote! {
                        __deser::__derive::EntrySer::new(#tag_handle, #content).into_chunk()
                    },
                    None => quote! {
                        __deser::__derive::FieldsSer(__deser::__derive::Vec::from([(#name, #content)]))
                            .into_chunk()
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
