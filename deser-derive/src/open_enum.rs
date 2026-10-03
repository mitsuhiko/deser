//! The attribute macros of open enums (`#[deser::open_enum]` and
//! `#[deser::variant]`).
//!
//! An open enum is a trait whose implementations are its variants.  The
//! trait gets a hidden method that returns the variant of a value, which
//! `#[deser::variant]` implements.  This also makes the attribute required
//! on every implementation (without it the method is missing).
//!
//! `#[deser::variant]` also implements `OpenVariant<dyn Trait>` for the type,
//! which is what the variants are registered with (in `OpenEnums`).  It
//! contains the names of the variant and the conversion into the trait
//! object, deser-core creates the rest from it.
//!
//! The names of the variants are given by the trait (`rename_all`), which
//! `#[deser::variant]` does not know.  The variants contain their name in
//! all styles and the trait picks one.
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;

use crate::attr::{Name, RenameAll, VariantName, ident_name, set_flag, set_once};

/// The configuration of an open enum.
struct OpenEnumArgs {
    rename: Option<Name>,
    tag: Option<Name>,
    tag_aliases: Vec<Name>,
    content: Option<Name>,
    content_aliases: Vec<Name>,
    rename_all: Option<RenameAll>,
    alias_all: Vec<RenameAll>,
    deny_unknown_fields: bool,
    untagged: bool,
    crate_path: Option<syn::Path>,
}

impl OpenEnumArgs {
    fn parse(args: TokenStream) -> syn::Result<OpenEnumArgs> {
        let mut rv = OpenEnumArgs {
            rename: None,
            tag: None,
            tag_aliases: Vec::new(),
            content: None,
            content_aliases: Vec::new(),
            rename_all: None,
            alias_all: Vec::new(),
            deny_unknown_fields: false,
            untagged: false,
            crate_path: None,
        };
        let parser = syn::meta::parser(|meta| {
            let name = match meta.path.get_ident() {
                Some(ident) => ident.to_string(),
                None => return Err(meta.error("unsupported attribute")),
            };
            match name.as_str() {
                "rename" => {
                    let value = Name::parse(&meta)?;
                    set_once(&meta, &name, &mut rv.rename, value)
                }
                "tag" => {
                    let value = Name::parse(&meta)?;
                    set_once(&meta, &name, &mut rv.tag, value)
                }
                "tag_alias" => {
                    rv.tag_aliases.push(Name::parse(&meta)?);
                    Ok(())
                }
                "content" => {
                    let value = Name::parse(&meta)?;
                    set_once(&meta, &name, &mut rv.content, value)
                }
                "content_alias" => {
                    rv.content_aliases.push(Name::parse(&meta)?);
                    Ok(())
                }
                "rename_all" => {
                    let value = RenameAll::parse_meta(&meta)?;
                    set_once(&meta, &name, &mut rv.rename_all, value)
                }
                "alias_all" => {
                    rv.alias_all.push(RenameAll::parse_meta(&meta)?);
                    Ok(())
                }
                "deny_unknown_fields" => set_flag(&meta, &name, &mut rv.deny_unknown_fields),
                "crate" => {
                    let value = meta.value()?.parse().map_err(|err| {
                        syn::Error::new(err.span(), "expected a path to the crate")
                    })?;
                    set_once(&meta, &name, &mut rv.crate_path, value)
                }
                "untagged" => set_flag(&meta, &name, &mut rv.untagged),
                _ => Err(meta.error(format!(
                    "unsupported attribute `{}` on open enums (supported are `tag`, \
                     `tag_alias`, `content`, `content_alias`, `untagged`, `rename`, \
                     `rename_all`, `alias_all`, `deny_unknown_fields` and `crate`)",
                    name
                ))),
            }
        });
        syn::parse::Parser::parse2(parser, args)?;

        if rv.untagged && rv.tag.is_some() {
            return Err(syn::Error::new(
                Span::call_site(),
                "untagged cannot be combined with tag",
            ));
        }
        if rv.tag.is_none() {
            for (names, attr) in [
                (&rv.tag_aliases, "tag_alias"),
                (&rv.content_aliases, "content_alias"),
            ] {
                if !names.is_empty() {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        format!("{} requires tag", attr),
                    ));
                }
            }
            if rv.content.is_some() {
                return Err(syn::Error::new(Span::call_site(), "content requires tag"));
            }
        }
        if rv.content.is_none() && !rv.content_aliases.is_empty() {
            return Err(syn::Error::new(
                Span::call_site(),
                "content_alias requires content",
            ));
        }
        if rv.deny_unknown_fields && rv.content.is_none() {
            return Err(syn::Error::new(
                Span::call_site(),
                "deny_unknown_fields only has an effect on adjacently tagged open enums \
                 (with tag and content), the variants deny their unknown fields themselves",
            ));
        }
        // like the derive, a key cannot be given twice
        let mut keys: Vec<&str> = Vec::new();
        for name in rv
            .tag
            .iter()
            .chain(&rv.tag_aliases)
            .chain(&rv.content)
            .chain(&rv.content_aliases)
        {
            if let Some(lit) = name.as_lit() {
                if keys.contains(&lit) {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        format!("`{}` is used more than once as tag or content key", lit),
                    ));
                }
                keys.push(lit);
            }
        }
        Ok(rv)
    }

    fn deser_path(&self) -> TokenStream {
        deser_path(self.crate_path.as_ref())
    }
}

