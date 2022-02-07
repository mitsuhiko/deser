use std::borrow::Cow;

#[derive(Copy, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum RenameAll {
    LowerCase,
    UpperCase,
    PascalCase,
    CamelCase,
    SnakeCase,
    ScreamingSnakeCase,
    KebabCase,
    ScreamingKebabCase,
}

impl RenameAll {
    fn parse(lit: &syn::Lit) -> Result<RenameAll, syn::Error> {
        if let syn::Lit::Str(s) = &lit {
            match s.value().as_str() {
                "lowercase" => Ok(RenameAll::LowerCase),
                "UPPERCASE" => Ok(RenameAll::UpperCase),
                "PascalCase" => Ok(RenameAll::PascalCase),
                "camelCase" => Ok(RenameAll::CamelCase),
                "snake_case" => Ok(RenameAll::SnakeCase),
                "SCREAMING_SNAKE_CASE" => Ok(RenameAll::ScreamingSnakeCase),
                "kebab-case" => Ok(RenameAll::KebabCase),
                "SCREAMING-KEBAB-CASE" => Ok(RenameAll::ScreamingKebabCase),
                _ => Err(syn::Error::new_spanned(lit, "")),
            }
        } else {
            Err(syn::Error::new_spanned(lit, "rename expects a string"))
        }
    }
}

pub struct ContainerAttrs<'a> {
    ident: &'a syn::Ident,
    rename: Option<String>,
    rename_all: Option<RenameAll>,
}

pub fn get_meta_items(attr: &syn::Attribute) -> syn::Result<Vec<syn::NestedMeta>> {
    if !attr.path.is_ident("deser") {
        return Ok(Vec::new());
    }

    match attr.parse_meta() {
        Ok(syn::Meta::List(meta)) => Ok(meta.nested.into_iter().collect()),
        Ok(_) => Err(syn::Error::new_spanned(attr, "expected #[deser(...)]")),
        Err(err) => Err(err),
    }
}

fn get_lit_str(attr_name: &str, lit: &syn::Lit) -> syn::Result<String> {
    if let syn::Lit::Str(lit) = lit {
        Ok(lit.value())
    } else {
        Err(syn::Error::new_spanned(
            lit,
            format!(
                "expected attribute to be a string: `{} = \"...\"`",
                attr_name,
            ),
        ))
    }
}

