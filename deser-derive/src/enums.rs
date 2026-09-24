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
use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};
use quote::quote;

use crate::attr::{ContainerAttrs, EnumVariantAttrs, FieldAttrs};
use crate::bound::where_clause_with_bound;

#[derive(Copy, Clone)]
enum Repr<'a> {
    External,
    Internal { tag: &'a str },
    Adjacent { tag: &'a str, content: &'a str },
    Untagged,
}

enum Kind<'a> {
    Unit,
    Newtype(&'a syn::Type),
    Tuple(Vec<&'a syn::Type>),
    Struct(&'a syn::FieldsNamed),
}

struct VariantInfo<'a> {
    ident: &'a syn::Ident,
    name: String,
    names: Vec<String>,
    other: bool,
    kind: Kind<'a>,
}

impl<'a> VariantInfo<'a> {
    fn helper(&self) -> syn::Ident {
        syn::Ident::new(&format!("__Variant{}", self.ident), Span::call_site())
    }

    fn bindings(&self) -> Vec<syn::Ident> {
        match self.kind {
            Kind::Unit => Vec::new(),
            Kind::Newtype(_) => vec![syn::Ident::new("__f0", Span::call_site())],
            Kind::Tuple(ref types) => (0..types.len())
                .map(|idx| syn::Ident::new(&format!("__f{}", idx), Span::call_site()))
                .collect(),
            Kind::Struct(fields) => fields
                .named
                .iter()
                .map(|field| {
                    syn::Ident::new(
                        &format!("__field_{}", field.ident.as_ref().unwrap()),
                        Span::call_site(),
                    )
                })
                .collect(),
        }
    }
}

/// Returns the type parameters of the generics that appear in the types.
fn used_type_params<'a>(
    generics: &'a syn::Generics,
    types: &[&syn::Type],
) -> Vec<&'a syn::TypeParam> {
    let mut idents = HashSet::new();
    for ty in types {
        collect_idents(quote! { #ty }, &mut idents);
    }
    generics
        .type_params()
        .filter(|param| idents.contains(&param.ident.to_string()))
        .collect()
}

/// Returns the where clause predicates of the generics which only refer to
/// the given type parameters.
fn helper_where_clause(generics: &syn::Generics, params: &[&syn::TypeParam]) -> TokenStream {
    let where_clause = match generics.where_clause {
        Some(ref where_clause) => where_clause,
        None => return TokenStream::new(),
    };
    let allowed = params
        .iter()
        .map(|x| x.ident.to_string())
        .collect::<HashSet<_>>();
    let predicates = where_clause
        .predicates
        .iter()
        .filter(|predicate| {
            let mut idents = HashSet::new();
            collect_idents(quote! { #predicate }, &mut idents);
            generics
                .type_params()
                .map(|x| x.ident.to_string())
                .filter(|x| idents.contains(x))
                .all(|x| allowed.contains(&x))
        })
        .collect::<Vec<_>>();
    if predicates.is_empty() {
        TokenStream::new()
    } else {
        quote! { where #(#predicates),* }
    }
}

fn collect_idents(stream: TokenStream, out: &mut HashSet<String>) {
    for token in stream {
        match token {
            proc_macro2::TokenTree::Ident(ident) => {
                out.insert(ident.to_string());
            }
            proc_macro2::TokenTree::Group(group) => collect_idents(group.stream(), out),
            _ => {}
        }
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

fn collect_variants<'a>(
    enumeration: &'a syn::DataEnum,
    container_attrs: &ContainerAttrs,
) -> syn::Result<Vec<VariantInfo<'a>>> {
    let mut rv = Vec::new();
    let mut seen_names = HashSet::new();
    let mut seen_other = false;
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

        let kind = match variant.fields {
            syn::Fields::Unit => Kind::Unit,
            syn::Fields::Unnamed(ref fields) if fields.unnamed.len() == 1 => {
                Kind::Newtype(&fields.unnamed[0].ty)
            }
            syn::Fields::Unnamed(ref fields) => {
                if matches!(repr, Repr::Internal { .. }) {
                    return Err(syn::Error::new_spanned(
                        variant,
                        "internally tagged enums do not support tuple variants",
                    ));
                }
                Kind::Tuple(fields.unnamed.iter().map(|x| &x.ty).collect())
            }
            syn::Fields::Named(ref fields) => Kind::Struct(fields),
        };

        for field in variant.fields.iter() {
            if !matches!(kind, Kind::Struct(_)) {
                crate::attr::ensure_no_field_attrs(field)?;
            } else {
                // validate the attributes early for better errors
                FieldAttrs::of(field)?;
            }
        }

        rv.push(VariantInfo {
            ident: &variant.ident,
            name,
            names,
            other: attrs.other(),
            kind,
        });
    }

    Ok(rv)
}

fn descriptor(container_attrs: &ContainerAttrs) -> TokenStream {
    let type_name = container_attrs.container_name();
    quote! {
        struct __Descriptor;

        impl ::deser::Descriptor for __Descriptor {
            fn name(&self) -> ::deser::__derive::Option<&::deser::__derive::str> {
                ::deser::__derive::Some(#type_name)
            }
        }
    }
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
    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();
    let turbofish = ty_generics.as_turbofish();
    let where_clause =
        where_clause_with_bound(&input.generics, quote!(::deser::Deserialize + 'static));
    let enum_ty = quote! { #ident #ty_generics };

    let mut helpers = Vec::new();
    let mut builders = Vec::new();
    for info in &variants {
        let var_ident = info.ident;
        let helper = info.helper();

        // struct variants (and unit variants of internally tagged enums which
        // are maps) are deserialized through a helper struct that uses the
        // regular struct derive.
        let needs_helper = match info.kind {
            Kind::Struct(_) => true,
            Kind::Unit => matches!(repr, Repr::Internal { .. }) && !info.other,
            _ => false,
        };
        // helper structs only take the type parameters they use
        let helper_params = match info.kind {
            Kind::Struct(fields) => used_type_params(
                &input.generics,
                &fields.named.iter().map(|x| &x.ty).collect::<Vec<_>>(),
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
            let fields = match info.kind {
                Kind::Struct(fields) => fields
                    .named
                    .iter()
                    .map(|field| {
                        let deser_attrs = field.attrs.iter().filter(|x| x.path.is_ident("deser"));
                        let name = &field.ident;
                        let ty = &field.ty;
                        quote! { #(#deser_attrs)* #name: #ty, }
                    })
                    .collect(),
                _ => Vec::new(),
            };
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
            helpers.push(quote! {
                #[derive(::deser::Deserialize)]
                #[deser(rename = #helper_name)]
                struct #helper_decl #helper_where {
                    #(#fields)*
                }
            });
        }

        let builder = if info.other {
            quote! { ::deser::__derive::IgnoredVariant::<#enum_ty>::boxed(|| #ident::#var_ident) }
        } else {
            match info.kind {
                Kind::Struct(fields) => {
                    let names = fields.named.iter().map(|x| &x.ident).collect::<Vec<_>>();
                    quote! {
                        ::deser::__derive::Variant::<#helper_ty, #enum_ty>::boxed(
                            |__v: #helper_ty| #ident::#var_ident { #(#names: __v.#names,)* }
                        )
                    }
                }
                Kind::Unit if needs_helper => quote! {
                    ::deser::__derive::Variant::<#helper_ty, #enum_ty>::boxed(|_: #helper_ty| #ident::#var_ident)
                },
                Kind::Unit => quote! {
                    ::deser::__derive::Variant::<(), #enum_ty>::boxed(|_: ()| #ident::#var_ident)
                },
                Kind::Newtype(ty) => quote! {
                    ::deser::__derive::Variant::<#ty, #enum_ty>::boxed(#ident::#var_ident)
                },
                Kind::Tuple(ref types) => {
                    let bindings = info.bindings();
                    quote! {
                        ::deser::__derive::Variant::<(#(#types,)*), #enum_ty>::boxed(
                            |(#(#bindings,)*): (#(#types,)*)| #ident::#var_ident(#(#bindings),*)
                        )
                    }
                }
            }
        };
        builders.push(builder);
    }

    let descriptor = descriptor(container_attrs);
    let other_builder = variants
        .iter()
        .zip(builders.iter())
        .find(|(info, _)| info.other)
        .map(|(_, builder)| quote! { ::deser::__derive::Some(#builder) })
        .unwrap_or_else(|| quote! { ::deser::__derive::None });

    let lookup = {
        let arms = variants.iter().zip(builders.iter()).map(|(info, builder)| {
            let names = &info.names;
            quote! { #(#names)|* => ::deser::__derive::Some(#builder), }
        });
        quote! {
            #[allow(clippy::type_complexity)]
            fn __lookup #impl_generics (
                __tag: &::deser::__derive::str,
            ) -> ::deser::__derive::Option<
                ::deser::__derive::Box<dyn ::deser::__derive::VariantBuilder<#enum_ty>>,
            > #where_clause {
                match __tag {
                    #(#arms)*
                    _ => #other_builder,
                }
            }
        }
    };

    let (support, handle) = match repr {
        Repr::External => {
            let unit_arms = variants
                .iter()
                .filter(|info| matches!(info.kind, Kind::Unit))
                .map(|info| {
                    let names = &info.names;
                    let var_ident = info.ident;
                    quote! { #(#names)|* => ::deser::__derive::Some(#ident::#var_ident), }
                });
            let unit_other = variants
                .iter()
                .find(|info| info.other)
                .map(|info| {
                    let var_ident = info.ident;
                    quote! { ::deser::__derive::Some(#ident::#var_ident) }
                })
                .unwrap_or_else(|| quote! { ::deser::__derive::None });
            (
                quote! {
                    #lookup

                    fn __unit #impl_generics (
                        __name: &::deser::__derive::str,
                    ) -> ::deser::__derive::Option<#enum_ty> #where_clause {
                        match __name {
                            #(#unit_arms)*
                            _ => #unit_other,
                        }
                    }
                },
                quote! {
                    ::deser::__derive::ExternallyTaggedSink::handle(
                        __slot,
                        &__Descriptor,
                        __lookup #turbofish,
                        __unit #turbofish,
                    )
                },
            )
        }
        Repr::Internal { tag } => (
            lookup,
            quote! {
                ::deser::__derive::InternallyTaggedSink::handle(
                    __slot,
                    #tag,
                    &__Descriptor,
                    __lookup #turbofish,
                )
            },
        ),
        Repr::Adjacent { tag, content } => (
            lookup,
            quote! {
                ::deser::__derive::AdjacentlyTaggedSink::handle(
                    __slot,
                    #tag,
                    #content,
                    &__Descriptor,
                    __lookup #turbofish,
                )
            },
        ),
        Repr::Untagged => {
            let indexes = 0..builders.len();
            (
                quote! {
                    #[allow(clippy::type_complexity)]
                    fn __candidate #impl_generics (
                        __index: usize,
                    ) -> ::deser::__derive::Option<
                        ::deser::__derive::Box<dyn ::deser::__derive::VariantBuilder<#enum_ty>>,
                    > #where_clause {
                        match __index {
                            #(#indexes => ::deser::__derive::Some(#builders),)*
                            _ => ::deser::__derive::None,
                        }
                    }
                },
                quote! {
                    ::deser::__derive::untagged_handle(__slot, &__Descriptor, __candidate #turbofish)
                },
            )
        }
    };

    Ok(quote! {
        const _: () = {
            #(#helpers)*

            #descriptor

            #support

            #[automatically_derived]
            impl #impl_generics ::deser::Deserialize for #ident #ty_generics #where_clause {
                fn deserialize_into(
                    __slot: &mut ::deser::__derive::Option<Self>,
                ) -> ::deser::de::SinkHandle<'_> {
                    #handle
                }
            }
        };
    })
}

/// Builds a `FieldsSer` for the fields of a struct variant.
fn fields_ser(
    info: &VariantInfo,
    container_attrs: &ContainerAttrs,
    tag: Option<(&str, &str)>,
) -> syn::Result<TokenStream> {
    let fields = match info.kind {
        Kind::Struct(fields) => fields,
        _ => unreachable!(),
    };
    let tag_push = tag.map(|(tag, name)| {
        quote! {
            __fields.push((#tag, ::deser::ser::SerializeHandle::to(&#name)));
        }
    });
    let mut pushes = Vec::new();
    for (field, binding) in fields.named.iter().zip(info.bindings()) {
        let attrs = FieldAttrs::of(field)?;
        if attrs.flatten() {
            return Err(syn::Error::new_spanned(
                field,
                "flatten is not supported in enum variants",
            ));
        }
        let name = attrs.plain_name().to_string();
        let mut conditions = Vec::new();
        if let Some(path) = attrs.skip_serializing_if() {
            conditions.push(quote! { #path(#binding) });
        }
        if container_attrs.skip_serializing_optionals() {
            conditions.push(quote! { ::deser::ser::Serialize::is_optional(#binding) });
        }
        let push = quote! {
            __fields.push((#name, ::deser::ser::SerializeHandle::to(#binding)));
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
            let mut __fields = ::deser::__derive::Vec::new();
            #tag_push
            #(#pushes)*
            ::deser::__derive::FieldsSer(__fields)
        }
    })
}

/// Returns a serialize handle for the content of a variant.
fn content_handle(
    info: &VariantInfo,
    container_attrs: &ContainerAttrs,
) -> syn::Result<TokenStream> {
    let bindings = info.bindings();
    Ok(match info.kind {
        Kind::Unit => quote! { ::deser::ser::SerializeHandle::to(&()) },
        Kind::Newtype(_) => quote! { ::deser::ser::SerializeHandle::to(__f0) },
        Kind::Tuple(_) => quote! {
            ::deser::ser::SerializeHandle::boxed(::deser::__derive::SeqSer(
                ::deser::__derive::Vec::from([
                    #(::deser::ser::SerializeHandle::to(#bindings)),*
                ])
            ))
        },
        Kind::Struct(_) => {
            let fields = fields_ser(info, container_attrs, None)?;
            quote! { ::deser::ser::SerializeHandle::boxed(#fields) }
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
    let where_clause = where_clause_with_bound(&input.generics, quote!(::deser::Serialize));

    let mut arms = Vec::new();
    for info in &variants {
        let var_ident = info.ident;
        let name = &info.name;
        let bindings = info.bindings();
        let pattern = match info.kind {
            Kind::Unit => quote! { #ident::#var_ident },
            Kind::Newtype(_) | Kind::Tuple(_) => quote! { #ident::#var_ident(#(ref #bindings),*) },
            Kind::Struct(fields) => {
                let names = fields.named.iter().map(|x| &x.ident);
                quote! { #ident::#var_ident { #(#names: ref #bindings),* } }
            }
        };

        let tag_entry = |tag: &str| {
            quote! { (#tag, ::deser::ser::SerializeHandle::to(&#name)) }
        };
        let chunk = match (repr, &info.kind) {
            (Repr::External, Kind::Unit) => quote! {
                ::deser::ser::Chunk::Atom(::deser::Atom::Str(::deser::__derive::Cow::Borrowed(#name)))
            },
            (Repr::External, _) => {
                let content = content_handle(info, container_attrs)?;
                quote! {
                    ::deser::__derive::FieldsSer(::deser::__derive::Vec::from([(#name, #content)]))
                        .into_chunk()
                }
            }
            (Repr::Internal { tag }, Kind::Unit) => {
                let tag_entry = tag_entry(tag);
                quote! {
                    ::deser::__derive::FieldsSer(::deser::__derive::Vec::from([#tag_entry]))
                        .into_chunk()
                }
            }
            (Repr::Internal { tag }, Kind::Struct(_)) => {
                let fields = fields_ser(info, container_attrs, Some((tag, name)))?;
                quote! { #fields.into_chunk() }
            }
            (Repr::Internal { tag }, Kind::Newtype(_)) => quote! {
                ::deser::__derive::TaggedNewtype::new(#tag, #name, __f0).into_chunk()
            },
            (Repr::Internal { .. }, Kind::Tuple(_)) => unreachable!(),
            (Repr::Adjacent { tag, .. }, Kind::Unit) => {
                let tag_entry = tag_entry(tag);
                quote! {
                    ::deser::__derive::FieldsSer(::deser::__derive::Vec::from([#tag_entry]))
                        .into_chunk()
                }
            }
            (Repr::Adjacent { tag, content }, _) => {
                let tag_entry = tag_entry(tag);
                let content_handle = content_handle(info, container_attrs)?;
                quote! {
                    ::deser::__derive::FieldsSer(::deser::__derive::Vec::from([
                        #tag_entry,
                        (#content, #content_handle),
                    ]))
                    .into_chunk()
                }
            }
            (Repr::Untagged, Kind::Unit) => {
                quote! { ::deser::ser::Chunk::Atom(::deser::Atom::Null) }
            }
            (Repr::Untagged, Kind::Newtype(_)) => quote! {
                ::deser::ser::Serialize::serialize(__f0, __state)?
            },
            (Repr::Untagged, Kind::Tuple(_)) => quote! {
                ::deser::__derive::SeqSer(::deser::__derive::Vec::from([
                    #(::deser::ser::SerializeHandle::to(#bindings)),*
                ]))
                .into_chunk()
            },
            (Repr::Untagged, Kind::Struct(_)) => {
                let fields = fields_ser(info, container_attrs, None)?;
                quote! { #fields.into_chunk() }
            }
        };
        arms.push(quote! { #pattern => #chunk, });
    }

    let descriptor = descriptor(container_attrs);

    Ok(quote! {
        const _: () = {
            #descriptor

            #[automatically_derived]
            impl #impl_generics ::deser::Serialize for #ident #ty_generics #where_clause {
                fn descriptor(&self) -> &dyn ::deser::Descriptor {
                    &__Descriptor
                }

                fn serialize(
                    &self,
                    __state: &mut ::deser::ser::SerializerState,
                ) -> ::deser::__derive::Result<::deser::ser::Chunk<'_>> {
                    ::deser::__derive::Ok(match *self {
                        #(#arms)*
                    })
                }
            }
        };
    })
}
