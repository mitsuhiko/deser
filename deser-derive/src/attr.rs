/// Find the value of a #[deser(rename = "...")] attribute.
fn attr_rename(attrs: &[syn::Attribute]) -> syn::Result<Option<String>> {
    let mut rename = None;

    for attr in attrs {
        if !attr.path.is_ident("deser") {
            continue;
        }

        let list = match attr.parse_meta()? {
            syn::Meta::List(list) => list,
            other => return Err(syn::Error::new_spanned(other, "unsupported attribute")),
        };

        for meta in &list.nested {
            if let syn::NestedMeta::Meta(syn::Meta::NameValue(value)) = meta {
                if value.path.is_ident("rename") {
                    if let syn::Lit::Str(s) = &value.lit {
                        if rename.is_some() {
                            return Err(syn::Error::new_spanned(
                                meta,
                                "duplicate rename attribute",
                            ));
                        }
                        rename = Some(s.value());
                        continue;
                    }
                }
            }
            return Err(syn::Error::new_spanned(meta, "unsupported attribute"));
        }
    }

    Ok(rename)
}

/// Determine the name of a field, respecting a rename attribute.
pub fn name_of_field(field: &syn::Field) -> syn::Result<String> {
    let rename = attr_rename(&field.attrs)?;
    Ok(rename.unwrap_or_else(|| field.ident.as_ref().unwrap().to_string()))
}
