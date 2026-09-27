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

    /// Returns `true` if the adapter of a container for the direction
    /// wraps the derived implementation (`_` in the attribute).
    ///
    /// The derived implementation is then implemented as
    /// `DerivedSerialize` or `DerivedDeserialize` (see the `Derived`
    /// adapter) and the implementation of the trait forwards to the
    /// adapter.
    pub fn uses_derived(&self, direction: Direction) -> bool {
        fn contains_derived(tokens: TokenStream) -> bool {
            tokens.into_iter().any(|token| match token {
                TokenTree::Ident(ref ident) => ident == "Derived",
                TokenTree::Group(group) => contains_derived(group.stream()),
                _ => false,
            })
        }
        self.get(direction)
            .is_some_and(|ty| contains_derived(ty.to_token_stream()))
    }
}

/// A value that can differ between serialization and deserialization.
///
/// `name = value` sets it for both directions, `name(serialize = value,
/// deserialize = value)` for each direction on its own.
#[derive(Clone)]
pub struct Directional<T> {
    ser: Option<T>,
    de: Option<T>,
}

impl<T> Default for Directional<T> {
    fn default() -> Self {
        Directional {
            ser: None,
            de: None,
        }
    }
}

impl<T: Clone> Directional<T> {
    /// Parses the value of the attribute.
    fn parse(
        &mut self,
        meta: &ParseNestedMeta,
        name: &str,
        parse: impl Fn(&ParseNestedMeta) -> syn::Result<T>,
    ) -> syn::Result<()> {
        if !meta.input.peek(syn::token::Paren) {
            if self.ser.is_some() || self.de.is_some() {
                return Err(meta.error(format!("duplicate {} attribute", name)));
            }
            let value = parse(meta)?;
            self.ser = Some(value.clone());
            self.de = Some(value);
            return Ok(());
        }
        meta.parse_nested_meta(|inner| {
            let slot = if inner.path.is_ident("serialize") {
                &mut self.ser
            } else if inner.path.is_ident("deserialize") {
                &mut self.de
            } else {
                return Err(inner.error("expected `serialize` or `deserialize`"));
            };
            let value = parse(&inner)?;
            set_once(&inner, name, slot, value)
        })
    }

    /// Returns the value for a direction.
    pub fn get(&self, direction: Direction) -> Option<&T> {
        match direction {
            Direction::Serialize => self.ser.as_ref(),
            Direction::Deserialize => self.de.as_ref(),
        }
    }

    /// Returns `true` if the value is set for any direction.
    pub fn any(&self) -> bool {
        self.ser.is_some() || self.de.is_some()
    }
}

/// The custom bounds of a field.
///
/// `bound` sets them for both directions, `serialize_bound` and
/// `deserialize_bound` for one (and take precedence).
#[derive(Default, Clone)]
pub struct FieldBounds {
    both: Option<Vec<syn::WherePredicate>>,
    ser: Option<Vec<syn::WherePredicate>>,
    de: Option<Vec<syn::WherePredicate>>,
}

impl FieldBounds {
    /// Parses the attribute if it's one of the bound attributes.
    ///
    /// Returns `false` if the attribute is not a bound attribute.
    fn parse(&mut self, name: &str, meta: &ParseNestedMeta) -> syn::Result<bool> {
        let slot = match name {
            "bound" => &mut self.both,
            "serialize_bound" => &mut self.ser,
            "deserialize_bound" => &mut self.de,
            _ => return Ok(false),
        };
        let value = parse_bound(meta)?;
        set_once(meta, name, slot, value)?;
        Ok(true)
    }

