//! Support for enums with data.
//!
//! This is used by the derive but it's generic over the enum so that the
//! generated code stays small.  The different enum representations are
//! implemented by different sinks, all of which deserialize the content of a
//! variant through a [`VariantBuilder`].
//!
//! Tags are recorded (see [`Recording`]) so that they can be any value.
//! Known variants are looked up by string, other tags go to the variant
//! marked with `#[deser(other)]` which can capture the tag.
use std::borrow::Cow;
use std::marker::PhantomData;
use std::mem::take;
use std::ptr::NonNull;

use crate::State;
use crate::de::unknown::{report_unclaimed_key, unknown_field, unknown_field_error};
use crate::de::{Deserialize, OwnedSink, Recording, Sink, SinkHandle};
use crate::error::{Error, ErrorKind, unknown_variant};
use crate::event::{Atom, Event};

/// Builds the value of an enum variant.
pub trait VariantBuilder<'de, E>: Send {
    /// Returns the sink for the variant's fields.
    fn sink(&mut self) -> &mut dyn Sink<'de>;

    /// Receives the tag of the variant.
    ///
    /// This is only invoked for the other variant (with the tag) and the
    /// default variant (with `None` as the tag is missing).
    fn set_tag(&mut self, tag: Option<&Recording>, state: &mut State) -> Result<(), Error> {
        let _ = tag;
        let _ = state;
        Ok(())
    }

    /// Builds the enum value after the sink finished.
    fn build(&mut self) -> Option<E>;
}

/// A boxed variant builder.
pub type BoxedVariant<'de, E> = Box<dyn VariantBuilder<'de, E> + 'de>;

/// A variant that is deserialized as `V` and then converted into `E`.
pub struct Variant<'de, V, E> {
    sink: OwnedSink<'de, V>,
    convert: fn(V) -> E,
}

impl<'de, V: Deserialize<'de> + 'de, E: 'de> Variant<'de, V, E> {
    /// Creates a boxed builder for a variant.
    pub fn boxed(convert: fn(V) -> E) -> BoxedVariant<'de, E> {
        Box::new(Variant {
            sink: OwnedSink::deserialize(),
            convert,
        })
    }
}

impl<'de, V: Deserialize<'de>, E> VariantBuilder<'de, E> for Variant<'de, V, E> {
    fn sink(&mut self) -> &mut dyn Sink<'de> {
        self.sink.borrow_mut()
    }

    fn build(&mut self) -> Option<E> {
        self.sink.take().map(self.convert)
    }
}

/// A variant that ignores its content (used for `#[deser(other)]`).
pub struct IgnoredVariant<'de, E> {
    sink: SinkHandle<'de, 'de>,
    make: fn() -> E,
}

impl<'de, E: 'de> IgnoredVariant<'de, E> {
    /// Creates a boxed builder for a variant which ignores its content.
    pub fn boxed(make: fn() -> E) -> BoxedVariant<'de, E> {
        Box::new(IgnoredVariant {
            sink: SinkHandle::null(),
            make,
        })
    }
}

impl<'de, E> VariantBuilder<'de, E> for IgnoredVariant<'de, E> {
    fn sink(&mut self) -> &mut dyn Sink<'de> {
        &mut self.sink
    }

    fn build(&mut self) -> Option<E> {
        Some((self.make)())
    }
}

/// A variant that captures its tag (used for `#[deser(other)]`).
///
/// The tag is deserialized as `T` and the content as `C`.
pub struct OtherVariant<'de, T, C, E> {
    tag: Option<T>,
    content: OwnedSink<'de, C>,
    convert: fn(T, C) -> E,
}

impl<'de, T, C, E> OtherVariant<'de, T, C, E>
where
    T: Deserialize<'de> + 'de,
    C: Deserialize<'de> + 'de,
    E: 'de,
{
    /// Creates a boxed builder for a variant which captures its tag.
    pub fn boxed(convert: fn(T, C) -> E) -> BoxedVariant<'de, E> {
        Box::new(OtherVariant {
            tag: None,
            content: OwnedSink::deserialize(),
            convert,
        })
    }
}