/// Returns the path to deser.
fn deser_path(crate_path: Option<&syn::Path>) -> TokenStream {
    match crate_path {
        Some(path) => quote! { #path },
        None => quote! { ::deser },
    }
}

/// Returns an `EnumKey` for the tag or content key.
fn enum_key(deser: &TokenStream, name: &Name, aliases: &[Name]) -> TokenStream {
    quote! {
        #deser::__derive::EnumKey {
            name: #name,
            aliases: &[#(#aliases),*],
        }
    }
}

/// The name of the hidden method of the trait.
const METHOD: &str = "__deser_variant";

/// Expands `#[deser::open_enum]` on a trait.
pub(crate) fn expand_open_enum(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let args = OpenEnumArgs::parse(args)?;
    let mut item: syn::ItemTrait = match syn::parse2(input) {
        Ok(item) => item,
        Err(err) => {
            return Err(syn::Error::new(
                err.span(),
                "#[deser::open_enum] can only be placed on traits",
            ));
        }
    };
    if !item.generics.params.is_empty() || item.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "open enums cannot have generic parameters",
        ));
    }
    // the trait objects are serialized and deserialized like other values,
    // which are `Send` and `Sync`.  Bounds that imply them (through other
    // traits) are not seen here, they have to be given explicitly.
    for bound in ["Send", "Sync"] {
        let found = item.supertraits.iter().any(|supertrait| match supertrait {
            syn::TypeParamBound::Trait(supertrait) => supertrait
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == bound),
            _ => false,
        });
        if !found {
            return Err(syn::Error::new_spanned(
                &item.ident,
                "open enums need `Send` and `Sync` as supertraits (values that are \
                 serialized and deserialized are `Send` and `Sync`)",
            ));
        }
    }
    let deser = args.deser_path();
    let ident = &item.ident;
    let method = syn::Ident::new(METHOD, Span::call_site());

    item.items.push(syn::parse_quote! {
        #[doc(hidden)]
        fn #method(&self) -> #deser::__derive::VariantValue<'_>;
    });

    let name = match args.rename {
        Some(ref name) => name.clone(),
        None => Name::Lit(ident_name(ident)),
    };
    let repr = match (&args.tag, &args.content) {
        (None, _) if args.untagged => quote! { #deser::__derive::OpenRepr::Untagged },
        (None, _) => quote! { #deser::__derive::OpenRepr::External },
        (Some(tag), None) => {
            let tag = enum_key(&deser, tag, &args.tag_aliases);
            quote! { #deser::__derive::OpenRepr::Internal { tag: #tag } }
        }
        (Some(tag), Some(content)) => {
            let tag = enum_key(&deser, tag, &args.tag_aliases);
            let content = enum_key(&deser, content, &args.content_aliases);
            let deny = args.deny_unknown_fields;
            quote! {
                #deser::__derive::OpenRepr::Adjacent {
                    tag: #tag,
                    content: #content,
                    deny_unknown_fields: #deny,
                }
            }
        }
    };
    let rename_all = args
        .rename_all
        .unwrap_or(RenameAll::PascalCase)
        .style_index();
    let mut alias_all = Vec::new();
    for style in &args.alias_all {
        alias_all.push(style.style_index());
    }

    let object = quote! { dyn #ident };

    Ok(quote! {
        #item

        #[doc(hidden)]
        const _: () = {
            #[automatically_derived]
            impl #deser::OpenEnum for #object {
                const INFO: &'static #deser::__derive::OpenEnumInfo = &#deser::__derive::OpenEnumInfo {
                    name: #name,
                    repr: #repr,
                    rename_all: #rename_all,
                    alias_all: &[#(#alias_all),*],
                };

                #[inline]
                fn __private_variant(value: &Self) -> #deser::__derive::VariantValue<'_> {
                    value.#method()
                }
            }

            #[automatically_derived]
            impl #deser::Serialize for dyn #ident {
                fn serialize<'__a>(
                    __value: &'__a Self,
                    __state: &mut #deser::State,
                ) -> #deser::__derive::Result<#deser::ser::Emit<'__a>> {
                    #deser::__derive::open_enum_serialize::<Self>(__value, __state)
                }

                fn describe(__value: &Self, __d: &mut dyn #deser::ser::Describe) {
                    #deser::__derive::open_enum_describe::<Self>(__value, __d)
                }

                fn container_shape(_: &Self) -> #deser::ContainerShape {
                    #deser::__derive::open_enum_container_shape::<Self>()
                }
            }

            #[automatically_derived]
            impl<'de> #deser::Deserialize<'de> for #deser::__derive::Box<dyn #ident> {
                fn deserialize_into<'__out>(
                    __slot: &'__out mut #deser::__derive::Option<Self>,
                    __state: &mut #deser::State,
                ) -> #deser::de::SinkHandle<'__out, 'de> {
                    #deser::__derive::open_enum_deserialize_box::<dyn #ident>(__slot, __state)
                }

                fn expecting() -> #deser::__derive::StrCow<'static> {
                    #deser::__derive::StrCow::Borrowed(#name)
                }
            }

            #[automatically_derived]
            impl<'de> #deser::__derive::DeserializeArc<'de, dyn #ident> for dyn #ident {
                fn __private_arc_into<'__out>(
                    __slot: &'__out mut #deser::__derive::Option<#deser::__derive::Arc<Self>>,
                    __state: &mut #deser::State,
                ) -> #deser::de::SinkHandle<'__out, 'de> {
                    #deser::__derive::open_enum_deserialize_arc::<Self>(__slot, __state)
                }

                fn __private_arc_expecting() -> #deser::__derive::StrCow<'static> {
                    #deser::__derive::StrCow::Borrowed(#name)
                }
            }
        };
    })
}