impl<'a> ContainerAttrs<'a> {
    pub fn of(input: &'a syn::DeriveInput) -> syn::Result<ContainerAttrs<'a>> {
        let mut rv = ContainerAttrs {
            ident: &input.ident,
            rename: None,
            rename_all: None,
        };

        for meta_item in input.attrs.iter().flat_map(get_meta_items).flatten() {
            if let syn::NestedMeta::Meta(meta) = meta_item {
                match meta {
                    syn::Meta::NameValue(nv) if nv.path.is_ident("rename_all") => {
                        rv.rename_all = Some(RenameAll::parse(&nv.lit)?);
                    }
                    syn::Meta::NameValue(nv) if nv.path.is_ident("rename") => {
                        rv.rename = Some(get_lit_str("rename", &nv.lit)?);
                    }
                    _ => return Err(syn::Error::new_spanned(meta, "unexpected attribute")),
                }
            } else {
                return Err(syn::Error::new_spanned(meta_item, "unexpected literal"));
            }
        }

        Ok(rv)
    }

    pub fn container_name(&self) -> String {
        match self.rename {
            Some(ref name) => name.clone(),
            None => self.ident.to_string(),
        }
    }

    pub fn get_field_name(&self, field: &syn::Field) -> String {
        let name = field.ident.as_ref().unwrap().to_string();
        if let Some(rename_all) = self.rename_all {
            match rename_all {
                RenameAll::LowerCase | RenameAll::SnakeCase => name,
                RenameAll::UpperCase | RenameAll::ScreamingSnakeCase => name.to_ascii_uppercase(),
                RenameAll::PascalCase => {
                    let mut pascal = String::new();
                    let mut capitalize = true;
                    for ch in name.chars() {
                        if ch == '_' {
                            capitalize = true;
                        } else if capitalize {
                            pascal.push(ch.to_ascii_uppercase());
                            capitalize = false;
                        } else {
                            pascal.push(ch);
                        }
                    }
                    pascal
                }
                RenameAll::CamelCase => {
                    let mut camel = String::new();
                    let mut capitalize = false;
                    for ch in name.chars() {
                        if ch == '_' {
                            capitalize = true;
                        } else if capitalize {
                            camel.push(ch.to_ascii_uppercase());
                            capitalize = false;
                        } else {
                            camel.push(ch);
                        }
                    }
                    camel
                }
                RenameAll::KebabCase => name.replace("_", "-'"),
                RenameAll::ScreamingKebabCase => name.replace("_", "-").to_ascii_uppercase(),
            }
        } else {
            name
        }
    }

    pub fn get_variant_name(&self, variant: &syn::Variant) -> String {
        let name = variant.ident.to_string();
        if let Some(rename_all) = self.rename_all {
            match rename_all {
                RenameAll::PascalCase => name,
                RenameAll::LowerCase => name.to_ascii_lowercase(),
                RenameAll::UpperCase => name.to_ascii_uppercase(),
                RenameAll::CamelCase => name[..1].to_ascii_lowercase() + &name[1..],
                RenameAll::SnakeCase
                | RenameAll::ScreamingSnakeCase
                | RenameAll::KebabCase
                | RenameAll::ScreamingKebabCase => {
                    let sep = if matches!(
                        rename_all,
                        RenameAll::SnakeCase | RenameAll::ScreamingSnakeCase
                    ) {
                        '_'
                    } else {
                        '-'
                    };
                    let upper = matches!(
                        rename_all,
                        RenameAll::ScreamingKebabCase | RenameAll::ScreamingSnakeCase
                    );
                    let mut rv = String::new();
                    for (i, ch) in name.char_indices() {
                        if i > 0 && ch.is_uppercase() {
                            rv.push(sep);
                        }
                        rv.push(if upper {
                            ch.to_ascii_uppercase()
                        } else {
                            ch.to_ascii_lowercase()
                        });
                    }
                    rv
                }
            }
        } else {
            name
        }
    }
}

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

        for meta_item in field.attrs.iter().flat_map(get_meta_items).flatten() {
            if let syn::NestedMeta::Meta(syn::Meta::NameValue(value)) = &meta_item {
                if value.path.is_ident("rename") {
                    if let syn::Lit::Str(s) = &value.lit {
                        if rv.rename.is_some() {
                            return Err(syn::Error::new_spanned(
                                meta_item,
                                "duplicate rename attribute",
                            ));
                        }
                        rv.rename = Some(s.value());
                        continue;
                    }
                }
            }
            return Err(syn::Error::new_spanned(meta_item, "unsupported attribute"));
        }

        Ok(rv)
    }

    pub fn name(&self, container_attrs: &ContainerAttrs) -> Cow<'_, str> {
        self.rename
            .as_deref()
            .map(Cow::Borrowed)
            .unwrap_or_else(|| container_attrs.get_field_name(self.field).into())
    }
}

pub struct EnumVariantAttrs<'a> {
    variant: &'a syn::Variant,
    rename: Option<String>,
}

impl<'a> EnumVariantAttrs<'a> {
    pub fn of(variant: &'a syn::Variant) -> syn::Result<EnumVariantAttrs<'a>> {
        let mut rv = EnumVariantAttrs {
            variant,
            rename: None,
        };

        for meta_item in variant.attrs.iter().flat_map(get_meta_items).flatten() {
            if let syn::NestedMeta::Meta(syn::Meta::NameValue(value)) = &meta_item {
                if value.path.is_ident("rename") {
                    if let syn::Lit::Str(s) = &value.lit {
                        if rv.rename.is_some() {
                            return Err(syn::Error::new_spanned(
                                meta_item,
                                "duplicate rename attribute",
                            ));
                        }
                        rv.rename = Some(s.value());
                        continue;
                    }
                }
            }
            return Err(syn::Error::new_spanned(meta_item, "unsupported attribute"));
        }

        Ok(rv)
    }

    pub fn name(&self, container_attrs: &ContainerAttrs) -> Cow<'_, str> {
        self.rename
            .as_deref()
            .map(Cow::Borrowed)
            .unwrap_or_else(|| container_attrs.get_variant_name(self.variant).into())
    }
}
