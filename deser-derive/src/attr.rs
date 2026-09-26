use std::borrow::Cow;

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{ToTokens, quote};
use syn::meta::ParseNestedMeta;

/// The direction of a derive.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Direction {
    Serialize,
    Deserialize,
}

/// An attribute that was used on an item.
pub struct SeenAttr {
    pub name: String,
    pub span: Span,
}

/// The adapters of an item for serialization and deserialization.
///
/// `as` sets both, `serialize_as` and `deserialize_as` one of them.
#[derive(Default, Clone)]
pub struct Adapters {
    ser: Option<syn::Type>,
    de: Option<syn::Type>,
}

impl Adapters {
    /// Returns the adapter used for serialization.
    pub fn ser(&self) -> Option<&syn::Type> {
        self.ser.as_ref()
    }

    /// Returns the adapter used for deserialization.
    pub fn de(&self) -> Option<&syn::Type> {
        self.de.as_ref()
    }

    /// Returns the adapter for a direction.
    pub fn get(&self, direction: Direction) -> Option<&syn::Type> {
        match direction {
            Direction::Serialize => self.ser(),
            Direction::Deserialize => self.de(),
        }
    }

    /// Returns `true` if there is an adapter for any direction.
    pub fn any(&self) -> bool {
        self.ser.is_some() || self.de.is_some()
    }
}

/// Collects the `as`, `serialize_as` and `deserialize_as` attributes.
#[derive(Default)]
struct AdapterAttrs {
    both: Option<syn::Type>,
    ser: Option<syn::Type>,
    de: Option<syn::Type>,
}

impl AdapterAttrs {
    /// Parses the attribute if it's one of the adapter attributes.
    ///
    /// Returns `false` if the attribute is not an adapter attribute.
    fn parse(
        &mut self,
        name: &str,
        meta: &ParseNestedMeta,
        parse: impl FnOnce(&ParseNestedMeta) -> syn::Result<syn::Type>,
    ) -> syn::Result<bool> {
        let slot = match name {
            "as" => &mut self.both,
            "serialize_as" => &mut self.ser,
            "deserialize_as" => &mut self.de,
            _ => return Ok(false),
        };
        let value = parse(meta)?;
        set_once(meta, name, slot, value)?;
        Ok(true)
    }

    /// Resolves the adapters for both directions.
    fn finish(self, seen: &[SeenAttr]) -> syn::Result<Adapters> {
        match self.both {
            Some(both) => {
                if self.ser.is_some() || self.de.is_some() {
                    let span = seen
                        .iter()
                        .find(|x| x.name == "serialize_as" || x.name == "deserialize_as")
                        .map_or_else(Span::call_site, |x| x.span);
                    return Err(syn::Error::new(
                        span,
                        "`as` cannot be combined with `serialize_as` or `deserialize_as`",
                    ));
                }
                Ok(Adapters {
                    ser: Some(both.clone()),
                    de: Some(both),
                })
            }
            None => Ok(Adapters {
                ser: self.ser,
                de: self.de,
            }),
        }
    }
}

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
    /// An expression that produces the default value.
    Explicit(TokenStream),
}

pub struct ContainerAttrs<'a> {
    ident: &'a syn::Ident,
    seen: Vec<SeenAttr>,
    adapters: Adapters,
    rename: Option<String>,
    rename_all: Option<RenameAll>,
    default: Option<TypeDefault>,
    skip_serializing_optionals: bool,
    tag: Option<String>,
    content: Option<String>,
    untagged: bool,
    crate_path: Option<syn::Path>,
    bound: Option<Vec<syn::WherePredicate>>,
    serialize_bound: Option<Vec<syn::WherePredicate>>,
    deserialize_bound: Option<Vec<syn::WherePredicate>>,
}

/// Invokes `logic` for every item in all `#[deser(...)]` attributes.
///
/// The callback is passed the name of the item.  Items with paths that are
/// not plain identifiers are rejected.  Returns the items that were seen.
fn parse_deser_attrs(
    attrs: &[syn::Attribute],
    mut logic: impl FnMut(&str, &ParseNestedMeta) -> syn::Result<()>,
) -> syn::Result<Vec<SeenAttr>> {
    let mut seen = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("deser") {
            continue;
        }
        attr.parse_nested_meta(|meta| match meta.path.get_ident() {
            Some(ident) => {
                let name = ident.to_string();
                seen.push(SeenAttr {
                    name: name.clone(),
                    span: ident.span(),
                });
                logic(&name, &meta)
            }
            None => Err(meta.error("unsupported attribute")),
        })?;
    }
    Ok(seen)
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

