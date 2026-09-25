use std::borrow::Cow;

use syn::meta::ParseNestedMeta;

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
    fn parse(lit: &syn::LitStr) -> syn::Result<RenameAll> {
        match lit.value().as_str() {
            "lowercase" => Ok(RenameAll::LowerCase),
            "UPPERCASE" => Ok(RenameAll::UpperCase),
            "PascalCase" => Ok(RenameAll::PascalCase),
            "camelCase" => Ok(RenameAll::CamelCase),
            "snake_case" => Ok(RenameAll::SnakeCase),
            "SCREAMING_SNAKE_CASE" => Ok(RenameAll::ScreamingSnakeCase),
            "kebab-case" => Ok(RenameAll::KebabCase),
            "SCREAMING-KEBAB-CASE" => Ok(RenameAll::ScreamingKebabCase),
            _ => Err(syn::Error::new_spanned(lit, "unknown rename_all style")),
        }
    }
}

#[derive(Clone)]
pub enum TypeDefault {
    Implicit,
    Explicit(syn::ExprPath),
}

pub struct ContainerAttrs<'a> {
    ident: &'a syn::Ident,
    rename: Option<String>,
    rename_all: Option<RenameAll>,
    default: Option<TypeDefault>,
    skip_serializing_optionals: bool,
    tag: Option<String>,
    content: Option<String>,
    untagged: bool,
}

/// Invokes `logic` for every item in all `#[deser(...)]` attributes.
///
/// The callback is passed the name of the item.  Items with paths that are
/// not plain identifiers are rejected.
fn parse_deser_attrs(
    attrs: &[syn::Attribute],
    mut logic: impl FnMut(&str, &ParseNestedMeta) -> syn::Result<()>,
) -> syn::Result<()> {
    for attr in attrs {
        if !attr.path().is_ident("deser") {
            continue;
        }
        attr.parse_nested_meta(|meta| match meta.path.get_ident() {
            Some(ident) => logic(&ident.to_string(), &meta),
            None => Err(meta.error("unsupported attribute")),
        })?;
    }
    Ok(())
}

/// Stores a value in a slot that must not have been filled before.
fn set_once<T>(
    meta: &ParseNestedMeta,
    name: &str,
    slot: &mut Option<T>,
    value: T,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(meta.error(format!("duplicate {} attribute", name)));
    }
    *slot = Some(value);
    Ok(())
}

/// Sets a flag that must not have been set before and does not take a value.
fn set_flag(meta: &ParseNestedMeta, name: &str, flag: &mut bool) -> syn::Result<()> {
    if has_value(meta) {
        return Err(meta.error(format!("{} does not take a value", name)));
    }
    if *flag {
        return Err(meta.error(format!("duplicate {} attribute", name)));
    }
    *flag = true;
    Ok(())
}

/// Checks if the item has a value (or arguments).
fn has_value(meta: &ParseNestedMeta) -> bool {
    !meta.input.is_empty() && !meta.input.peek(syn::Token![,])
}

/// Parses the value of `name = "..."`.
fn parse_lit_str(meta: &ParseNestedMeta) -> syn::Result<syn::LitStr> {
    meta.value()?.parse()
}

/// Parses the value of `name = "..."` as a string.
fn parse_str(meta: &ParseNestedMeta) -> syn::Result<String> {
    Ok(parse_lit_str(meta)?.value())
}

/// Parses the value of `name = "path"` as a path.
fn parse_str_path(meta: &ParseNestedMeta) -> syn::Result<syn::ExprPath> {
    parse_lit_str(meta)?.parse()
}

/// Parses `default` or `default = "path"`.
fn parse_default(meta: &ParseNestedMeta) -> syn::Result<TypeDefault> {
    if has_value(meta) {
        Ok(TypeDefault::Explicit(parse_str_path(meta)?))
    } else {
        Ok(TypeDefault::Implicit)
    }
}