impl<'de, T, C, E> VariantBuilder<'de, E> for OtherVariant<'de, T, C, E>
where
    T: Deserialize<'de>,
    C: Deserialize<'de>,
{
    fn sink(&mut self) -> &mut dyn Sink<'de> {
        self.content.borrow_mut()
    }

    fn set_tag(&mut self, tag: Option<&Recording>, state: &mut State) -> Result<(), Error> {
        match tag {
            Some(tag) => tag.replay(T::deserialize_into(&mut self.tag), state),
            None => {
                // the tag is missing, the tag field needs to accept this
                self.tag = T::initial_value();
                if self.tag.is_none() {
                    Err(Error::new(ErrorKind::MissingField, "missing tag"))
                } else {
                    Ok(())
                }
            }
        }
    }

    fn build(&mut self) -> Option<E> {
        let tag = self.tag.take()?;
        let content = self.content.take()?;
        Some((self.convert)(tag, content))
    }
}

/// Content that is ignored.
///
/// This accepts any value and is used for variants without content.
pub struct IgnoredContent;

impl<'de> Deserialize<'de> for IgnoredContent {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        struct IgnoredContentSink<'a>(&'a mut Option<IgnoredContent>);

        impl<'a, 'de> Sink<'de> for IgnoredContentSink<'a> {
            fn atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
                Ok(())
            }

            fn map(&mut self, _state: &mut State) -> Result<(), Error> {
                Ok(())
            }

            fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
                Ok(())
            }

            fn __private_key_atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
                Ok(())
            }

            fn __private_value_atom(
                &mut self,
                _atom: Atom,
                _state: &mut State,
            ) -> Result<(), Error> {
                Ok(())
            }

            fn __private_borrowed_key_atom(
                &mut self,
                _atom: Atom<'de>,
                _state: &mut State,
            ) -> Result<(), Error> {
                Ok(())
            }

            fn __private_borrowed_value_atom(
                &mut self,
                _atom: Atom<'de>,
                _state: &mut State,
            ) -> Result<(), Error> {
                Ok(())
            }

            fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
                *self.0 = Some(IgnoredContent);
                Ok(())
            }
        }

        SinkHandle::boxed(IgnoredContentSink(out))
    }
}

/// The tag of a variant.
///
/// Variants are named by strings, integers or booleans.  Non-negative
/// integers are always [`U64`](Self::U64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag<'a> {
    Str(&'a str),
    U64(u64),
    I64(i64),
    Bool(bool),
}

impl<'a> Tag<'a> {
    /// Returns the tag of an atom that is not lexical.
    #[inline]
    fn of_atom(atom: &'a Atom) -> Option<Tag<'a>> {
        match *atom {
            Atom::Str(ref name) => Some(Tag::Str(name)),
            Atom::U64(value) => Some(Tag::U64(value)),
            Atom::I64(value) => Some(match u64::try_from(value) {
                Ok(value) => Tag::U64(value),
                Err(_) => Tag::I64(value),
            }),
            Atom::Bool(value) => Some(Tag::Bool(value)),
            _ => None,
        }
    }

    /// Parses lexical text into a tag that is not a string.
    fn parse_lexical(text: &str) -> Option<Tag<'static>> {
        match text {
            "true" => Some(Tag::Bool(true)),
            "false" => Some(Tag::Bool(false)),
            _ => text
                .parse()
                .map(Tag::U64)
                .ok()
                .or_else(|| text.parse().map(Tag::I64).ok()),
        }
    }
}