/// The configuration of a variant.
struct VariantArgs {
    rename: Option<VariantName>,
    aliases: Vec<VariantName>,
    crate_path: Option<syn::Path>,
}

impl VariantArgs {
    fn parse(args: TokenStream) -> syn::Result<VariantArgs> {
        let mut rv = VariantArgs {
            rename: None,
            aliases: Vec::new(),
            crate_path: None,
        };
        let parser = syn::meta::parser(|meta| {
            let name = match meta.path.get_ident() {
                Some(ident) => ident.to_string(),
                None => return Err(meta.error("unsupported attribute")),
            };
            match name.as_str() {
                "rename" => {
                    let value = VariantName::parse(&meta)?;
                    set_once(&meta, &name, &mut rv.rename, value)
                }
                "alias" => {
                    rv.aliases.push(VariantName::parse(&meta)?);
                    Ok(())
                }
                "crate" => {
                    let value = meta.value()?.parse().map_err(|err| {
                        syn::Error::new(err.span(), "expected a path to the crate")
                    })?;
                    set_once(&meta, &name, &mut rv.crate_path, value)
                }
                _ => Err(meta.error(format!(
                    "unsupported attribute `{}` on variants of open enums (supported are \
                     `rename`, `alias` and `crate`)",
                    name
                ))),
            }
        });
        syn::parse::Parser::parse2(parser, args)?;
        Ok(rv)
    }
}

