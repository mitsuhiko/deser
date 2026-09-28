use std::collections::HashSet;

use proc_macro2::{Span, TokenStream};

pub(crate) fn with_lifetime_bound(generics: &syn::Generics, lifetime: &str) -> syn::Generics {
    let bound = syn::Lifetime::new(lifetime, Span::call_site());
    let def = syn::LifetimeParam {
        attrs: Vec::new(),
        lifetime: bound.clone(),
        colon_token: None,
        bounds: syn::punctuated::Punctuated::new(),
    };

    let mut params = syn::punctuated::Punctuated::new();
    params.push(syn::GenericParam::Lifetime(def));
    for param in &generics.params {
        let mut param = param.clone();
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
        params.push(param);
    }

    syn::Generics {
        params,
        ..generics.clone()
    }
}

/// Returns a turbofish with the type and const parameters of the generics.
///
/// Lifetimes are left out (and inferred), which allows the turbofish to be
/// used for functions that have more lifetime parameters than the type
/// (such as `'de`).
pub(crate) fn turbofish_without_lifetimes(generics: &syn::Generics) -> TokenStream {
    let mut params = Vec::new();
    for param in &generics.params {
        match param {
            syn::GenericParam::Type(param) => params.push(&param.ident),
            syn::GenericParam::Const(param) => params.push(&param.ident),
            syn::GenericParam::Lifetime(_) => {}
        }
    }
    if params.is_empty() {
        TokenStream::new()
    } else {
        quote::quote! { ::<#(#params),*> }
    }
}

/// Adds the lifetime `'__s` of the slot of a value to the generics.
///
/// The lifetime has no bounds so that functions which take a slot with
/// this lifetime are generic over it (it's late bound), as needed for
/// `for<'x> fn(&'x mut Option<T>) -> SinkHandle<'x, 'de>`.
pub(crate) fn with_slot_lifetime(generics: &syn::Generics) -> syn::Generics {
    let mut rv = generics.clone();
    rv.params.insert(
        0,
        syn::GenericParam::Lifetime(syn::LifetimeParam::new(syn::Lifetime::new(
            "'__s",
            Span::call_site(),
        ))),
    );
    rv
}

/// Adds the `'de` lifetime of `Deserialize` to the generics.
///
/// All lifetimes of the type are bounded by `'de` so that borrowed data can
/// be deserialized into them.
pub(crate) fn with_de_lifetime(generics: &syn::Generics) -> syn::Result<syn::Generics> {
    let mut bounds = syn::punctuated::Punctuated::<_, syn::Token![+]>::new();
    for lifetime in generics.lifetimes() {
        if lifetime.lifetime.ident == "de" {
            return Err(syn::Error::new_spanned(
                lifetime,
                "cannot derive Deserialize for types with a lifetime named 'de, \
                 it's used by the derive",
            ));
        }
        bounds.push(lifetime.lifetime.clone());
    }
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
pub(crate) fn where_clause_with_bound(
    generics: &syn::Generics,
    bound: TokenStream,
    custom: Option<&[syn::WherePredicate]>,
) -> syn::WhereClause {
    let mut new_predicates: Vec<syn::WherePredicate> = Vec::new();
    match custom {
        Some(custom) => new_predicates.extend_from_slice(custom),
        None => {
            for param in generics.type_params() {
                let param = &param.ident;
                new_predicates.push(syn::parse_quote!(#param : #bound));
            }
        }
    }

    let mut generics = generics.clone();
    generics
        .make_where_clause()
        .predicates
        .extend(new_predicates);
    generics.where_clause.unwrap()
}

/// Returns the names of the type parameters.
pub(crate) fn type_param_names(generics: &syn::Generics) -> HashSet<String> {
    let mut rv = HashSet::new();
    for param in generics.type_params() {
        rv.insert(param.ident.to_string());
    }
    rv
}

/// Returns `true` if one of the names is an identifier in the tokens.
pub(crate) fn mentions_any(tokens: TokenStream, names: &HashSet<String>) -> bool {
    let mut idents = HashSet::new();
    collect_idents(tokens, &mut idents);
    for ident in &idents {
        if names.contains(ident) {
            return true;
        }
    }
    false
}

/// Collects all identifiers in a token stream.
pub(crate) fn collect_idents(stream: TokenStream, out: &mut HashSet<String>) {
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

/// Collects the names of all lifetimes (without the `'`) in a token stream.
pub(crate) fn collect_lifetimes(stream: TokenStream, out: &mut HashSet<String>) {
    let mut after_quote = false;
    for token in stream {
        match token {
            proc_macro2::TokenTree::Punct(ref punct) if punct.as_char() == '\'' => {
                after_quote = true;
                continue;
            }
            proc_macro2::TokenTree::Ident(ident) if after_quote => {
                out.insert(ident.to_string());
            }
            proc_macro2::TokenTree::Group(group) => collect_lifetimes(group.stream(), out),
            _ => {}
        }
        after_quote = false;
    }
}

/// A field for the purpose of bound inference.
pub(crate) struct BoundField<'a> {
    pub ty: &'a syn::Type,
    pub adapter: Option<&'a syn::Type>,
    /// The field is skipped (not serialized or deserialized).
    pub skipped: bool,
    /// The custom bounds of the field which replace the inferred ones.
    pub bound: Option<&'a [syn::WherePredicate]>,
    /// The type refers to the lifetime `'__x`, the bound of the adapter is
    /// higher-ranked over it (`for<'__x> A: SerializeAs<(&'__x T, &'__x U)>`).
    pub higher_ranked: bool,
}

/// Returns the where clause of the generics with bounds inferred from fields.
///
/// If `custom` is `Some` the custom predicates are used instead, otherwise
/// every type parameter gets `bound` unless it only appears in fields with
/// adapters, skipped fields or fields with custom bounds.  Such parameters
/// get `adapter_only_bound` if provided.  For fields with adapters that
/// refer to type parameters a predicate that requires the adapter to
/// implement `adapter_trait` for the field type is added.  The custom bounds
/// of fields are always added.
pub(crate) fn where_clause_for_fields(
    generics: &syn::Generics,
    bound: TokenStream,
    adapter_only_bound: Option<TokenStream>,
    adapter_trait: TokenStream,
    adapter_lifetime: Option<TokenStream>,
    custom: Option<&[syn::WherePredicate]>,
    fields: &[BoundField<'_>],
) -> syn::WhereClause {
    let mut field_bounds = Vec::new();
    let mut all_plain = true;
    for field in fields {
        if let Some(bound) = field.bound {
            field_bounds.extend_from_slice(bound);
        }
        if field.adapter.is_some() || field.skipped || field.bound.is_some() {
            all_plain = false;
        }
    }
    if custom.is_some() || all_plain {
        let mut rv = where_clause_with_bound(generics, bound, custom);
        rv.predicates.extend(field_bounds);
        return rv;
    }

    let mut plain = HashSet::new();
    let mut adapted = HashSet::new();
    for field in fields {
        let ty = field.ty;
        match field.adapter {
            _ if field.skipped || field.bound.is_some() => {
                collect_idents(quote::quote! { #ty }, &mut adapted)
            }
            Some(adapter) => {
                collect_idents(quote::quote! { #ty #adapter }, &mut adapted);
            }
            None => collect_idents(quote::quote! { #ty }, &mut plain),
        }
    }

    let params = type_param_names(generics);
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
            Some(adapter) if !field.skipped && field.bound.is_none() => adapter,
            _ => continue,
        };
        let ty = field.ty;
        if mentions_any(quote::quote! { #ty #adapter }, &params) {
            let for_lifetimes = if field.higher_ranked {
                Some(quote::quote! { for<'__x> })
            } else {
                None
            };
            new_predicates.push(match adapter_lifetime {
                Some(ref lifetime) => {
                    syn::parse_quote!(#for_lifetimes #adapter : #adapter_trait<#lifetime, #ty>)
                }
                None => syn::parse_quote!(#for_lifetimes #adapter : #adapter_trait<#ty>),
            });
        }
    }
    new_predicates.extend(field_bounds);

    let mut generics = generics.clone();
    generics
        .make_where_clause()
        .predicates
        .extend(new_predicates);
    generics.where_clause.unwrap()
}