/// Looks up the variant for an atom.
///
/// Strings, integers and booleans are tags.  Lexical atoms (text of unknown
/// type) are looked up as strings and then as integers or booleans.
/// Extension values are lowered to their fallback.
#[inline]
pub fn lookup_atom<T>(atom: &Atom, lookup: impl Fn(Tag<'_>) -> Option<T>) -> Option<T> {
    match atom {
        Atom::Lexical(text) => lookup(Tag::Str(text)).or_else(|| lookup(Tag::parse_lexical(text)?)),
        Atom::Ext(ext) => lookup_atom(&ext.fallback(), lookup),
        atom => lookup(Tag::of_atom(atom)?),
    }
}

/// Returns the index of the variant of a unit enum for an atom.
///
/// Extension values are lowered to their fallback.  Atoms that are not the
/// name of a variant are the `other` variant if there is one, otherwise
/// they are an error.  This does everything but the lookup of the names for
/// the unit enums of the derive, it's not inlined so that it exists once.
#[inline(never)]
pub fn unit_variant(
    atom: &Atom<'_>,
    lookup: fn(Tag<'_>) -> Option<usize>,
    names: &[&str],
    expecting: &str,
    other: Option<usize>,
) -> Result<usize, Error> {
    if let Atom::Ext(ext) = atom {
        return match ext.fallback() {
            Atom::Ext(_) => Err(atom.unexpected_error(expecting)),
            fallback => unit_variant(&fallback, lookup, names, expecting, other),
        };
    }
    match lookup_atom(atom, lookup).or(other) {
        Some(index) => Ok(index),
        None => Err(unknown_variant_atom(atom, names, expecting)),
    }
}

/// Returns the name of an atom as tag for errors.
///
/// Returns `None` if the atom cannot be a tag.
fn tag_display<'a>(atom: &'a Atom<'_>) -> Option<Cow<'a, str>> {
    match atom {
        Atom::Lexical(text) => Some(Cow::Borrowed(text)),
        Atom::Ext(ext) => tag_display(&ext.fallback()).map(|x| Cow::Owned(x.into_owned())),
        atom => Some(match Tag::of_atom(atom)? {
            Tag::Str(name) => Cow::Borrowed(name),
            Tag::U64(value) => Cow::Owned(value.to_string()),
            Tag::I64(value) => Cow::Owned(value.to_string()),
            Tag::Bool(value) => Cow::Borrowed(if value { "true" } else { "false" }),
        }),
    }
}

/// Creates the error for an atom that is not the tag of a variant.
///
/// Atoms that cannot be tags (such as floats) are unexpected.
#[cold]
pub fn unknown_variant_atom(atom: &Atom, names: &[&str], expecting: &str) -> Error {
    match tag_display(atom) {
        Some(name) => unknown_variant(Some(&name), expecting, names),
        None => atom.unexpected_error(expecting),
    }
}

/// Looks up a variant by tag.
pub type VariantLookup<'de, E> = fn(Tag<'_>) -> Option<BoxedVariant<'de, E>>;

/// Creates the builder of a special variant.
pub type VariantMaker<'de, E> = fn() -> BoxedVariant<'de, E>;

/// Looks up a unit variant by tag.
pub type UnitLookup<E> = fn(Tag<'_>) -> Option<E>;

/// Creates the builder for the n-th variant of an untagged enum.
pub type CandidateLookup<'de, E> = fn(usize) -> Option<BoxedVariant<'de, E>>;

/// The variants of a tagged enum.
pub struct Variants<'de, E> {
    /// Looks up the known variants by tag.
    pub lookup: VariantLookup<'de, E>,
    /// Creates the variant for unknown tags (`#[deser(other)]`).
    pub other: Option<VariantMaker<'de, E>>,
    /// Creates the variant for missing tags (`#[deser(default)]`).
    pub default: Option<VariantMaker<'de, E>>,
    /// The names of the variants for errors.
    pub names: &'static [&'static str],
}

impl<'de, E> Clone for Variants<'de, E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'de, E> Copy for Variants<'de, E> {}

impl<'de, E> Variants<'de, E> {
    /// Returns the variant for a recorded tag.
    ///
    /// The name is the name of the enum for errors.
    fn resolve(
        &self,
        tag: &Recording,
        name: &str,
        state: &mut State,
    ) -> Result<BoxedVariant<'de, E>, Error> {
        let atom = single_atom(tag);
        if let Some(atom) = atom
            && let Some(variant) = lookup_atom(atom, self.lookup)
        {
            return Ok(variant);
        }
        match self.other {
            Some(other) => {
                let mut variant = other();
                variant.set_tag(Some(tag), state)?;
                Ok(variant)
            }
            None => Err(unknown_variant(
                atom.and_then(tag_display).as_deref(),
                name,
                self.names,
            )),
        }
    }

    /// Returns the variant for a missing tag.
    fn resolve_missing(&self, tag: &str, state: &mut State) -> Result<BoxedVariant<'de, E>, Error> {
        match self.default {
            Some(default) => {
                let mut variant = default();
                variant.set_tag(None, state)?;
                Ok(variant)
            }
            None => Err(Error::new(
                ErrorKind::MissingField,
                format!("missing tag `{}`", tag),
            )),
        }
    }
}