    /// Returns the bounds for a direction.
    ///
    /// If this returns `Some` the predicates replace the bounds inferred
    /// from the field.
    pub fn get(&self, direction: Direction) -> Option<&[syn::WherePredicate]> {
        match direction {
            Direction::Serialize => self.ser.as_ref(),
            Direction::Deserialize => self.de.as_ref(),
        }
        .or(self.both.as_ref())
        .map(|x| &x[..])
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
    /// Converts the name of a field (in snake case) to the style.
    fn apply_to_field(self, name: &str) -> String {
        match self {
            RenameAll::LowerCase | RenameAll::SnakeCase => name.to_string(),
            RenameAll::UpperCase | RenameAll::ScreamingSnakeCase => name.to_ascii_uppercase(),
            RenameAll::PascalCase | RenameAll::CamelCase => {
                let mut rv = String::new();
                let mut capitalize = matches!(self, RenameAll::PascalCase);
                for ch in name.chars() {
                    if ch == '_' {
                        capitalize = true;
                    } else if capitalize {
                        rv.push(ch.to_ascii_uppercase());
                        capitalize = false;
                    } else {
                        rv.push(ch);
                    }
                }
                rv
            }
            RenameAll::KebabCase => name.replace("_", "-"),
            RenameAll::ScreamingKebabCase => name.replace("_", "-").to_ascii_uppercase(),
        }
    }

    /// Converts the name of a variant (in pascal case) to the style.
    fn apply_to_variant(self, name: &str) -> String {
        match self {
            RenameAll::PascalCase => name.to_string(),
            RenameAll::LowerCase => name.to_ascii_lowercase(),
            RenameAll::UpperCase => name.to_ascii_uppercase(),
            RenameAll::CamelCase => name[..1].to_ascii_lowercase() + &name[1..],
            RenameAll::SnakeCase
            | RenameAll::ScreamingSnakeCase
            | RenameAll::KebabCase
            | RenameAll::ScreamingKebabCase => {
                let sep = if matches!(self, RenameAll::SnakeCase | RenameAll::ScreamingSnakeCase) {
                    '_'
                } else {
                    '-'
                };
                let upper = matches!(
                    self,
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
    }

    /// Returns the name of the style (as given to `rename_all`).
    pub fn as_str(self) -> &'static str {
        match self {
            RenameAll::LowerCase => "lowercase",
            RenameAll::UpperCase => "UPPERCASE",
            RenameAll::PascalCase => "PascalCase",
            RenameAll::CamelCase => "camelCase",
            RenameAll::SnakeCase => "snake_case",
            RenameAll::ScreamingSnakeCase => "SCREAMING_SNAKE_CASE",
            RenameAll::KebabCase => "kebab-case",
            RenameAll::ScreamingKebabCase => "SCREAMING-KEBAB-CASE",
        }
    }

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

/// The name of a field or a type.
///
/// Names are string literals or expressions that evaluate to a
/// `&'static str` at compile time: paths to constants and macro
/// invocations (such as `concat!(...)`).
#[derive(Clone)]
pub enum Name {
    Lit(String),
    Expr(syn::Expr),
}

impl Name {
    /// Parses the value of `name = ...`.
    fn parse(meta: &ParseNestedMeta) -> syn::Result<Name> {
        let expr: syn::Expr = meta.value()?.parse()?;
        Name::from_expr(expr)
    }

    fn from_expr(expr: syn::Expr) -> syn::Result<Name> {
        match expr {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(ref lit),
                ..
            }) => Ok(Name::Lit(lit.value())),
            syn::Expr::Path(_) | syn::Expr::Macro(_) => {
                reject_self(expr.to_token_stream())?;
                Ok(Name::Expr(expr))
            }
            _ => Err(syn::Error::new_spanned(
                expr,
                "expected a string, a path to a constant or a macro invocation",
            )),
        }
    }

    /// Returns the name if it's a string literal.
    pub fn as_lit(&self) -> Option<&str> {
        match self {
            Name::Lit(name) => Some(name),
            Name::Expr(_) => None,
        }
    }

    /// Returns the name for messages of the derive.
    ///
    /// For expressions this is the expression.
    pub fn display(&self) -> String {
        match self {
            Name::Lit(name) => name.clone(),
            Name::Expr(expr) => expr.to_token_stream().to_string(),
        }
    }

    /// Returns match arms for names as `&str` that evaluate to `result`.
    ///
    /// Expressions cannot be used as patterns, they are compared in guards.
    pub fn str_arms(names: &[Name], result: TokenStream) -> TokenStream {
        name_arms(
            names,
            result,
            |name| quote! { #name },
            |binding| quote! { #binding },
        )
    }
}

/// Returns match arms for names that evaluate to `result`.
///
/// `lit_pattern` makes a pattern for a literal, `binding_pattern` a pattern
/// that binds the `&str` to compare expressions with.
fn name_arms(
    names: &[Name],
    result: TokenStream,
    lit_pattern: impl Fn(&str) -> TokenStream,
    binding_pattern: impl Fn(&syn::Ident) -> TokenStream,
) -> TokenStream {
    let lits = names
        .iter()
        .filter_map(|x| x.as_lit())
        .map(&lit_pattern)
        .collect::<Vec<_>>();
    let binding = syn::Ident::new("__name", Span::call_site());
    let pattern = binding_pattern(&binding);
    let exprs = names.iter().filter_map(|x| match x {
        Name::Expr(expr) => Some(expr),
        Name::Lit(_) => None,
    });
    let mut rv = if lits.is_empty() {
        TokenStream::new()
    } else {
        quote! { #(#lits)|* => #result, }
    };
    for expr in exprs {
        rv.extend(quote! { #pattern if #binding == (#expr) => #result, });
    }
    rv
}

impl ToTokens for Name {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Name::Lit(name) => name.to_tokens(tokens),
            Name::Expr(expr) => quote! { (#expr) }.to_tokens(tokens),
        }
    }
}

impl PartialEq for Name {
    fn eq(&self, other: &Name) -> bool {
        match (self, other) {
            (Name::Lit(a), Name::Lit(b)) => a == b,
            // expressions are compared by their tokens
            (Name::Expr(_), Name::Expr(_)) => self.display() == other.display(),
            _ => false,
        }
    }
}

impl Eq for Name {}

impl std::hash::Hash for Name {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_lit().is_some().hash(state);
        self.display().hash(state);
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
    // the direction of the derive, directional attributes are resolved for
    // it
    direction: Direction,
    seen: Vec<SeenAttr>,
    adapters: Adapters,
    rename: Option<Name>,
    rename_all: Option<RenameAll>,
    rename_all_fields: Option<RenameAll>,
    alias_all: Vec<RenameAll>,
    default: Option<TypeDefault>,
    skip_serializing_optionals: bool,
    deny_unknown_fields: bool,
    transparent: bool,
    expecting: Option<Name>,
    tag: Option<Name>,
    tag_aliases: Vec<Name>,
    content: Option<Name>,
    content_aliases: Vec<Name>,
    untagged: bool,
    // the names of the variants by their discriminants (`repr`)
    discriminants: Option<Vec<(&'a syn::Ident, VariantName)>>,
    crate_path: Option<syn::Path>,
    bound: Option<Vec<syn::WherePredicate>>,
    serialize_bound: Option<Vec<syn::WherePredicate>>,
    deserialize_bound: Option<Vec<syn::WherePredicate>>,
}

/// Where an attribute is placed.
#[derive(Copy, Clone, PartialEq, Eq)]
enum AttrLevel {
    Container,
    Variant,
    NamedField,
    UnnamedField,
}

impl AttrLevel {
    const ALL: [AttrLevel; 4] = [
        AttrLevel::Container,
        AttrLevel::Variant,
        AttrLevel::NamedField,
        AttrLevel::UnnamedField,
    ];

    /// Returns the attributes supported at the level.
    fn attrs(self) -> &'static [&'static str] {
        match self {
            AttrLevel::Container => &[
                "rename",
                "rename_all",
                "rename_all_fields",
                "alias_all",
                "tag",
                "tag_alias",
                "content",
                "content_alias",
                "untagged",
                "repr",
                "default",
                "deny_unknown_fields",
                "skip_serializing_optionals",
                "transparent",
                "expecting",
                "as",
                "serialize_as",
                "deserialize_as",
                "bound",
                "serialize_bound",
                "deserialize_bound",
                "crate",
            ],
            AttrLevel::Variant => &[
                "rename",
                "rename_all",
                "alias",
                "other",
                "default",
                "untagged",
                "deny_unknown_fields",
                "skip",
                "skip_serializing",
                "skip_deserializing",
            ],
            AttrLevel::NamedField => &[
                "rename",
                "alias",
                "default",
                "flatten",
                "required",
                "skip",
                "skip_serializing",
                "skip_deserializing",
                "skip_serializing_if",
                "as",
                "serialize_as",
                "deserialize_as",
                "bound",
                "serialize_bound",
                "deserialize_bound",
                "tag",
            ],
            AttrLevel::UnnamedField => &[
                "skip",
                "skip_serializing",
                "skip_deserializing",
                "default",
                "as",
                "serialize_as",
                "deserialize_as",
                "bound",
                "serialize_bound",
                "deserialize_bound",
                "tag",
            ],
        }
    }

    fn describe(self) -> &'static str {
        match self {
            AttrLevel::Container => "structs and enums",
            AttrLevel::Variant => "variants",
            AttrLevel::NamedField => "named fields",
            AttrLevel::UnnamedField => "unnamed fields",
        }
    }
}

/// Returns a hint for attributes which deser does not have (of serde and
/// earlier versions of deser).
fn serde_hint(name: &str) -> Option<&'static str> {
    Some(match name {
        "validate" => {
            "validation moved into `deser-validate`: use `#[deser(as = Check<V>)]` on fields \
             and `#[deser(deserialize_as = Check<V, _>)]` on types"
        }
        "with" | "serialize_with" | "deserialize_with" => {
            "deser uses adapter types instead of functions, use `as`, `serialize_as` or \
             `deserialize_as` with an adapter (see `deser::adapters`)"
        }
        "from" => "use `#[deser(deserialize_as = deser::adapters::FromInto<T>)]` instead",
        "try_from" => "use `#[deser(deserialize_as = deser::adapters::TryFromInto<T>)]` instead",
        "into" => "use `#[deser(serialize_as = deser::adapters::FromInto<T>)]` instead",
        "borrow" => {
            "references (`&str` and `&[u8]`) always borrow, `Cow` borrows with \
             `#[deser(as = deser::adapters::Borrowed)]`"
        }
        "remote" | "getter" => {
            "it's not supported, implement `SerializeAs` and `DeserializeAs` (see \
             `deser::adapters`) for a local type and use it with `as` instead"
        }
        "field_identifier" | "variant_identifier" => "it's not supported",
        _ => return None,
    })
}

/// Returns the error for an attribute that is not supported where it's
/// placed.
///
/// The error mentions where the attribute is supported, or what to use
/// instead for attributes of serde, or which attributes are supported.
fn unsupported_attr(meta: &ParseNestedMeta, name: &str, level: AttrLevel) -> syn::Error {
    if let Some(hint) = serde_hint(name) {
        return meta.error(format!("unsupported attribute `{}`: {}", name, hint));
    }
    let levels = AttrLevel::ALL
        .iter()
        .filter(|x| x.attrs().contains(&name))
        .map(|x| match x {
            AttrLevel::Container => "types",
            other => other.describe(),
        })
        .collect::<Vec<_>>();
    if !levels.is_empty() {
        return meta.error(format!(
            "`{}` is not supported on {}, it's supported on {}",
            name,
            level.describe(),
            join_list(&levels, "and")
        ));
    }
    // typos are likely, suggest an attribute with a similar name
    if let Some(similar) = level
        .attrs()
        .iter()
        .map(|x| (edit_distance(name, x), x))
        .filter(|(distance, _)| *distance <= 2)
        .min_by_key(|(distance, _)| *distance)
    {
        return meta.error(format!(
            "unknown attribute `{}`, did you mean `{}`?",
            name, similar.1
        ));
    }
    let supported = level
        .attrs()
        .iter()
        .map(|x| format!("`{}`", x))
        .collect::<Vec<_>>();
    meta.error(format!(
        "unknown attribute `{}`, the attributes of {} are {}",
        name,
        level.describe(),
        join_list(&supported, "and")
    ))
}

/// Returns the number of edits to turn one string into another.
fn edit_distance(a: &str, b: &str) -> usize {
    let b = b.chars().collect::<Vec<_>>();
    let mut row = (0..=b.len()).collect::<Vec<_>>();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if ca == *cb {
                prev
            } else {
                1 + prev.min(row[j]).min(cur)
            };
            prev = cur;
        }
    }
    row[b.len()]
}

/// Joins items into a list for messages (`a, b and c`).
fn join_list<T: AsRef<str>>(items: &[T], last: &str) -> String {
    let mut rv = String::new();
    for (idx, item) in items.iter().enumerate() {
        if idx > 0 {
            rv.push_str(if idx + 1 == items.len() { " " } else { ", " });
            if idx + 1 == items.len() {
                rv.push_str(last);
                rv.push(' ');
            }
        }
        rv.push_str(item.as_ref());
    }
    rv
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
/// that use the implementation of the container would recurse forever:
/// `Same` (which stands for the type's own implementation) and the type
/// itself are rejected as adapter and as direct type argument of the
/// adapter (as in `FromInto<Self>` or `DefaultOnError<Same>`).  Nested uses
/// such as `FromInto<Vec<Node>>` are fine.  `_` stands for the derived
/// implementation (the `Derived` adapter) in these places, as in
/// `Check<Rules, _>`.  Deeper in the type it stands for `Same` as usual.
fn parse_container_adapter(meta: &ParseNestedMeta, ident: &syn::Ident) -> syn::Result<syn::Type> {
    fn is_own_impl(ty: &syn::Type, ident: &syn::Ident) -> bool {
        match ty {
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
                 this would recurse forever (`_` refers to the derived implementation)",
                what, ident
            ),
        ));
    }

    // `_` as the adapter or a direct type argument is the derived
    // implementation
    fn derived(span: proc_macro2::Span) -> syn::Type {
        syn::parse_quote_spanned! { span=> __deser::adapters::Derived }
    }
    let mut ty = ty;
    match ty {
        syn::Type::Infer(ref infer) => ty = derived(infer.underscore_token.span),
        syn::Type::Path(ref mut path) => {
            if let Some(segment) = path.path.segments.last_mut()
                && let syn::PathArguments::AngleBracketed(ref mut args) = segment.arguments
            {
                for arg in args.args.iter_mut() {
                    if let syn::GenericArgument::Type(syn::Type::Infer(ref infer)) = *arg {
                        *arg = syn::GenericArgument::Type(derived(infer.underscore_token.span));
                    }
                }
            }
        }
        _ => {}
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
    pub fn of(
        input: &'a syn::DeriveInput,
        direction: Direction,
    ) -> syn::Result<ContainerAttrs<'a>> {
        let mut rv = ContainerAttrs {
            ident: &input.ident,
            direction,
            seen: Vec::new(),
            adapters: Adapters::default(),
            rename: None,
            rename_all: None,
            rename_all_fields: None,
            alias_all: Vec::new(),
            default: None,
            skip_serializing_optionals: false,
            deny_unknown_fields: false,
            transparent: false,
            expecting: None,
            tag: None,
            tag_aliases: Vec::new(),
            content: None,
            content_aliases: Vec::new(),
            untagged: false,
            discriminants: None,
            crate_path: None,
            bound: None,
            serialize_bound: None,
            deserialize_bound: None,
        };
        let is_enum = matches!(input.data, syn::Data::Enum(_));
        let mut adapters = AdapterAttrs::default();
        let mut rename = Directional::default();
        let mut rename_all = Directional::default();
        let mut rename_all_fields = Directional::default();

        let seen = parse_deser_attrs(&input.attrs, |name, meta| match name {
            "as" | "serialize_as" | "deserialize_as" => {
                adapters.parse(name, meta, |meta| {
                    parse_container_adapter(meta, &input.ident)
                })?;
                Ok(())
            }
            "rename_all" => {
                rename_all.parse(meta, name, |meta| RenameAll::parse(&parse_lit_str(meta)?))
            }
            "rename_all_fields" => {
                rename_all_fields
                    .parse(meta, name, |meta| RenameAll::parse(&parse_lit_str(meta)?))?;
                if !is_enum {
                    return Err(meta.error(
                        "rename_all_fields is only supported on enums, use rename_all on structs",
                    ));
                }
                Ok(())
            }
            "alias_all" => {
                rv.alias_all.push(RenameAll::parse(&parse_lit_str(meta)?)?);
                Ok(())
            }
            "rename" => rename.parse(meta, name, Name::parse),
            "tag" => {
                let value = Name::parse(meta)?;
                set_once(meta, name, &mut rv.tag, value)?;
                if !is_enum {
                    return Err(meta.error("tag is only supported on enums"));
                }
                Ok(())
            }
            "tag_alias" => {
                rv.tag_aliases.push(Name::parse(meta)?);
                Ok(())
            }
            "content" => {
                let value = Name::parse(meta)?;
                set_once(meta, name, &mut rv.content, value)
            }
            "content_alias" => {
                rv.content_aliases.push(Name::parse(meta)?);
                Ok(())
            }
            "repr" => {
                let mut repr = false;
                set_flag(meta, name, &mut repr)?;
                match input.data {
                    syn::Data::Enum(ref data) => {
                        rv.discriminants = Some(discriminant_names(data)?);
                        Ok(())
                    }
                    _ => Err(meta.error("repr is only supported on enums")),
                }
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
            "deny_unknown_fields" => set_flag(meta, name, &mut rv.deny_unknown_fields),
            "expecting" => {
                let value = Name::parse(meta)?;
                set_once(meta, name, &mut rv.expecting, value)
            }
            "transparent" => {
                set_flag(meta, name, &mut rv.transparent)?;
                match input.data {
                    syn::Data::Struct(syn::DataStruct {
                        fields: syn::Fields::Named(_) | syn::Fields::Unnamed(_),
                        ..
                    }) => Ok(()),
                    _ => Err(meta.error("transparent is only supported on structs with fields")),
                }
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
            _ => Err(unsupported_attr(meta, name, AttrLevel::Container)),
        })?;
        rv.seen = seen;
        rv.rename = rename.get(direction).cloned();
        rv.rename_all = rename_all.get(direction).copied();
        rv.rename_all_fields = rename_all_fields.get(direction).copied();

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
        if !rv.tag_aliases.is_empty() && rv.tag.is_none() {
            return Err(syn::Error::new(
                rv.span_of("tag_alias"),
                "tag_alias requires a tag attribute",
            ));
        }
        if !rv.content_aliases.is_empty() && rv.content.is_none() {
            return Err(syn::Error::new(
                rv.span_of("content_alias"),
                "content_alias requires a content attribute",
            ));
        }
        rv.check_tag_keys()?;
        if rv.discriminants.is_some() {
            rv.check_repr(input)?;
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

    /// Returns the direction of the derive.
    pub fn direction(&self) -> Direction {
        self.direction
    }

    pub fn container_name(&self) -> Name {
        match self.rename {
            Some(ref name) => name.clone(),
            None => Name::Lit(self.ident.to_string()),
        }
    }

    /// Returns the name style of the fields of struct variants.
    pub fn rename_all_fields(&self) -> Option<RenameAll> {
        self.rename_all_fields
    }

    /// Returns what is expected in errors (the name of the type unless
    /// `expecting` was given).
    pub fn expecting(&self) -> Name {
        match self.expecting {
            Some(ref expecting) => expecting.clone(),
            None => self.container_name(),
        }
    }

    /// Rejects `expecting` for types which are deserialized like other
    /// values (which report what they expect).
    pub fn reject_expecting(&self, kind: &str) -> syn::Result<()> {
        match self.expecting {
            Some(_) => Err(syn::Error::new(
                self.span_of("expecting"),
                format!(
                    "`expecting` has no effect on {}, they are deserialized like their fields",
                    kind
                ),
            )),
            None => Ok(()),
        }
    }

    pub fn get_field_name(&self, field: &syn::Field) -> String {
        let name = field.ident.as_ref().unwrap().to_string();
        match self.rename_all {
            Some(rename_all) => rename_all.apply_to_field(&name),
            None => name,
        }
    }

    /// Returns the aliases of a field from `alias_all`.
    ///
    /// Aliases that are the same as the name of the field are skipped.
    pub fn field_aliases(&self, field: &syn::Field, name: &Name) -> Vec<Name> {
        let ident = field.ident.as_ref().unwrap().to_string();
        let styles = self.alias_all.iter().map(|x| x.apply_to_field(&ident));
        unique_aliases(styles, name)
    }

    /// Returns the aliases of a variant from `alias_all`.
    ///
    /// Aliases that are the same as the name of the variant are skipped.
    pub fn variant_aliases(&self, variant: &syn::Variant, name: &VariantName) -> Vec<VariantName> {
        let ident = variant.ident.to_string();
        let mut rv: Vec<VariantName> = Vec::new();
        for alias in self.alias_all.iter().map(|x| x.apply_to_variant(&ident)) {
            let alias = VariantName::Str(Name::Lit(alias));
            if alias != *name && !rv.contains(&alias) {
                rv.push(alias);
            }
        }
        rv
    }

    pub fn default(&self) -> Option<&TypeDefault> {
        self.default.as_ref()
    }

    pub fn skip_serializing_optionals(&self) -> bool {
        self.skip_serializing_optionals
    }

    /// Rejects the attributes that only have an effect on structs with named
    /// fields and enums.
    ///
    /// `kind` describes the struct (for instance `"tuple structs"`).  Every
    /// derive only reports the attributes that affect its own direction.
    pub fn reject_named_only(&self, kind: &str, direction: Direction) -> syn::Result<()> {
        let names: &[&str] = match direction {
            Direction::Serialize => &["rename_all", "alias_all", "skip_serializing_optionals"],
            Direction::Deserialize => {
                &["rename_all", "alias_all", "default", "deny_unknown_fields"]
            }
        };
        match self.seen.iter().find(|x| names.contains(&x.name.as_str())) {
            Some(seen) => Err(syn::Error::new(
                seen.span,
                format!("`{}` has no effect on {}", seen.name, kind),
            )),
            None => Ok(()),
        }
    }

    /// Returns the span of an attribute that was used on the container.
    pub fn span_of(&self, name: &str) -> Span {
        self.seen
            .iter()
            .find(|x| x.name == name)
            .map_or_else(Span::call_site, |x| x.span)
    }

    /// Returns `true` if the struct is serialized and deserialized like its
    /// only field.
    pub fn transparent(&self) -> bool {
        self.transparent
    }

    /// Returns `true` if keys that no field takes are rejected.
    pub fn deny_unknown_fields(&self) -> bool {
        self.deny_unknown_fields
    }

    /// Returns the key of the tag of internally and adjacently tagged enums.
    pub fn tag(&self) -> Option<&Name> {
        self.tag.as_ref()
    }

    /// Returns the aliases of the tag key.
    pub fn tag_aliases(&self) -> &[Name] {
        &self.tag_aliases
    }

    /// Returns the key of the content of adjacently tagged enums.
    pub fn content(&self) -> Option<&Name> {
        self.content.as_ref()
    }

    /// Returns the aliases of the content key.
    pub fn content_aliases(&self) -> &[Name] {
        &self.content_aliases
    }

    /// Rejects tag and content keys (with their aliases) that are given more
    /// than once.
    ///
    /// Only string literals can be compared, keys from constants are
    /// compared at runtime (the tag wins).
    fn check_tag_keys(&self) -> syn::Result<()> {
        let keys = (self.tag.iter().map(|x| (x, "tag")))
            .chain(self.tag_aliases.iter().map(|x| (x, "tag_alias")))
            .chain(self.content.iter().map(|x| (x, "content")))
            .chain(self.content_aliases.iter().map(|x| (x, "content_alias")))
            .filter_map(|(name, attr)| Some((name.as_lit()?, attr)))
            .collect::<Vec<_>>();
        for (idx, (key, attr)) in keys.iter().enumerate() {
            if keys[..idx].iter().any(|(other, _)| other == key) {
                return Err(syn::Error::new(
                    self.span_of(attr),
                    format!("`{}` is used more than once as tag or content key", key),
                ));
            }
        }
        Ok(())
    }

    /// Returns the name of a variant if variants are named by their
    /// discriminants.
    pub fn discriminant_name(&self, variant: &syn::Variant) -> Option<&VariantName> {
        self.discriminants
            .as_ref()?
            .iter()
            .find(|(ident, _)| **ident == variant.ident)
            .map(|(_, name)| name)
    }

    /// Rejects the attributes that name variants in other ways than their
    /// discriminants.
    fn check_repr(&self, input: &syn::DeriveInput) -> syn::Result<()> {
        for attr in ["rename_all", "alias_all", "untagged"] {
            if let Some(seen) = self.seen.iter().find(|x| x.name == attr) {
                return Err(syn::Error::new(
                    seen.span,
                    format!("`{}` has no effect together with `repr`", attr),
                ));
            }
        }
        if let syn::Data::Enum(ref data) = input.data {
            for variant in &data.variants {
                let seen = EnumVariantAttrs::of(variant)?.into_seen();
                if let Some(seen) = seen.iter().find(|x| x.name == "rename") {
                    return Err(syn::Error::new(
                        seen.span,
                        "variants are named by their discriminants with `repr`",
                    ));
                }
            }
        }
        Ok(())
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
        match self.rename_all {
            Some(rename_all) => rename_all.apply_to_variant(&name),
            None => name,
        }
    }
}

/// Returns the aliases that differ from the name, without duplicates.
fn unique_aliases(aliases: impl Iterator<Item = String>, name: &Name) -> Vec<Name> {
    let mut rv: Vec<Name> = Vec::new();
    for alias in aliases {
        if name.as_lit() != Some(alias.as_str()) && !rv.iter().any(|x| x.as_lit() == Some(&alias)) {
            rv.push(Name::Lit(alias));
        }
    }
    rv
}

/// The attributes of unnamed fields (of newtype structs and tuple variants).
pub struct UnnamedFieldAttrs {
    seen: Vec<SeenAttr>,
    adapters: Adapters,
    bounds: FieldBounds,
    tag: bool,
    default: Option<TypeDefault>,
    skip_serializing: bool,
    skip_deserializing: bool,
}

impl UnnamedFieldAttrs {
    pub fn of(field: &syn::Field) -> syn::Result<UnnamedFieldAttrs> {
        let mut rv = UnnamedFieldAttrs {
            seen: Vec::new(),
            adapters: Adapters::default(),
            bounds: FieldBounds::default(),
            tag: false,
            default: None,
            skip_serializing: false,
            skip_deserializing: false,
        };
        let mut skip = false;
        let mut adapters = AdapterAttrs::default();
        let seen = parse_deser_attrs(&field.attrs, |name, meta| {
            if adapters.parse(name, meta, parse_adapter)? || rv.bounds.parse(name, meta)? {
                return Ok(());
            }
            match name {
                "tag" => set_flag(meta, name, &mut rv.tag),
                "default" => {
                    let value = parse_default(meta)?;
                    set_once(meta, name, &mut rv.default, value)
                }
                "skip" => set_flag(meta, name, &mut skip),
                "skip_serializing" => set_flag(meta, name, &mut rv.skip_serializing),
                "skip_deserializing" => set_flag(meta, name, &mut rv.skip_deserializing),
                _ => Err(unsupported_attr(meta, name, AttrLevel::UnnamedField)),
            }
        })?;
        rv.seen = seen;
        rv.adapters = adapters.finish(&rv.seen)?;

        // attributes that have no effect with the skips are rejected
        let conflict = |name: &str, others: &[&str]| -> syn::Result<()> {
            match rv.seen.iter().find(|x| others.contains(&x.name.as_str())) {
                Some(other) => Err(syn::Error::new(
                    other.span,
                    format!("`{}` has no effect together with `{}`", other.name, name),
                )),
                None => Ok(()),
            }
        };
        if skip {
            if rv.skip_serializing || rv.skip_deserializing {
                return Err(syn::Error::new_spanned(
                    field,
                    "skip already skips serialization and deserialization",
                ));
            }
            conflict("skip", &["as", "serialize_as", "deserialize_as", "tag"])?;
            rv.skip_serializing = true;
            rv.skip_deserializing = true;
        }
        if rv.skip_serializing && !skip {
            conflict("skip_serializing", &["serialize_as", "tag"])?;
        }
        if rv.skip_deserializing && !skip {
            conflict("skip_deserializing", &["deserialize_as", "tag"])?;
        }
        // unnamed fields are identified by their position, they cannot be
        // missing (only skipped)
        if rv.default.is_some() && !rv.skip_deserializing {
            return Err(syn::Error::new(
                rv.seen
                    .iter()
                    .find(|x| x.name == "default")
                    .map_or_else(Span::call_site, |x| x.span),
                "`default` on unnamed fields is the value of skipped fields, it requires \
                 `skip` or `skip_deserializing`",
            ));
        }

        Ok(rv)
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

    /// Returns the custom bounds of the field.
    pub fn bounds(&self) -> &FieldBounds {
        &self.bounds
    }

    /// Returns the value of the field if it's skipped when deserializing.
    pub fn default(&self) -> Option<&TypeDefault> {
        self.default.as_ref()
    }

    /// Returns `true` if the field is not serialized.
    pub fn skip_serializing(&self) -> bool {
        self.skip_serializing
    }

    /// Returns `true` if the field is not deserialized.
    pub fn skip_deserializing(&self) -> bool {
        self.skip_deserializing
    }

    /// Returns `true` if the field is skipped in the direction.
    pub fn skipped(&self, direction: Direction) -> bool {
        match direction {
            Direction::Serialize => self.skip_serializing,
            Direction::Deserialize => self.skip_deserializing,
        }
    }
}

pub struct FieldAttrs<'a> {
    field: &'a syn::Field,
    seen: Vec<SeenAttr>,
    rename: Directional<Name>,
    aliases: Vec<Name>,
    default: Option<TypeDefault>,
    flatten: bool,
    skip_serializing_if: Option<syn::ExprPath>,
    skip_serializing: bool,
    skip_deserializing: bool,
    required: bool,
    adapters: Adapters,
    bounds: FieldBounds,
    tag: bool,
}

impl<'a> FieldAttrs<'a> {
    pub fn of(field: &'a syn::Field) -> syn::Result<FieldAttrs<'a>> {
        let mut rv = FieldAttrs {
            field,
            seen: Vec::new(),
            rename: Directional::default(),
            aliases: Vec::new(),
            default: None,
            flatten: false,
            skip_serializing_if: None,
            skip_serializing: false,
            skip_deserializing: false,
            required: false,
            adapters: Adapters::default(),
            bounds: FieldBounds::default(),
            tag: false,
        };
        let mut skip = false;
        let mut adapters = AdapterAttrs::default();

        let seen = parse_deser_attrs(&field.attrs, |name, meta| match name {
            "as" | "serialize_as" | "deserialize_as" => {
                adapters.parse(name, meta, parse_adapter)?;
                Ok(())
            }
            "bound" | "serialize_bound" | "deserialize_bound" => {
                rv.bounds.parse(name, meta)?;
                Ok(())
            }
            "rename" => rv.rename.parse(meta, name, Name::parse),
            "alias" => {
                rv.aliases.push(Name::parse(meta)?);
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
            "skip" => set_flag(meta, name, &mut skip),
            "skip_serializing" => set_flag(meta, name, &mut rv.skip_serializing),
            "skip_deserializing" => set_flag(meta, name, &mut rv.skip_deserializing),
            "required" => set_flag(meta, name, &mut rv.required),
            "tag" => set_flag(meta, name, &mut rv.tag),
            _ => Err(unsupported_attr(meta, name, AttrLevel::NamedField)),
        })?;
        rv.seen = seen;
        rv.adapters = adapters.finish(&rv.seen)?;

        // attributes that have no effect with the skips are rejected
        let conflict = |name: &str, others: &[&str]| -> syn::Result<()> {
            match rv.seen.iter().find(|x| others.contains(&x.name.as_str())) {
                Some(other) => Err(syn::Error::new(
                    other.span,
                    format!("`{}` has no effect together with `{}`", other.name, name),
                )),
                None => Ok(()),
            }
        };
        if skip {
            if rv.skip_serializing || rv.skip_deserializing {
                return Err(syn::Error::new_spanned(
                    field,
                    "skip already skips serialization and deserialization",
                ));
            }
            conflict(
                "skip",
                &[
                    "rename",
                    "alias",
                    "flatten",
                    "skip_serializing_if",
                    "required",
                    "as",
                    "serialize_as",
                    "deserialize_as",
                    "tag",
                ],
            )?;
            rv.skip_serializing = true;
            rv.skip_deserializing = true;
        }
        if rv.skip_serializing && !skip {
            conflict(
                "skip_serializing",
                &["skip_serializing_if", "serialize_as", "flatten", "tag"],
            )?;
        }
        if rv.skip_deserializing && !skip {
            conflict(
                "skip_deserializing",
                &["alias", "required", "deserialize_as", "flatten", "tag"],
            )?;
        }
        if rv.required {
            conflict("required", &["default", "flatten", "tag"])?;
        }

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
            && (rv.rename.any()
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

    pub fn name(&self, container_attrs: &ContainerAttrs) -> Name {
        self.rename
            .get(container_attrs.direction())
            .cloned()
            .unwrap_or_else(|| Name::Lit(container_attrs.get_field_name(self.field)))
    }

    /// Returns the aliases of the field, including those of `alias_all`.
    pub fn aliases(&self, container_attrs: &ContainerAttrs) -> Vec<Name> {
        let mut rv = self.aliases.clone();
        let name = self.name(container_attrs);
        for alias in container_attrs.field_aliases(self.field, &name) {
            if !rv.contains(&alias) {
                rv.push(alias);
            }
        }
        rv
    }

    /// Returns the name of the field of a struct variant.
    ///
    /// `style` is the name style of the fields of the variant.
    pub fn variant_field_name(&self, direction: Direction, style: Option<RenameAll>) -> Name {
        self.rename.get(direction).cloned().unwrap_or_else(|| {
            let name = self.field.ident.as_ref().unwrap().to_string();
            Name::Lit(match style {
                Some(style) => style.apply_to_field(&name),
                None => name,
            })
        })
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

    /// Returns `true` if the field is not serialized.
    pub fn skip_serializing(&self) -> bool {
        self.skip_serializing
    }

    /// Returns `true` if the field is not deserialized.
    pub fn skip_deserializing(&self) -> bool {
        self.skip_deserializing
    }

    /// Returns `true` if the field has to be given, even if its type has a
    /// value for missing fields (like `Option`).
    pub fn required(&self) -> bool {
        self.required
    }

    /// Returns the adapters of the field.
    pub fn adapters(&self) -> &Adapters {
        &self.adapters
    }

    /// Returns the custom bounds of the field.
    pub fn bounds(&self) -> &FieldBounds {
        &self.bounds
    }

    /// Returns `true` if the field receives the tag of the variant.
    pub fn tag(&self) -> bool {
        self.tag
    }
}

/// The name of a variant (its tag).
///
/// Variants are named by strings, integers or booleans.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum VariantName {
    Str(Name),
    U64(u64),
    I64(i64),
    Bool(bool),
}

impl VariantName {
    /// Parses the value of `rename = ...` or `alias = ...` of a variant.
    fn parse(meta: &ParseNestedMeta) -> syn::Result<VariantName> {
        let expr: syn::Expr = meta.value()?.parse()?;
        let (negative, lit) = match expr {
            syn::Expr::Lit(syn::ExprLit { ref lit, .. }) => (false, lit),
            syn::Expr::Unary(syn::ExprUnary {
                op: syn::UnOp::Neg(_),
                expr: ref inner,
                ..
            }) => match **inner {
                syn::Expr::Lit(syn::ExprLit {
                    lit: ref lit @ syn::Lit::Int(_),
                    ..
                }) => (true, lit),
                _ => return Err(unsupported_name(&expr)),
            },
            syn::Expr::Path(_) | syn::Expr::Macro(_) => {
                return Ok(VariantName::Str(Name::from_expr(expr)?));
            }
            _ => return Err(unsupported_name(&expr)),
        };
        Ok(match lit {
            syn::Lit::Str(lit) => VariantName::Str(Name::Lit(lit.value())),
            syn::Lit::Bool(lit) => VariantName::Bool(lit.value),
            syn::Lit::Int(lit) if negative => {
                let value: i128 = lit.base10_parse()?;
                match i64::try_from(-value) {
                    Ok(value) => VariantName::I64(value),
                    Err(_) => return Err(syn::Error::new_spanned(lit, "integer is out of range")),
                }
            }
            syn::Lit::Int(lit) => VariantName::U64(lit.base10_parse()?),
            _ => return Err(unsupported_name(&expr)),
        })
    }

    /// Returns the name if it's a string.
    pub fn as_str(&self) -> Option<&Name> {
        match self {
            VariantName::Str(name) => Some(name),
            _ => None,
        }
    }

    /// Returns the name as it appears in messages of the derive.
    pub fn display(&self) -> String {
        match self {
            VariantName::Str(name) => name.display(),
            VariantName::U64(value) => value.to_string(),
            VariantName::I64(value) => value.to_string(),
            VariantName::Bool(value) => value.to_string(),
        }
    }

    /// Returns an expression for the name as `&str` for descriptions and
    /// errors.
    pub fn str_expr(&self) -> TokenStream {
        match self {
            VariantName::Str(name) => quote! { #name },
            other => {
                let name = other.display();
                quote! { #name }
            }
        }
    }

    /// Returns match arms for names as `__deser::__derive::Tag` that
    /// evaluate to `result`.
    pub fn tag_arms(names: &[VariantName], result: TokenStream) -> TokenStream {
        let strs = names
            .iter()
            .filter_map(|x| x.as_str().cloned())
            .collect::<Vec<_>>();
        let mut rv = name_arms(
            &strs,
            result.clone(),
            |name| quote! { __deser::__derive::Tag::Str(#name) },
            |binding| quote! { __deser::__derive::Tag::Str(#binding) },
        );
        let others = names
            .iter()
            .filter_map(|x| match x {
                VariantName::Str(_) => None,
                VariantName::U64(value) => Some(quote! { __deser::__derive::Tag::U64(#value) }),
                VariantName::I64(value) => Some(quote! { __deser::__derive::Tag::I64(#value) }),
                VariantName::Bool(value) => Some(quote! { __deser::__derive::Tag::Bool(#value) }),
            })
            .collect::<Vec<_>>();
        if !others.is_empty() {
            rv.extend(quote! { #(#others)|* => #result, });
        }
        rv
    }

    /// Returns an expression for the name as atom.
    pub fn atom(&self) -> TokenStream {
        match self {
            VariantName::Str(name) => quote! {
                __deser::Atom::Str(__deser::__derive::Cow::Borrowed(#name))
            },
            VariantName::U64(value) => quote! { __deser::Atom::U64(#value) },
            VariantName::I64(value) => quote! { __deser::Atom::I64(#value) },
            VariantName::Bool(value) => quote! { __deser::Atom::Bool(#value) },
        }
    }

    /// Returns an expression for a serialize handle of the name.
    pub fn ser_handle(&self) -> TokenStream {
        let value = match self {
            VariantName::Str(name) => quote! { #name },
            VariantName::U64(value) => quote! { #value },
            VariantName::I64(value) => quote! { #value },
            VariantName::Bool(value) => quote! { #value },
        };
        quote! { __deser::ser::SerializeHandle::to(&#value) }
    }
}

/// Returns the names of the variants of an enum by their discriminants.
///
/// Discriminants that are not given are the previous one plus one (or zero
/// for the first variant), like in Rust.  Only integer literals are
/// supported as the derive has to know the values.
fn discriminant_names(data: &syn::DataEnum) -> syn::Result<Vec<(&syn::Ident, VariantName)>> {
    let mut rv = Vec::new();
    let mut next = 0i128;
    for variant in &data.variants {
        let value = match variant.discriminant {
            Some((_, ref expr)) => discriminant_value(expr)?,
            None => next,
        };
        let name = match u64::try_from(value) {
            Ok(value) => VariantName::U64(value),
            Err(_) => match i64::try_from(value) {
                Ok(value) => VariantName::I64(value),
                Err(_) => {
                    return Err(syn::Error::new_spanned(
                        variant,
                        "discriminant is out of range",
                    ));
                }
            },
        };
        rv.push((&variant.ident, name));
        next = value + 1;
    }
    Ok(rv)
}

/// Returns the value of a discriminant that is an integer literal.
fn discriminant_value(expr: &syn::Expr) -> syn::Result<i128> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(lit),
            ..
        }) => lit.base10_parse(),
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr: inner,
            ..
        }) => discriminant_value(inner).map(|x| -x),
        syn::Expr::Group(syn::ExprGroup { expr: inner, .. })
        | syn::Expr::Paren(syn::ExprParen { expr: inner, .. }) => discriminant_value(inner),
        _ => Err(syn::Error::new_spanned(
            expr,
            "repr requires integer literals as discriminants",
        )),
    }
}

fn unsupported_name(expr: &syn::Expr) -> syn::Error {
    syn::Error::new_spanned(
        expr,
        "expected a string, an integer or a boolean as name of the variant",
    )
}

pub struct EnumVariantAttrs<'a> {
    variant: &'a syn::Variant,
    seen: Vec<SeenAttr>,
    rename: Directional<VariantName>,
    rename_all: Directional<RenameAll>,
    aliases: Vec<VariantName>,
    other: bool,
    default: bool,
    untagged: bool,
    deny_unknown_fields: bool,
    skip_serializing: bool,
    skip_deserializing: bool,
}

impl<'a> EnumVariantAttrs<'a> {
    pub fn of(variant: &'a syn::Variant) -> syn::Result<EnumVariantAttrs<'a>> {
        let mut rv = EnumVariantAttrs {
            variant,
            seen: Vec::new(),
            rename: Directional::default(),
            rename_all: Directional::default(),
            aliases: Vec::new(),
            other: false,
            default: false,
            untagged: false,
            deny_unknown_fields: false,
            skip_serializing: false,
            skip_deserializing: false,
        };

        let mut skip = false;
        let seen = parse_deser_attrs(&variant.attrs, |name, meta| match name {
            "rename" => rv.rename.parse(meta, name, VariantName::parse),
            "rename_all" => {
                rv.rename_all
                    .parse(meta, name, |meta| RenameAll::parse(&parse_lit_str(meta)?))?;
                if !matches!(variant.fields, syn::Fields::Named(_)) {
                    return Err(
                        meta.error("rename_all on variants renames the fields of struct variants")
                    );
                }
                Ok(())
            }
            "alias" => {
                rv.aliases.push(VariantName::parse(meta)?);
                Ok(())
            }
            "other" => set_flag(meta, name, &mut rv.other),
            "default" => set_flag(meta, name, &mut rv.default),
            "deny_unknown_fields" => set_flag(meta, name, &mut rv.deny_unknown_fields),
            "untagged" => set_flag(meta, name, &mut rv.untagged),
            "skip" => set_flag(meta, name, &mut skip),
            "skip_serializing" => set_flag(meta, name, &mut rv.skip_serializing),
            "skip_deserializing" => set_flag(meta, name, &mut rv.skip_deserializing),
            _ => Err(unsupported_attr(meta, name, AttrLevel::Variant)),
        })?;
        rv.seen = seen;

        let conflict = |name: &str, others: &[&str]| -> syn::Result<()> {
            match rv.seen.iter().find(|x| others.contains(&x.name.as_str())) {
                Some(other) => Err(syn::Error::new(
                    other.span,
                    format!("`{}` has no effect together with `{}`", other.name, name),
                )),
                None => Ok(()),
            }
        };
        if skip {
            if rv.skip_serializing || rv.skip_deserializing {
                return Err(syn::Error::new_spanned(
                    variant,
                    "skip already skips serialization and deserialization",
                ));
            }
            conflict(
                "skip",
                &[
                    "rename",
                    "rename_all",
                    "alias",
                    "other",
                    "default",
                    "deny_unknown_fields",
                    "untagged",
                ],
            )?;
            rv.skip_serializing = true;
            rv.skip_deserializing = true;
        } else if rv.skip_deserializing {
            conflict(
                "skip_deserializing",
                &["alias", "other", "default", "deny_unknown_fields"],
            )?;
        }

        if rv.untagged {
            // untagged variants have no name and are not selected by tags
            conflict("untagged", &["rename", "alias", "other", "default"])?;
        }

        Ok(rv)
    }

    /// Returns `true` if the variant is not tagged (in an enum which is).
    pub fn untagged(&self) -> bool {
        self.untagged
    }

    /// Returns `true` if the variant cannot be serialized.
    pub fn skip_serializing(&self) -> bool {
        self.skip_serializing
    }

    /// Returns `true` if the variant cannot be deserialized.
    pub fn skip_deserializing(&self) -> bool {
        self.skip_deserializing
    }

    /// Returns the attributes that were used on the variant.
    pub fn into_seen(self) -> Vec<SeenAttr> {
        self.seen
    }

    /// Returns the name style of the fields of the variant.
    ///
    /// This is the one of the variant or the one for the fields of all
    /// variants of the enum.
    pub fn fields_rename_all(&self, container_attrs: &ContainerAttrs) -> Option<RenameAll> {
        self.rename_all
            .get(container_attrs.direction())
            .copied()
            .or(container_attrs.rename_all_fields())
    }

    /// Returns `true` if this is the catch-all variant for unknown tags.
    pub fn other(&self) -> bool {
        self.other
    }

    /// Returns `true` if the variant rejects unknown fields.
    pub fn deny_unknown_fields(&self) -> bool {
        self.deny_unknown_fields
    }

    /// Returns `true` if this variant is used if the tag is missing.
    pub fn default(&self) -> bool {
        self.default
    }

    pub fn variant(&self) -> &syn::Variant {
        self.variant
    }

    pub fn name(&self, container_attrs: &ContainerAttrs) -> VariantName {
        if let Some(name) = container_attrs.discriminant_name(self.variant) {
            return name.clone();
        }
        self.rename
            .get(container_attrs.direction())
            .cloned()
            .unwrap_or_else(|| {
                VariantName::Str(Name::Lit(container_attrs.get_variant_name(self.variant)))
            })
    }

    /// Returns the aliases of the variant, including those of `alias_all`.
    pub fn aliases(&self, container_attrs: &ContainerAttrs) -> Vec<VariantName> {
        let mut rv = self.aliases.clone();
        let name = self.name(container_attrs);
        for alias in container_attrs.variant_aliases(self.variant, &name) {
            if !rv.contains(&alias) {
                rv.push(alias);
            }
        }
        rv
    }
}
