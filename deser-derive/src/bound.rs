use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};

pub fn with_lifetime_bound(generics: &syn::Generics, lifetime: &str) -> syn::Generics {
    let bound = syn::Lifetime::new(lifetime, Span::call_site());
    let def = syn::LifetimeParam {
        attrs: Vec::new(),
        lifetime: bound.clone(),
        colon_token: None,
        bounds: syn::punctuated::Punctuated::new(),
    };

    let params = Some(syn::GenericParam::Lifetime(def))
        .into_iter()
        .chain(generics.params.iter().cloned().map(|mut param| {
            match &mut param {
                syn::GenericParam::Lifetime(param) => {
                    param.bounds.push(bound.clone());
                }
                syn::GenericParam::Type(param) => {
                    param
                        .bounds
                        .push(syn::TypeParamBound::Lifetime(bound.clone()));
                }
                syn::GenericParam::Const(_) => {}
            }
            param
        }))
        .collect();

    syn::Generics {
        params,
        ..generics.clone()
    }
}

/// Adds the `'de` lifetime of `Deserialize` to the generics.
///
/// All lifetimes of the type are bounded by `'de` so that borrowed data can
/// be deserialized into them.
pub fn with_de_lifetime(generics: &syn::Generics) -> syn::Result<syn::Generics> {
    if let Some(lifetime) = generics.lifetimes().find(|x| x.lifetime.ident == "de") {
        return Err(syn::Error::new_spanned(
            lifetime,
            "cannot derive Deserialize for types with a lifetime named 'de, \
             it's used by the derive",
        ));
    }
    let bounds = generics
        .lifetimes()
        .map(|x| x.lifetime.clone())
        .collect::<syn::punctuated::Punctuated<_, syn::Token![+]>>();
    let def = syn::LifetimeParam {
        attrs: Vec::new(),
        lifetime: syn::Lifetime::new("'de", Span::call_site()),
        colon_token: if bounds.is_empty() {
            None
        } else {
            Some(Default::default())
        },
        bounds,
    };
    let mut rv = generics.clone();
    rv.params.insert(0, syn::GenericParam::Lifetime(def));
    Ok(rv)
}

/// Returns the where clause of the generics with added bounds.
///
/// If `custom` is `None` every type parameter gets the given bound.
/// Otherwise the custom predicates are added instead.
pub fn where_clause_with_bound(
    generics: &syn::Generics,
    bound: TokenStream,
    custom: Option<&[syn::WherePredicate]>,
) -> syn::WhereClause {
    let new_predicates: Vec<syn::WherePredicate> = match custom {
        Some(custom) => custom.to_vec(),
        None => generics
            .type_params()
            .map(|param| {
                let param = &param.ident;
                syn::parse_quote!(#param : #bound)
            })
            .collect(),
    };

    let mut generics = generics.clone();
    generics
        .make_where_clause()
        .predicates
        .extend(new_predicates);
    generics.where_clause.unwrap()
}

/// Collects all identifiers in a token stream.
pub fn collect_idents(stream: TokenStream, out: &mut HashSet<String>) {
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

/// A field for the purpose of bound inference.
pub struct BoundField<'a> {
    pub ty: &'a syn::Type,
    pub adapter: Option<&'a syn::Type>,
}

/// Returns the where clause of the generics with bounds inferred from fields.
///
/// If `custom` is `Some` the custom predicates are used instead, otherwise
/// every type parameter gets `bound` unless it only appears in fields with
/// adapters.  Such parameters get `adapter_only_bound` if provided.  For
/// fields with adapters that refer to type parameters a predicate that
/// requires the adapter to implement `adapter_trait` for the field type is
/// added.
pub fn where_clause_for_fields(
    generics: &syn::Generics,
    bound: TokenStream,
    adapter_only_bound: Option<TokenStream>,
    adapter_trait: TokenStream,
    adapter_lifetime: Option<TokenStream>,
    custom: Option<&[syn::WherePredicate]>,
    fields: &[BoundField<'_>],
) -> syn::WhereClause {
    if custom.is_some() || fields.iter().all(|x| x.adapter.is_none()) {
        return where_clause_with_bound(generics, bound, custom);
    }

    let mut plain = HashSet::new();
    let mut adapted = HashSet::new();
    for field in fields {
        let ty = field.ty;
        match field.adapter {
            Some(adapter) => {
                collect_idents(quote::quote! { #ty #adapter }, &mut adapted);
            }
            None => collect_idents(quote::quote! { #ty }, &mut plain),
        }
    }

    let params = generics
        .type_params()
        .map(|x| x.ident.to_string())
        .collect::<HashSet<_>>();
    let mut new_predicates: Vec<syn::WherePredicate> = Vec::new();
    for param in generics.type_params() {
        let name = param.ident.to_string();
        let param = &param.ident;
        if adapted.contains(&name) && !plain.contains(&name) {
            if let Some(ref adapter_only_bound) = adapter_only_bound {
                new_predicates.push(syn::parse_quote!(#param : #adapter_only_bound));
            }
        } else {
            new_predicates.push(syn::parse_quote!(#param : #bound));
        }
    }
    for field in fields {
        let adapter = match field.adapter {
            Some(adapter) => adapter,
            None => continue,
        };
        let ty = field.ty;
        let mut idents = HashSet::new();
        collect_idents(quote::quote! { #ty #adapter }, &mut idents);
        if idents.iter().any(|x| params.contains(x)) {
            new_predicates.push(match adapter_lifetime {
                Some(ref lifetime) => syn::parse_quote!(#adapter : #adapter_trait<#lifetime, #ty>),
                None => syn::parse_quote!(#adapter : #adapter_trait<#ty>),
            });
        }
    }

    let mut generics = generics.clone();
    generics
        .make_where_clause()
        .predicates
        .extend(new_predicates);
    generics.where_clause.unwrap()
}