/// Returns the atom of a recording if it's a single atom.
fn single_atom(tag: &Recording) -> Option<&Atom<'static>> {
    let mut events = tag.events();
    match (events.next(), events.next()) {
        (Some(Event::Atom(atom)), None) => Some(atom),
        _ => None,
    }
}

/// Feeds a null to a variant which has no content.
fn feed_null<'de, E>(
    variant: &mut dyn VariantBuilder<'de, E>,
    state: &mut State,
) -> Result<(), Error> {
    let sink = variant.sink();
    sink.atom(Atom::Null, state)?;
    sink.finish(state)
}

/// A sink for externally tagged enums.
///
/// Unit variants are represented as their tag (usually a string), all other
/// variants as maps with a single key (the tag) and the content as value.
/// Variants with content which are represented as a tag receive null as
/// content.
pub struct ExternallyTaggedSink<'a, 'de, E> {
    out: &'a mut Option<E>,
    name: &'static str,
    variants: Variants<'de, E>,
    unit: UnitLookup<E>,
    key: Recording,
    has_key: bool,
    done: bool,
    variant: Option<BoxedVariant<'de, E>>,
}

impl<'a, 'de, E: Send + 'de> ExternallyTaggedSink<'a, 'de, E> {
    /// Creates a sink handle for an externally tagged enum.
    pub fn handle(
        out: &'a mut Option<E>,
        name: &'static str,
        variants: Variants<'de, E>,
        unit: UnitLookup<E>,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::boxed(ExternallyTaggedSink {
            out,
            name,
            variants,
            unit,
            key: Recording::new(),
            has_key: false,
            done: false,
            variant: None,
        })
    }

    fn begin_key(&mut self) -> Result<(), Error> {
        if self.has_key {
            return Err(Error::new(
                ErrorKind::Unexpected,
                format!("expected a map with a single key for {}", self.expecting()),
            ));
        }
        self.has_key = true;
        Ok(())
    }
}

impl<'a, 'de, E: Send + 'de> Sink<'de> for ExternallyTaggedSink<'a, 'de, E> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        if let Atom::Ext(_) = atom {
            // lowered to the fallback
            return self.unexpected_atom(atom, state);
        }
        if let Some(value) = lookup_atom(&atom, self.unit) {
            *self.out = Some(value);
            self.done = true;
            return Ok(());
        }
        let mut variant = lookup_atom(&atom, self.variants.lookup);
        if variant.is_none()
            && let Some(other) = self.variants.other
        {
            let mut tag = Recording::new();
            tag.set_atom(&atom, state);
            let mut other = other();
            other.set_tag(Some(&tag), state)?;
            variant = Some(other);
        }
        let mut variant = match variant {
            Some(variant) => variant,
            None => {
                return Err(unknown_variant_atom(
                    &atom,
                    self.variants.names,
                    &self.expecting(),
                ));
            }
        };
        feed_null(&mut *variant, state)?;
        *self.out = variant.build();
        self.done = true;
        Ok(())
    }

    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.begin_key()?;
        Ok(self.key.recorder())
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.begin_key()?;
        self.key.set_atom(&atom, state);
        Ok(())
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.__private_key_atom(atom, state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        let variant = self.variants.resolve(&self.key, self.name, state)?;
        Ok(SinkHandle::to(self.variant.insert(variant).sink()))
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        if self.done {
            // unit variant from a tag
            return Ok(());
        }
        let variant = self.variant.as_mut().ok_or_else(|| {
            Error::new(
                ErrorKind::Unexpected,
                format!("expected a map with a single key for {}", self.name),
            )
        })?;
        *self.out = variant.build();
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.name)
    }
}

/// The key of the tag or the content of a tagged enum.
///
/// Besides its name the key can be given by one of its aliases.
#[derive(Debug, Clone, Copy)]
pub struct EnumKey {
    /// The name of the key.
    pub name: &'static str,
    /// Other names of the key which are accepted when deserializing.
    pub aliases: &'static [&'static str],
}

impl EnumKey {
    /// Checks if a key is this key.
    #[inline]
    fn matches(&self, key: &str) -> bool {
        key == self.name || self.aliases.contains(&key)
    }

    /// Checks if a recorded key is this key.
    #[inline]
    fn matches_recorded(&self, key: &Recording) -> bool {
        key.as_str().is_some_and(|key| self.matches(key))
    }
}