/// Returns the name of a variant as `Tag`.
fn tag(deser: &TokenStream, name: &VariantName) -> TokenStream {
    match name {
        VariantName::Str(name) => quote! { #deser::__derive::Tag::Str(#name) },
        VariantName::U64(value) => quote! { #deser::__derive::Tag::U64(#value) },
        VariantName::I64(value) => quote! { #deser::__derive::Tag::I64(#value) },
        VariantName::Bool(value) => quote! { #deser::__derive::Tag::Bool(#value) },
    }
}

/// Returns the name of a type for the default name of a variant.
///
/// This is the last segment of a path without generic arguments.
fn type_ident(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(syn::TypePath {
            qself: None, path, ..
        }) => {
            let last = path.segments.last()?;
            match last.arguments {
                syn::PathArguments::None => Some(ident_name(&last.ident)),
                _ => None,
            }
        }
        syn::Type::Group(group) => type_ident(&group.elem),
        syn::Type::Paren(paren) => type_ident(&paren.elem),
        _ => None,
    }
}

/// Expands `#[deser::variant]` on an implementation of an open enum.
pub(crate) fn expand_variant(args: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let args = VariantArgs::parse(args)?;
    let mut item: syn::ItemImpl = match syn::parse2(input) {
        Ok(item) => item,
        Err(err) => {
            return Err(syn::Error::new(
                err.span(),
                "#[deser::variant] can only be placed on implementations of open enums",
            ));
        }
    };
    if let Some(bang) = item.modifiers.polarity {
        return Err(syn::Error::new_spanned(
            bang,
            "negative implementations cannot be variants",
        ));
    }
    let trait_path = match item.trait_ {
        Some((ref path, _)) => path.clone(),
        None => {
            return Err(syn::Error::new_spanned(
                &item.self_ty,
                "#[deser::variant] can only be placed on implementations of open enums \
                 (`impl Trait for Type`)",
            ));
        }
    };
    if !item.generics.params.is_empty() || item.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "generic implementations cannot be variants, implement the trait for every type \
             that is a variant (with a name, for instance `#[deser::variant(rename = \"...\")]`)",
        ));
    }
    if let Some(segment) = trait_path.segments.last()
        && !matches!(segment.arguments, syn::PathArguments::None)
    {
        return Err(syn::Error::new_spanned(
            &segment.arguments,
            "open enums cannot have generic parameters",
        ));
    }
    let method = syn::Ident::new(METHOD, Span::call_site());
    for impl_item in &item.items {
        if let syn::ImplItem::Fn(func) = impl_item
            && func.sig.ident == method
        {
            return Err(syn::Error::new_spanned(
                &func.sig.ident,
                "this method is implemented by #[deser::variant]",
            ));
        }
    }

    let deser = deser_path(args.crate_path.as_ref());
    let self_ty = &item.self_ty;
    let ident = match type_ident(self_ty) {
        Some(ident) => ident,
        None if args.rename.is_some() => String::new(),
        None => {
            return Err(syn::Error::new_spanned(
                self_ty,
                "the variant needs a name as the type has generic arguments or is not a path \
                 (`#[deser::variant(rename = \"...\")]`)",
            ));
        }
    };
    let mut styled = Vec::with_capacity(RenameAll::STYLES.len());
    for style in RenameAll::STYLES {
        styled.push(style.apply_to_variant(&ident));
    }
    let rename = match args.rename {
        Some(ref name) => {
            let tag = tag(&deser, name);
            quote! { #deser::__derive::Some(#tag) }
        }
        None => quote! { #deser::__derive::None },
    };
    let mut aliases = Vec::with_capacity(args.aliases.len());
    for alias in &args.aliases {
        aliases.push(tag(&deser, alias));
    }
    let object = quote! { dyn #trait_path };
    let variant = quote_spanned! { self_ty.span()=>
        #deser::__derive::VariantValue::new(
            <Self as #deser::OpenVariant<#object>>::ENTRY,
            self,
        )
    };

    item.items.push(syn::parse_quote! {
        #[doc(hidden)]
        #[inline]
        fn #method(&self) -> #deser::__derive::VariantValue<'_> {
            #variant
        }
    });
    // errors about the type (not serializable or deserializable) point to
    // it (they are the supertraits of `OpenVariant`)
    Ok(quote! {
        #item

        #[automatically_derived]
        impl #deser::OpenVariant<#object> for #self_ty {
            const ENTRY: &'static #deser::__derive::VariantEntry = &#deser::__derive::VariantEntry {
                styled: &[#(#styled),*],
                rename: #rename,
                aliases: &[#(#aliases),*],
            };

            #[inline]
            fn __private_into_box(self) -> #deser::__derive::Box<#object> {
                #deser::__derive::Box::new(self)
            }
        }
    })
}
