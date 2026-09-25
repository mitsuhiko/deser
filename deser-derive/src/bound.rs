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