/// Rejects `Self` in expressions and paths.
///
/// The generated code that evaluates the expressions lives in different impl
/// blocks (sinks, emitters, helper structs for enum variants) so `Self` would
/// not refer to the type the attribute is placed on.
fn reject_self(tokens: TokenStream) -> syn::Result<()> {
    for token in tokens {
        match token {
            TokenTree::Ident(ident) if ident == "Self" => {
                return Err(syn::Error::new(
                    ident.span(),
                    "`Self` is not supported in deser attributes, use the type name",
                ));
            }
            TokenTree::Group(group) => reject_self(group.stream())?,
            _ => {}
        }
    }
    Ok(())
}

/// Parses the value of `name = path`.
fn parse_path(meta: &ParseNestedMeta) -> syn::Result<syn::ExprPath> {
    let path: syn::ExprPath = meta
        .value()?
        .parse()
        .map_err(|err| syn::Error::new(err.span(), "expected a path to a function"))?;
    reject_self(path.to_token_stream())?;
    Ok(path)
}

/// Replaces `_` in an adapter type with the `Same` adapter.
fn replace_infer(tokens: TokenStream) -> TokenStream {
    tokens
        .into_iter()
        .flat_map(|token| -> TokenStream {
            match token {
                TokenTree::Ident(ref ident) if ident == "_" => {
                    let span = ident.span();
                    quote::quote_spanned! { span=> __deser::adapters::Same }
                }
                TokenTree::Group(group) => {
                    let mut new_group =
                        proc_macro2::Group::new(group.delimiter(), replace_infer(group.stream()));
                    new_group.set_span(group.span());
                    TokenTree::Group(new_group).into()
                }
                other => other.into(),
            }
        })
        .collect()
}

/// Parses the value of `as = Type`.
///
/// `_` in the type is replaced with the `Same` adapter.
fn parse_adapter(meta: &ParseNestedMeta) -> syn::Result<syn::Type> {
    let ty: syn::Type = meta
        .value()?
        .parse()
        .map_err(|err| syn::Error::new(err.span(), "expected an adapter type"))?;
    reject_self(ty.to_token_stream())?;
    syn::parse2(replace_infer(ty.to_token_stream()))
}

/// Parses the value of `as = Type` on a container.
///
/// The implementations of the container forward to the adapter.  Adapters
/// that use the implementation of the container would recurse forever: `_`
/// and `Same` (which stand for the type's own implementation) and the type
/// itself are rejected as adapter and as direct type argument of the
/// adapter (as in `FromInto<Self>` or `DefaultOnError<_>`).  Nested uses
/// such as `FromInto<Vec<Node>>` are fine.
fn parse_container_adapter(meta: &ParseNestedMeta, ident: &syn::Ident) -> syn::Result<syn::Type> {
    fn is_own_impl(ty: &syn::Type, ident: &syn::Ident) -> bool {
        match ty {
            syn::Type::Infer(_) => true,
            syn::Type::Paren(ty) => is_own_impl(&ty.elem, ident),
            syn::Type::Group(ty) => is_own_impl(&ty.elem, ident),
            syn::Type::Path(ty) if ty.qself.is_none() => ty
                .path
                .segments
                .last()
                .is_some_and(|x| x.ident == "Same" || x.ident == *ident),
            _ => false,
        }
    }

    let ty: syn::Type = meta
        .value()?
        .parse()
        .map_err(|err| syn::Error::new(err.span(), "expected an adapter type"))?;
    reject_self(ty.to_token_stream())?;

    let mut candidates = vec![&ty];
    if let syn::Type::Path(ref path) = ty
        && let Some(segment) = path.path.segments.last()
        && let syn::PathArguments::AngleBracketed(ref args) = segment.arguments
    {
        candidates.extend(args.args.iter().filter_map(|arg| match arg {
            syn::GenericArgument::Type(ty) => Some(ty),
            _ => None,
        }));
    }
    if let Some(bad) = candidates.into_iter().find(|x| is_own_impl(x, ident)) {
        let what = match bad {
            syn::Type::Infer(_) => "`_`".to_string(),
            _ => format!("`{}`", bad.to_token_stream()).replace(' ', ""),
        };
        return Err(syn::Error::new_spanned(
            bad,
            format!(
                "{} refers to the implementation of `{}` which forwards to this adapter, \
                 this would recurse forever",
                what, ident
            ),
        ));
    }

    syn::parse2(replace_infer(ty.to_token_stream()))
}

/// Parses `bound(T: Trait, U: Other)` into where predicates.
fn parse_bound(meta: &ParseNestedMeta) -> syn::Result<Vec<syn::WherePredicate>> {
    if !meta.input.peek(syn::token::Paren) {
        return Err(meta.error("expected a list of where predicates: `bound(T: Trait)`"));
    }
    let content;
    syn::parenthesized!(content in meta.input);
    let predicates =
        syn::punctuated::Punctuated::<syn::WherePredicate, syn::Token![,]>::parse_terminated(
            &content,
        )?;
    reject_self(predicates.to_token_stream())?;
    Ok(predicates.into_iter().collect())
}

