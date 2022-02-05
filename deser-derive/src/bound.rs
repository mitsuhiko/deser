use proc_macro2::{Span, TokenStream};

pub fn with_lifetime_bound(generics: &syn::Generics, lifetime: &str) -> syn::Generics {
    let bound = syn::Lifetime::new(lifetime, Span::call_site());
    let def = syn::LifetimeDef {
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

pub fn where_clause_with_bound(generics: &syn::Generics, bound: TokenStream) -> syn::WhereClause {
    let new_predicates = generics
        .type_params()
        .map::<syn::WherePredicate, _>(|param| {
            let param = &param.ident;
            syn::parse_quote!(#param : #bound)
        });

    let mut generics = generics.clone();
    generics
        .make_where_clause()
        .predicates
        .extend(new_predicates);
    generics.where_clause.unwrap()
}