impl<'a> ContainerAttrs<'a> {
    pub fn of(input: &'a syn::DeriveInput) -> syn::Result<ContainerAttrs<'a>> {
        let mut rv = ContainerAttrs {
            ident: &input.ident,
            rename: None,
            rename_all: None,
            default: None,
            skip_serializing_optionals: false,
            tag: None,
            content: None,
            untagged: false,
        };
        let is_enum = matches!(input.data, syn::Data::Enum(_));

        parse_deser_attrs(&input.attrs, |name, meta| match name {
            "rename_all" => {
                let value = RenameAll::parse(&parse_lit_str(meta)?)?;
                set_once(meta, name, &mut rv.rename_all, value)
            }
            "rename" => {
                let value = parse_str(meta)?;
                set_once(meta, name, &mut rv.rename, value)
            }
            "tag" => {
                let value = parse_str(meta)?;
                set_once(meta, name, &mut rv.tag, value)?;
                if !is_enum {
                    return Err(meta.error("tag is only supported on enums"));
                }
                Ok(())
            }
            "content" => {
                let value = parse_str(meta)?;
                set_once(meta, name, &mut rv.content, value)
            }
            "untagged" => {
                set_flag(meta, name, &mut rv.untagged)?;
                if !is_enum {
                    return Err(meta.error("untagged is only supported on enums"));
                }
                Ok(())
            }
            "default" => {
                let value = parse_default(meta)?;
                set_once(meta, name, &mut rv.default, value)
            }
            "skip_serializing_optionals" => {
                set_flag(meta, name, &mut rv.skip_serializing_optionals)
            }
            _ => Err(meta.error("unsupported attribute")),
        })?;

        if rv.content.is_some() && rv.tag.is_none() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "content requires a tag attribute",
            ));
        }
        if rv.untagged && rv.tag.is_some() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "untagged cannot be combined with tag",
            ));
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
                RenameAll::KebabCase => name.replace("_", "-"),
                RenameAll::ScreamingKebabCase => name.replace("_", "-").to_ascii_uppercase(),
            }
        } else {
            name
        }
    }

    pub fn default(&self) -> Option<&TypeDefault> {
        self.default.as_ref()
    }

    pub fn skip_serializing_optionals(&self) -> bool {
        self.skip_serializing_optionals
    }

    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    pub fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }

    pub fn untagged(&self) -> bool {
        self.untagged
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

pub fn ensure_no_field_attrs(field: &syn::Field) -> syn::Result<()> {
    parse_deser_attrs(&field.attrs, |_, meta| {
        Err(meta.error("unsupported attribute"))
    })
}

pub struct FieldAttrs<'a> {
    field: &'a syn::Field,
    rename: Option<String>,
    aliases: Vec<String>,
    default: Option<TypeDefault>,
    flatten: bool,
    skip_serializing_if: Option<syn::ExprPath>,
}

impl<'a> FieldAttrs<'a> {
    pub fn of(field: &'a syn::Field) -> syn::Result<FieldAttrs<'a>> {
        let mut rv = FieldAttrs {
            field,
            rename: None,
            aliases: Vec::new(),
            default: None,
            flatten: false,
            skip_serializing_if: None,
        };

        parse_deser_attrs(&field.attrs, |name, meta| match name {
            "rename" => {
                let value = parse_str(meta)?;
                set_once(meta, name, &mut rv.rename, value)
            }
            "alias" => {
                rv.aliases.push(parse_str(meta)?);
                Ok(())
            }
            "default" => {
                let value = parse_default(meta)?;
                set_once(meta, name, &mut rv.default, value)
            }
            "flatten" => set_flag(meta, name, &mut rv.flatten),
            "skip_serializing_if" => {
                let value = parse_str_path(meta)?;
                set_once(meta, name, &mut rv.skip_serializing_if, value)
            }
            _ => Err(meta.error("unsupported attribute")),
        })?;

        if rv.flatten && rv.default.is_some() {
            return Err(syn::Error::new_spanned(
                field,
                "cannot combine flatten and default",
            ));
        }

        Ok(rv)
    }

    pub fn field(&self) -> &syn::Field {
        self.field
    }

    pub fn name(&self, container_attrs: &ContainerAttrs) -> Cow<'_, str> {
        self.rename
            .as_deref()
            .map(Cow::Borrowed)
            .unwrap_or_else(|| container_attrs.get_field_name(self.field).into())
    }

    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    /// Returns the name of the field ignoring container level renames.
    pub fn plain_name(&self) -> Cow<'_, str> {
        self.rename
            .as_deref()
            .map(Cow::Borrowed)
            .unwrap_or_else(|| self.field.ident.as_ref().unwrap().to_string().into())
    }

    pub fn default(&self) -> Option<&TypeDefault> {
        self.default.as_ref()
    }

    pub fn flatten(&self) -> bool {
        self.flatten
    }

    pub fn skip_serializing_if(&self) -> Option<&syn::ExprPath> {
        self.skip_serializing_if.as_ref()
    }
}

pub struct EnumVariantAttrs<'a> {
    variant: &'a syn::Variant,
    rename: Option<String>,
    aliases: Vec<String>,
    other: bool,
}

impl<'a> EnumVariantAttrs<'a> {
    pub fn of(variant: &'a syn::Variant) -> syn::Result<EnumVariantAttrs<'a>> {
        let mut rv = EnumVariantAttrs {
            variant,
            rename: None,
            aliases: Vec::new(),
            other: false,
        };

        parse_deser_attrs(&variant.attrs, |name, meta| match name {
            "rename" => {
                let value = parse_str(meta)?;
                set_once(meta, name, &mut rv.rename, value)
            }
            "alias" => {
                rv.aliases.push(parse_str(meta)?);
                Ok(())
            }
            "other" => {
                set_flag(meta, name, &mut rv.other)?;
                if !matches!(variant.fields, syn::Fields::Unit) {
                    return Err(meta.error("other is only supported on unit variants"));
                }
                Ok(())
            }
            _ => Err(meta.error("unsupported attribute")),
        })?;

        Ok(rv)
    }

    pub fn other(&self) -> bool {
        self.other
    }

    pub fn variant(&self) -> &syn::Variant {
        self.variant
    }

    pub fn name(&self, container_attrs: &ContainerAttrs) -> Cow<'_, str> {
        self.rename
            .as_deref()
            .map(Cow::Borrowed)
            .unwrap_or_else(|| container_attrs.get_variant_name(self.variant).into())
    }

    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }
}