/// Parses `default` or `default = expr`.
///
/// String literals are converted with `Into` so that they can be used as
/// defaults for `String` and friends.  All other expressions are used as is.
fn parse_default(meta: &ParseNestedMeta) -> syn::Result<TypeDefault> {
    if !has_value(meta) {
        return Ok(TypeDefault::Implicit);
    }
    let expr: syn::Expr = meta.value()?.parse().map_err(|err| {
        syn::Error::new(
            err.span(),
            "expected an expression, complex defaults must go into a function",
        )
    })?;
    reject_self(expr.to_token_stream())?;
    Ok(TypeDefault::Explicit(match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(ref lit),
            ..
        }) => quote! { __deser::__derive::Into::into(#lit) },
        expr => expr.into_token_stream(),
    }))
}

impl<'a> ContainerAttrs<'a> {
    pub fn of(input: &'a syn::DeriveInput) -> syn::Result<ContainerAttrs<'a>> {
        let mut rv = ContainerAttrs {
            ident: &input.ident,
            seen: Vec::new(),
            adapters: Adapters::default(),
            rename: None,
            rename_all: None,
            default: None,
            skip_serializing_optionals: false,
            tag: None,
            content: None,
            untagged: false,
            crate_path: None,
            bound: None,
            serialize_bound: None,
            deserialize_bound: None,
        };
        let is_enum = matches!(input.data, syn::Data::Enum(_));
        let mut adapters = AdapterAttrs::default();

        let seen = parse_deser_attrs(&input.attrs, |name, meta| match name {
            "as" | "serialize_as" | "deserialize_as" => {
                adapters.parse(name, meta, |meta| {
                    parse_container_adapter(meta, &input.ident)
                })?;
                Ok(())
            }
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
            "crate" => {
                let value = meta
                    .value()?
                    .parse()
                    .map_err(|err| syn::Error::new(err.span(), "expected a path to the crate"))?;
                set_once(meta, name, &mut rv.crate_path, value)
            }
            "bound" => {
                let value = parse_bound(meta)?;
                set_once(meta, name, &mut rv.bound, value)
            }
            "serialize_bound" => {
                let value = parse_bound(meta)?;
                set_once(meta, name, &mut rv.serialize_bound, value)
            }
            "deserialize_bound" => {
                let value = parse_bound(meta)?;
                set_once(meta, name, &mut rv.deserialize_bound, value)
            }
            _ => Err(meta.error("unsupported attribute")),
        })?;
        rv.seen = seen;

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
        rv.adapters = adapters.finish(&rv.seen)?;

        Ok(rv)
    }

    /// Returns the attributes that were used on the container.
    pub fn into_seen(self) -> Vec<SeenAttr> {
        self.seen
    }

    /// Returns the adapters the container is serialized and deserialized
    /// with.
    pub fn adapters(&self) -> &Adapters {
        &self.adapters
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

    /// Returns the path to the deser crate if it was overridden.
    pub fn crate_path(&self) -> Option<&syn::Path> {
        self.crate_path.as_ref()
    }

    /// Returns the custom where predicates for the `Serialize` impl.
    ///
    /// If this returns `Some` the predicates replace the inferred bounds.
    pub fn serialize_bound(&self) -> Option<&[syn::WherePredicate]> {
        self.serialize_bound
            .as_ref()
            .or(self.bound.as_ref())
            .map(|x| &x[..])
    }

    /// Returns the custom where predicates for the `Deserialize` impl.
    ///
    /// If this returns `Some` the predicates replace the inferred bounds.
    pub fn deserialize_bound(&self) -> Option<&[syn::WherePredicate]> {
        self.deserialize_bound
            .as_ref()
            .or(self.bound.as_ref())
            .map(|x| &x[..])
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

/// The attributes of unnamed fields (of newtype structs and tuple variants).
pub struct UnnamedFieldAttrs {
    seen: Vec<SeenAttr>,
    adapters: Adapters,
    tag: bool,
}

impl UnnamedFieldAttrs {
    pub fn of(field: &syn::Field) -> syn::Result<UnnamedFieldAttrs> {
        let mut tag = false;
        let mut adapters = AdapterAttrs::default();
        let seen = parse_deser_attrs(&field.attrs, |name, meta| {
            if adapters.parse(name, meta, parse_adapter)? {
                return Ok(());
            }
            match name {
                "tag" => set_flag(meta, name, &mut tag),
                _ => Err(meta.error("unsupported attribute")),
            }
        })?;
        Ok(UnnamedFieldAttrs {
            adapters: adapters.finish(&seen)?,
            seen,
            tag,
        })
    }

    /// Returns the attributes that were used on the field.
    pub fn into_seen(self) -> Vec<SeenAttr> {
        self.seen
    }

    /// Returns the adapters of the field.
    pub fn adapters(&self) -> &Adapters {
        &self.adapters
    }

    /// Returns `true` if the field receives the tag of the variant.
    pub fn tag(&self) -> bool {
        self.tag
    }
}

pub struct FieldAttrs<'a> {
    field: &'a syn::Field,
    seen: Vec<SeenAttr>,
    rename: Option<String>,
    aliases: Vec<String>,
    default: Option<TypeDefault>,
    flatten: bool,
    skip_serializing_if: Option<syn::ExprPath>,
    adapters: Adapters,
    tag: bool,
}

impl<'a> FieldAttrs<'a> {
    pub fn of(field: &'a syn::Field) -> syn::Result<FieldAttrs<'a>> {
        let mut rv = FieldAttrs {
            field,
            seen: Vec::new(),
            rename: None,
            aliases: Vec::new(),
            default: None,
            flatten: false,
            skip_serializing_if: None,
            adapters: Adapters::default(),
            tag: false,
        };
        let mut adapters = AdapterAttrs::default();

        let seen = parse_deser_attrs(&field.attrs, |name, meta| match name {
            "as" | "serialize_as" | "deserialize_as" => {
                adapters.parse(name, meta, parse_adapter)?;
                Ok(())
            }
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
                let value = parse_path(meta)?;
                set_once(meta, name, &mut rv.skip_serializing_if, value)
            }
            "tag" => set_flag(meta, name, &mut rv.tag),
            _ => Err(meta.error("unsupported attribute")),
        })?;
        rv.seen = seen;
        rv.adapters = adapters.finish(&rv.seen)?;

        if rv.flatten && rv.default.is_some() {
            return Err(syn::Error::new_spanned(
                field,
                "cannot combine flatten and default",
            ));
        }
        if rv.flatten && rv.adapters.any() {
            return Err(syn::Error::new_spanned(
                field,
                "cannot combine flatten with as, serialize_as or deserialize_as",
            ));
        }
        if rv.tag
            && (rv.rename.is_some()
                || !rv.aliases.is_empty()
                || rv.default.is_some()
                || rv.flatten
                || rv.skip_serializing_if.is_some())
        {
            return Err(syn::Error::new_spanned(
                field,
                "tag fields only support the as, serialize_as and deserialize_as attributes",
            ));
        }

        Ok(rv)
    }

    pub fn field(&self) -> &syn::Field {
        self.field
    }

    /// Returns the attributes that were used on the field.
    pub fn into_seen(self) -> Vec<SeenAttr> {
        self.seen
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

    /// Returns the adapters of the field.
    pub fn adapters(&self) -> &Adapters {
        &self.adapters
    }

    /// Returns `true` if the field receives the tag of the variant.
    pub fn tag(&self) -> bool {
        self.tag
    }
}

pub struct EnumVariantAttrs<'a> {
    variant: &'a syn::Variant,
    seen: Vec<SeenAttr>,
    rename: Option<String>,
    aliases: Vec<String>,
    other: bool,
    default: bool,
}

impl<'a> EnumVariantAttrs<'a> {
    pub fn of(variant: &'a syn::Variant) -> syn::Result<EnumVariantAttrs<'a>> {
        let mut rv = EnumVariantAttrs {
            variant,
            seen: Vec::new(),
            rename: None,
            aliases: Vec::new(),
            other: false,
            default: false,
        };

        let seen = parse_deser_attrs(&variant.attrs, |name, meta| match name {
            "rename" => {
                let value = parse_str(meta)?;
                set_once(meta, name, &mut rv.rename, value)
            }
            "alias" => {
                rv.aliases.push(parse_str(meta)?);
                Ok(())
            }
            "other" => set_flag(meta, name, &mut rv.other),
            "default" => set_flag(meta, name, &mut rv.default),
            _ => Err(meta.error("unsupported attribute")),
        })?;
        rv.seen = seen;

        Ok(rv)
    }

    /// Returns the attributes that were used on the variant.
    pub fn into_seen(self) -> Vec<SeenAttr> {
        self.seen
    }

    /// Returns `true` if this is the catch-all variant for unknown tags.
    pub fn other(&self) -> bool {
        self.other
    }

    /// Returns `true` if this variant is used if the tag is missing.
    pub fn default(&self) -> bool {
        self.default
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
