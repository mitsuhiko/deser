use std::borrow::Cow;

pub struct FieldAttrs<'a> {
    field: &'a syn::Field,
    rename: Option<String>,
}

impl<'a> FieldAttrs<'a> {
    pub fn of(field: &'a syn::Field) -> syn::Result<FieldAttrs<'a>> {
        let mut rv = FieldAttrs {
            field,
            rename: None,
        };

        for attr in &field.attrs {
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
                            if rv.rename.is_some() {
                                return Err(syn::Error::new_spanned(
                                    meta,
                                    "duplicate rename attribute",
                                ));
                            }
                            rv.rename = Some(s.value());
                            continue;
                        }
                    }
                }
                return Err(syn::Error::new_spanned(meta, "unsupported attribute"));
            }
        }

        Ok(rv)
    }

    pub fn name(&self) -> Cow<'_, str> {
        self.rename
            .as_deref()
            .map(Cow::Borrowed)
            .unwrap_or_else(|| self.field.ident.as_ref().unwrap().to_string().into())
    }
}