/// Creates the error for a key that is given more than once.
fn duplicate_key(what: &str, key: &str) -> Error {
    Error::new(
        ErrorKind::Unexpected,
        format!("duplicate {} `{}`", what, key),
    )
}

/// A sink for adjacently tagged enums.
///
/// The variant name is stored in the tag field and the content in the content
/// field.  If the content comes before the tag it's recorded and replayed
/// once the tag is known.
pub struct AdjacentlyTaggedSink<'a, 'de, E> {
    out: &'a mut Option<E>,
    tag: EnumKey,
    content: EnumKey,
    name: &'static str,
    variants: Variants<'de, E>,
    key: Recording,
    tag_value: Option<Recording>,
    recorded_content: Option<Recording>,
    has_content: bool,
    deny_unknown_fields: bool,
    variant: Option<BoxedVariant<'de, E>>,
}

impl<'a, 'de, E: Send + 'de> AdjacentlyTaggedSink<'a, 'de, E> {
    /// Creates a sink handle for an adjacently tagged enum.
    ///
    /// Keys other than the tag and the content are unknown fields, they are
    /// rejected if `deny_unknown_fields` is set.
    pub fn handle(
        out: &'a mut Option<E>,
        tag: EnumKey,
        content: EnumKey,
        name: &'static str,
        variants: Variants<'de, E>,
        deny_unknown_fields: bool,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::boxed(AdjacentlyTaggedSink {
            out,
            tag,
            content,
            name,
            variants,
            key: Recording::new(),
            tag_value: None,
            recorded_content: None,
            has_content: false,
            deny_unknown_fields,
            variant: None,
        })
    }

    fn start_variant(
        &mut self,
        mut variant: BoxedVariant<'de, E>,
        state: &mut State,
    ) -> Result<(), Error> {
        if let Some(content) = self.recorded_content.take() {
            content.replay(SinkHandle::to(variant.sink()), state)?;
        }
        self.variant = Some(variant);
        Ok(())
    }

    fn ensure_variant(&mut self, state: &mut State) -> Result<(), Error> {
        if self.variant.is_some() {
            return Ok(());
        }
        let variant = match self.tag_value {
            Some(ref tag) => self.variants.resolve(tag, self.name, state)?,
            None => return Ok(()),
        };
        self.start_variant(variant, state)
    }
}

impl<'a, 'de, E: Send + 'de> Sink<'de> for AdjacentlyTaggedSink<'a, 'de, E> {
    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.ensure_variant(state)?;
        Ok(self.key.recorder())
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        let key = take(&mut self.key);
        if self.tag.matches_recorded(&key) {
            if self.tag_value.is_some() {
                return Err(duplicate_key("field", self.tag.name));
            }
            Ok(self.tag_value.insert(Recording::new()).recorder())
        } else if self.content.matches_recorded(&key) {
            if self.has_content {
                return Err(duplicate_key("field", self.content.name));
            }
            self.has_content = true;
            Ok(match self.variant {
                Some(ref mut variant) => SinkHandle::to(variant.sink()),
                None => self.recorded_content.insert(Recording::new()).recorder(),
            })
        } else {
            unknown_field(
                key.as_str().unwrap_or("?"),
                key.offset(),
                &[self.tag.name, self.content.name],
                self.deny_unknown_fields,
                state,
            )?;
            Ok(SinkHandle::null())
        }
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.ensure_variant(state)?;
        if self.variant.is_none() {
            let variant = self.variants.resolve_missing(self.tag.name, state)?;
            self.start_variant(variant, state)?;
        }
        let variant = self.variant.as_mut().unwrap();
        if !self.has_content {
            // variants without content (unit variants) are fed a null
            feed_null(&mut **variant, state)?;
        }
        *self.out = variant.build();
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.name)
    }
}

/// Creates a sink handle for an untagged enum.
///
/// The value is recorded and replayed into the variants in order until one
/// of them accepts it.
pub fn untagged_handle<'a, 'de, E: Send>(
    out: &'a mut Option<E>,
    name: &'static str,
    candidates: CandidateLookup<'de, E>,
) -> SinkHandle<'a, 'de> {
    Recording::capture(move |recording, state| {
        for index in 0.. {
            let mut variant = match candidates(index) {
                Some(variant) => variant,
                None => break,
            };
            if recording
                .replay(SinkHandle::to(variant.sink()), state)
                .is_ok()
                && let Some(value) = variant.build()
            {
                *out = Some(value);
                return Ok(());
            }
        }
        Err(Error::new(
            ErrorKind::Unexpected,
            format!("data did not match any variant of {}", name),
        ))
    })
}

/// A sink for internally tagged enums.
///
/// Until the tag is known, all key value pairs are recorded.  Once the tag
/// was seen, the recorded pairs are replayed into the variant and all further
/// pairs are forwarded to it directly.
pub struct InternallyTaggedSink<'a, 'de, E> {
    out: &'a mut Option<E>,
    tag: EnumKey,
    name: &'static str,
    variants: Variants<'de, E>,
    key: Recording,
    pending: Vec<(Recording, Recording)>,
    tag_value: Option<Recording>,
    variant: Option<BoxedVariant<'de, E>>,
    // if the key of the variant was recorded to look for the tag
    variant_key: bool,
    // if the enum is flattened into a struct
    flattened: bool,
    // the errors for keys that were taken but the variant did not use
    unclaimed: Vec<Error>,
}

impl<'a, 'de, E: Send + 'de> InternallyTaggedSink<'a, 'de, E> {
    /// Creates a sink handle for an internally tagged enum.
    pub fn handle(
        out: &'a mut Option<E>,
        tag: EnumKey,
        name: &'static str,
        variants: Variants<'de, E>,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::boxed(InternallyTaggedSink {
            out,
            tag,
            name,
            variants,
            key: Recording::new(),
            pending: Vec::new(),
            tag_value: None,
            variant: None,
            variant_key: false,
            flattened: false,
            unclaimed: Vec::new(),
        })
    }

    /// Starts a variant and replays the pairs recorded so far into it.
    fn start_variant(
        &mut self,
        mut variant: BoxedVariant<'de, E>,
        state: &mut State,
    ) -> Result<(), Error> {
        variant.sink().map(state)?;
        for (key, value) in take(&mut self.pending) {
            if !self.flattened {
                key.replay(variant.sink().next_key(state)?, state)?;
                value.replay(variant.sink().next_value(state)?, state)?;
                continue;
            }
            // the keys of flattened enums are offered to the variant like
            // the keys after the tag.  Those it does not take are unknown
            // keys of the struct the enum is flattened into.
            let name = key.as_str().unwrap_or_default();
            match variant.sink().value_for_key(name, state)? {
                Some(sink) => value.replay(sink, state)?,
                None => {
                    let err = value.attach_context(unknown_field_error(name, None), state);
                    self.unclaimed.push(err);
                }
            }
        }
        self.variant = Some(variant);
        Ok(())
    }

    /// Creates the variant once the tag is known.
    fn ensure_variant(&mut self, state: &mut State) -> Result<(), Error> {
        if self.variant.is_some() {
            return Ok(());
        }
        let variant = match self.tag_value {
            Some(ref tag) => self.variants.resolve(tag, self.name, state)?,
            None => return Ok(()),
        };
        self.start_variant(variant, state)
    }
}

impl<'a, 'de, E: Send + 'de> Sink<'de> for InternallyTaggedSink<'a, 'de, E> {
    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    /// Receives the next key.
    ///
    /// Once the variant is known, the keys still need to be checked for the
    /// tag (which is given more than once then), so they are recorded and
    /// replayed into the variant.  Keys that are atoms (the common case) are
    /// checked without recording them in
    /// [`__private_key_atom`](Sink::__private_key_atom).
    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.ensure_variant(state)?;
        self.variant_key = self.variant.is_some();
        Ok(self.key.recorder())
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.ensure_variant(state)?;
        match self.variant {
            Some(ref mut variant) => {
                if atom.as_str().is_some_and(|key| self.tag.matches(key)) {
                    return Err(duplicate_key("tag", self.tag.name));
                }
                variant.sink().__private_key_atom(atom, state)
            }
            None => {
                self.key.set_atom(&atom, state);
                Ok(())
            }
        }
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.ensure_variant(state)?;
        match self.variant {
            Some(ref mut variant) => {
                if atom.as_str().is_some_and(|key| self.tag.matches(key)) {
                    return Err(duplicate_key("tag", self.tag.name));
                }
                variant.sink().__private_borrowed_key_atom(atom, state)
            }
            None => {
                self.key.set_atom(&atom, state);
                Ok(())
            }
        }
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        if let Some(variant) = &mut self.variant {
            if take(&mut self.variant_key) {
                let key = take(&mut self.key);
                if self.tag.matches_recorded(&key) {
                    return Err(duplicate_key("tag", self.tag.name));
                }
                key.replay(variant.sink().next_key(state)?, state)?;
            }
            return variant.sink().next_value(state);
        }
        let key = take(&mut self.key);
        if self.tag.matches_recorded(&key) {
            if self.tag_value.is_some() {
                return Err(duplicate_key("tag", self.tag.name));
            }
            return Ok(self.tag_value.insert(Recording::new()).recorder());
        }
        self.pending.push((key, Recording::new()));
        Ok(self.pending.last_mut().unwrap().1.recorder())
    }

    /// Takes the keys of a struct the enum is flattened into.
    ///
    /// Once the tag is known, keys are offered to the variant.  Until then
    /// all keys offered are recorded (they are the keys that no field of the
    /// struct took) and offered to the variant once it's known.  The keys
    /// it does not take are reported to the struct when the enum finishes
    /// as they are unknown keys of the struct (see
    /// [`UnknownFields`](crate::de::UnknownFields)).
    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.flattened = true;
        if self.tag.matches(key) {
            if self.tag_value.is_some() {
                return Err(duplicate_key("tag", self.tag.name));
            }
            return Ok(Some(self.tag_value.insert(Recording::new()).recorder()));
        }
        self.ensure_variant(state)?;
        if let Some(variant) = &mut self.variant {
            return variant.sink().value_for_key(key, state);
        }
        let mut recorded_key = Recording::new();
        recorded_key.set_atom(&Atom::Str(Cow::Borrowed(key)), state);
        self.pending.push((recorded_key, Recording::new()));
        Ok(Some(self.pending.last_mut().unwrap().1.recorder()))
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.ensure_variant(state)?;
        if self.variant.is_none() {
            let variant = self.variants.resolve_missing(self.tag.name, state)?;
            self.start_variant(variant, state)?;
        }
        let variant = self.variant.as_mut().unwrap();
        variant.sink().finish(state)?;
        *self.out = variant.build();
        for err in take(&mut self.unclaimed) {
            report_unclaimed_key(err, state);
        }
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.name)
    }
}

/// A function that sets a value from an atom.
pub type AtomSetter<T> = for<'x> fn(&mut T, Atom<'x>, &mut State) -> Result<(), Error>;

/// [`AtomSetter`] with the type of the value erased.
type ErasedAtomSetter = for<'x> fn(NonNull<()>, Atom<'x>, &mut State) -> Result<(), Error>;

/// A sink that sets a value from an atom with a function.
///
/// Unit enums of the derive use this for their sinks (to deserialize and
/// to update them), all they have to generate is the function.  The sink
/// exists once for all types.
struct AtomSink<'a> {
    target: NonNull<()>,
    set: ErasedAtomSetter,
    name: &'static str,
    _marker: PhantomData<&'a mut ()>,
}

// SAFETY: the sink only allows the setter to mutate the target (a
// `&'a mut T` with `T: Send`, see `atom_sink`).
unsafe impl Send for AtomSink<'_> {}

/// Creates a sink that sets the target from an atom with the setter.
///
/// Other data is rejected with `name` as expected type.
pub fn atom_sink<'a, 'de, T: Send + 'a>(
    target: &'a mut T,
    set: AtomSetter<T>,
    name: &'static str,
) -> SinkHandle<'a, 'de> {
    // SAFETY: `&mut T` and `NonNull<()>` are ABI compatible (both are
    // pointers to sized types), and the setter is only ever called with
    // the target which is a valid `&'a mut T` for the lifetime of the sink.
    let set = unsafe { std::mem::transmute::<AtomSetter<T>, ErasedAtomSetter>(set) };
    SinkHandle::boxed(AtomSink {
        target: NonNull::from(target).cast(),
        set,
        name,
        _marker: PhantomData,
    })
}

impl<'de> Sink<'de> for AtomSink<'_> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        (self.set)(self.target, atom, state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.name)
    }
}
