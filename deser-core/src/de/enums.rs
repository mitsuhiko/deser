//! Support for enums with data.
//!
//! This is used by the derive but it's generic over the enum so that the
//! generated code stays small.  The different enum representations are
//! implemented by different sinks, all of which deserialize the content of a
//! variant through a [`VariantBuilder`].
//!
//! Tags are recorded (see [`RecordBuf`]) so that they can be any value.
//! Known variants are looked up by string, other tags go to the variant
//! marked with `#[deser(other)]` which can capture the tag.
use alloc::borrow::Cow;
use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::marker::PhantomData;
use core::mem::take;
use core::ptr::NonNull;

use crate::State;
use crate::Text;
use crate::arena::ArenaBox;
use crate::de::recording::{Capture, RecordBuf};
use crate::de::unknown::{report_unclaimed_key, unknown_field, unknown_field_error};
use crate::de::{Deserialize, OwnedSink, Sink, SinkHandle, default_atom};
use crate::error::{Error, ErrorKind, unknown_variant};
use crate::event::Atom;
use crate::extensions::EventData;

/// Builds the value of an enum variant.
pub trait VariantBuilder<'de, E>: Send {
    /// Returns the sink for the variant's fields.
    fn sink(&mut self) -> &mut dyn Sink<'de>;

    /// Receives the tag of the variant.
    ///
    /// This is only invoked for the other variant (with the tag) and the
    /// default variant (with `None` as the tag is missing).
    fn set_tag(&mut self, tag: Option<&RecordBuf<'de>>, state: &mut State) -> Result<(), Error> {
        let _ = tag;
        let _ = state;
        Ok(())
    }

    /// Builds the enum value after the sink finished.
    fn build(&mut self) -> Option<E>;

    /// Returns `true` if the content of the variant is a unit struct.
    ///
    /// Such variants of internally tagged enums are the tag alone, they
    /// receive null if there are no other keys (see
    /// [`InternallyTaggedSink`]).
    fn unit_struct(&self) -> bool {
        false
    }
}

/// A variant builder in the arena of the state.
///
/// `'a` is the lifetime of the slot of the enum.  The enum (and with it
/// the types of its fields) outlives it, which allows enums to borrow.
pub struct ArenaVariant<'a, 'de, E>(ArenaBox<dyn VariantBuilder<'de, E> + 'a>);

impl<'a, 'de, E> ArenaVariant<'a, 'de, E> {
    /// Moves a builder into the arena of the state.
    #[inline(always)]
    pub fn new<B: VariantBuilder<'de, E> + 'a>(builder: B, state: &mut State) -> Self {
        let ptr = ArenaBox::into_raw(ArenaBox::new(builder, &mut state.arena));
        // SAFETY: the pointer comes from the box
        ArenaVariant(unsafe {
            ArenaBox::from_raw(ptr.as_ptr() as *mut (dyn VariantBuilder<'de, E> + 'a))
        })
    }
}

impl<'a, 'de, E> core::ops::Deref for ArenaVariant<'a, 'de, E> {
    type Target = dyn VariantBuilder<'de, E> + 'a;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.0.get()
    }
}

impl<'a, 'de, E> core::ops::DerefMut for ArenaVariant<'a, 'de, E> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.get_mut()
    }
}

/// A variant whose content is deserialized as a value of type `V` which is
/// then converted into `E`.
pub struct ValueVariant<'de, V, E> {
    sink: OwnedSink<'de, V>,
    convert: fn(V) -> E,
}

impl<'de, V: Deserialize<'de>, E> ValueVariant<'de, V, E> {
    /// Creates a builder for a variant in the arena of the state.
    pub fn arena<'a>(convert: fn(V) -> E, state: &mut State) -> ArenaVariant<'a, 'de, E>
    where
        'de: 'a,
        V: 'a,
        E: 'a,
    {
        ArenaVariant::new(
            ValueVariant {
                sink: OwnedSink::deserialize(state),
                convert,
            },
            state,
        )
    }
}

impl<'de, V: Deserialize<'de>, E> VariantBuilder<'de, E> for ValueVariant<'de, V, E> {
    fn sink(&mut self) -> &mut dyn Sink<'de> {
        self.sink.get_mut()
    }

    fn build(&mut self) -> Option<E> {
        self.sink.take().map(self.convert)
    }

    fn unit_struct(&self) -> bool {
        crate::ser::is_unit_struct(V::describe_type)
    }
}

/// A variant that ignores its content (used for `#[deser(other)]`).
pub struct IgnoredVariant<'de, E> {
    sink: SinkHandle<'de, 'de>,
    make: fn() -> E,
}

impl<'de, E> IgnoredVariant<'de, E> {
    /// Creates a builder for a variant which ignores its content, in the
    /// arena of the state.
    pub fn arena<'a>(make: fn() -> E, state: &mut State) -> ArenaVariant<'a, 'de, E>
    where
        'de: 'a,
        E: 'a,
    {
        ArenaVariant::new(
            IgnoredVariant {
                sink: SinkHandle::null(),
                make,
            },
            state,
        )
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
    T: Deserialize<'de>,
    C: Deserialize<'de>,
{
    /// Creates a builder for a variant which captures its tag, in the arena
    /// of the state.
    pub fn arena<'a>(convert: fn(T, C) -> E, state: &mut State) -> ArenaVariant<'a, 'de, E>
    where
        'de: 'a,
        T: 'a,
        C: 'a,
        E: 'a,
    {
        ArenaVariant::new(
            OtherVariant {
                tag: None,
                content: OwnedSink::deserialize(state),
                convert,
            },
            state,
        )
    }
}

impl<'de, T, C, E> VariantBuilder<'de, E> for OtherVariant<'de, T, C, E>
where
    T: Deserialize<'de>,
    C: Deserialize<'de>,
{
    fn sink(&mut self) -> &mut dyn Sink<'de> {
        self.content.get_mut()
    }

    fn set_tag(&mut self, tag: Option<&RecordBuf<'de>>, state: &mut State) -> Result<(), Error> {
        match tag {
            Some(tag) => tag.replay(T::deserialize_into(&mut self.tag, state), state),
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
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
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

        SinkHandle::arena(IgnoredContentSink(out), state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("any value")
    }
}

/// The tag of a variant.
///
/// Variants are named by strings, integers or booleans.  Non-negative
/// integers are always [`U64`](Self::U64).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
/// Implicit atoms are looked up as their value and then as their text.
/// Extension values are lowered to their fallback.
#[inline]
pub(crate) fn lookup_atom<T>(
    atom: &Atom,
    mut lookup: impl FnMut(Tag<'_>) -> Option<T>,
) -> Option<T> {
    match atom {
        Atom::Lexical(text) => match lookup(Tag::Str(text)) {
            Some(rv) => Some(rv),
            None => lookup(Tag::parse_lexical(text)?),
        },
        Atom::Implicit(value) => match Tag::of_atom(&value.value().to_atom()).and_then(&mut lookup)
        {
            Some(rv) => Some(rv),
            None => lookup(Tag::Str(value.text())),
        },
        Atom::Ext(ext) => lookup_atom(&ext.fallback(), lookup),
        atom => lookup(Tag::of_atom(atom)?),
    }
}

/// Returns the index of the variant of a unit enum for an atom.
///
/// Extension values are lowered to their fallback.  Atoms that are not the
/// name of a variant are the `other` variant if there is one, otherwise
/// they are an error.  This does everything but the lookup of the names for
/// the unit enums of the derive.  Names given as strings (the common case)
/// are looked up inline, everything else in a function that exists once.
#[inline]
pub(crate) fn unit_variant(
    atom: &Atom<'_>,
    lookup: fn(Tag<'_>) -> Option<usize>,
    names: &[&str],
    expecting: &str,
    other: Option<usize>,
) -> Result<usize, Error> {
    if let Atom::Str(name) = atom
        && let Some(index) = lookup(Tag::Str(name))
    {
        return Ok(index);
    }
    unit_variant_slow(atom, lookup, names, expecting, other)
}

#[inline(never)]
fn unit_variant_slow(
    atom: &Atom<'_>,
    lookup: fn(Tag<'_>) -> Option<usize>,
    names: &[&str],
    expecting: &str,
    other: Option<usize>,
) -> Result<usize, Error> {
    if let Atom::Ext(ext) = atom {
        return match ext.fallback() {
            Atom::Ext(_) => Err(atom.unexpected_error(expecting)),
            fallback => unit_variant_slow(&fallback, lookup, names, expecting, other),
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
        Atom::Implicit(value) => Some(Cow::Borrowed(value.text())),
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
pub(crate) fn unknown_variant_atom(atom: &Atom, names: &[&str], expecting: &str) -> Error {
    match tag_display(atom) {
        Some(name) => unknown_variant(Some(&name), expecting, names),
        None => atom.unexpected_error(expecting),
    }
}

/// Looks up a variant by tag.
pub(crate) type VariantLookup<'a, 'de, E> =
    fn(Tag<'_>, &mut State) -> Option<ArenaVariant<'a, 'de, E>>;

/// Creates the builder of a special variant.
pub type VariantMaker<'a, 'de, E> = fn(&mut State) -> ArenaVariant<'a, 'de, E>;

/// Looks up a unit variant by tag.
pub(crate) type UnitLookup<E> = fn(Tag<'_>) -> Option<E>;

/// The variants of a tagged enum.
pub struct Variants<'a, 'de, E> {
    /// Looks up the known variants by tag.
    pub lookup: VariantLookup<'a, 'de, E>,
    /// Creates the variant for unknown tags (`#[deser(other)]`).
    pub other: Option<VariantMaker<'a, 'de, E>>,
    /// Creates the variant for missing tags (`#[deser(default)]`).
    pub default: Option<VariantMaker<'a, 'de, E>>,
    /// The names of the variants for errors.
    pub names: VariantNames,
}

/// The names of the variants of a tagged enum, for errors.
#[derive(Clone, Copy)]
pub enum VariantNames {
    /// The names are known at compile time (enums of the derive).
    Static(&'static [&'static str]),
    /// The names depend on the state (open enums, whose variants are
    /// registered in the context).
    Dynamic(for<'s> fn(&'s State) -> Vec<&'s str>),
}

impl VariantNames {
    /// Returns the names.
    fn get<'s>(&self, state: &'s State) -> Cow<'s, [&'s str]> {
        match *self {
            VariantNames::Static(names) => Cow::Borrowed(names),
            VariantNames::Dynamic(names) => Cow::Owned(names(state)),
        }
    }
}

impl<'a, 'de, E> Clone for Variants<'a, 'de, E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, 'de, E> Copy for Variants<'a, 'de, E> {}

impl<'a, 'de, E> Variants<'a, 'de, E> {
    /// Returns the variant for a recorded tag.
    ///
    /// The name is the name of the enum for errors.
    fn resolve(
        &self,
        tag: &RecordBuf<'de>,
        name: &str,
        state: &mut State,
    ) -> Result<ArenaVariant<'a, 'de, E>, Error> {
        let atom = tag.single_atom();
        if let Some(atom) = atom
            && let Some(variant) = lookup_atom(atom, |tag| (self.lookup)(tag, state))
        {
            return Ok(variant);
        }
        match self.other {
            Some(other) => {
                let mut variant = other(state);
                variant.set_tag(Some(tag), state)?;
                Ok(variant)
            }
            None => Err(unknown_variant(
                atom.and_then(tag_display).as_deref(),
                name,
                &self.names.get(state),
            )),
        }
    }

    /// Returns the variant for a missing tag.
    fn resolve_missing(
        &self,
        tag: &str,
        state: &mut State,
    ) -> Result<ArenaVariant<'a, 'de, E>, Error> {
        match self.default {
            Some(default) => {
                let mut variant = default(state);
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
    variants: Variants<'a, 'de, E>,
    unit: UnitLookup<E>,
    key: RecordBuf<'de>,
    has_key: bool,
    done: bool,
    variant: Option<ArenaVariant<'a, 'de, E>>,
}

impl<'a, 'de, E: Send> ExternallyTaggedSink<'a, 'de, E> {
    /// Creates a sink handle for an externally tagged enum.
    pub fn handle(
        out: &'a mut Option<E>,
        name: &'static str,
        variants: Variants<'a, 'de, E>,
        unit: UnitLookup<E>,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::arena(
            ExternallyTaggedSink {
                out,
                name,
                variants,
                unit,
                key: RecordBuf::new(),
                has_key: false,
                done: false,
                variant: None,
            },
            state,
        )
    }

    fn begin_key(&mut self) -> Result<(), Error> {
        if self.has_key {
            return Err(Error::new(
                ErrorKind::InvalidType,
                format!("expected a map with a single key for {}", self.expecting()),
            ));
        }
        self.has_key = true;
        Ok(())
    }
}

impl<'a, 'de, E: Send> Sink<'de> for ExternallyTaggedSink<'a, 'de, E> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        if let Atom::Ext(_) = atom {
            // lowered to the fallback
            return default_atom(self, atom, state);
        }
        if let Some(value) = lookup_atom(&atom, self.unit) {
            *self.out = Some(value);
            self.done = true;
            return Ok(());
        }
        let lookup = self.variants.lookup;
        let mut variant = lookup_atom(&atom, |tag| lookup(tag, state));
        if variant.is_none()
            && let Some(other) = self.variants.other
        {
            let mut tag = RecordBuf::new();
            tag.set_atom(&atom, state);
            let mut other = other(state);
            other.set_tag(Some(&tag), state)?;
            variant = Some(other);
        }
        let mut variant = match variant {
            Some(variant) => variant,
            None => {
                return Err(unknown_variant_atom(
                    &atom,
                    &self.variants.names.get(state),
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

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.begin_key()?;
        Ok(self.key.recorder(state))
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
                ErrorKind::InvalidType,
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
    fn matches_recorded(&self, key: &RecordBuf<'_>) -> bool {
        key.as_str().is_some_and(|key| self.matches(key))
    }
}

/// Creates the error for a key that is given more than once.
fn duplicate_key(what: &str, key: &str) -> Error {
    Error::new(
        ErrorKind::DuplicateKey,
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
    variants: Variants<'a, 'de, E>,
    key: RecordBuf<'de>,
    tag_value: Option<RecordBuf<'de>>,
    recorded_content: Option<RecordBuf<'de>>,
    has_content: bool,
    deny_unknown_fields: bool,
    variant: Option<ArenaVariant<'a, 'de, E>>,
}

impl<'a, 'de, E: Send> AdjacentlyTaggedSink<'a, 'de, E> {
    /// Creates a sink handle for an adjacently tagged enum.
    ///
    /// Keys other than the tag and the content are unknown fields, they are
    /// rejected if `deny_unknown_fields` is set.
    pub fn handle(
        out: &'a mut Option<E>,
        tag: EnumKey,
        content: EnumKey,
        name: &'static str,
        variants: Variants<'a, 'de, E>,
        deny_unknown_fields: bool,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::arena(
            AdjacentlyTaggedSink {
                out,
                tag,
                content,
                name,
                variants,
                key: RecordBuf::new(),
                tag_value: None,
                recorded_content: None,
                has_content: false,
                deny_unknown_fields,
                variant: None,
            },
            state,
        )
    }

    fn start_variant(
        &mut self,
        mut variant: ArenaVariant<'a, 'de, E>,
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

impl<'a, 'de, E: Send> Sink<'de> for AdjacentlyTaggedSink<'a, 'de, E> {
    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.ensure_variant(state)?;
        Ok(self.key.recorder(state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        let key = take(&mut self.key);
        if self.tag.matches_recorded(&key) {
            if self.tag_value.is_some() {
                return Err(duplicate_key("field", self.tag.name));
            }
            Ok(self.tag_value.insert(RecordBuf::new()).recorder(state))
        } else if self.content.matches_recorded(&key) {
            if self.has_content {
                return Err(duplicate_key("field", self.content.name));
            }
            self.has_content = true;
            Ok(match self.variant {
                Some(ref mut variant) => SinkHandle::to(variant.sink()),
                None => self
                    .recorded_content
                    .insert(RecordBuf::new())
                    .recorder(state),
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

/// Tries the variants of an untagged enum.
///
/// The derive generates this function: it passes the variant with the
/// given index to [`UntaggedTry::variant`] and returns `false` if there is
/// no such variant.
pub(crate) type UntaggedVariants<'de, E> = for<'t> fn(usize, &mut UntaggedTry<'t, 'de, E>) -> bool;

/// Creates a sink handle for an untagged enum.
///
/// The value is delivered to the variants in order until one of them
/// accepts it.  Values other than atoms are recorded and replayed for every
/// variant, the recording borrows from the data.
pub fn untagged_handle<'a, 'de, E: Send>(
    out: &'a mut Option<E>,
    name: &'static str,
    variants: UntaggedVariants<'de, E>,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    untagged_handle_with(out, name, variants, no_matching_variant, state)
}

/// Creates the error for a value that no variant of an untagged enum
/// accepted, from the name of the enum.
pub(crate) type NoMatch = fn(&str, &State) -> Error;

/// Creates a sink handle for an untagged enum with the error for values
/// that no variant accepts (see [`untagged_handle`]).
pub(crate) fn untagged_handle_with<'a, 'de, E: Send>(
    out: &'a mut Option<E>,
    name: &'static str,
    variants: UntaggedVariants<'de, E>,
    no_match: NoMatch,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    RecordBuf::capture_with(
        UntaggedCapture {
            out,
            name,
            variants,
            no_match,
        },
        state,
    )
}

/// Deserializes an atom into an untagged enum.
///
/// This is what the sink of [`untagged_handle`] does with an atom, without
/// creating the sink.
pub fn untagged_atom<'de, E>(
    out: &mut Option<E>,
    name: &'static str,
    variants: UntaggedVariants<'de, E>,
    atom: Atom,
    state: &mut State,
) -> Result<(), Error> {
    *out = Some(
        try_untagged_atom(variants, UntaggedInput::Atom(atom), state)
            .ok_or_else(|| no_matching_variant(name, state))?,
    );
    Ok(())
}

/// Deserializes a borrowed atom into an untagged enum.
///
/// See [`untagged_atom`].
pub fn untagged_borrowed_atom<'de, E>(
    out: &mut Option<E>,
    name: &'static str,
    variants: UntaggedVariants<'de, E>,
    atom: Atom<'de>,
    state: &mut State,
) -> Result<(), Error> {
    *out = Some(
        try_untagged_atom(variants, UntaggedInput::Borrowed(atom), state)
            .ok_or_else(|| no_matching_variant(name, state))?,
    );
    Ok(())
}

/// The value the variants of an untagged enum are tried with.
enum UntaggedInput<'t, 'de> {
    Atom(Atom<'t>),
    Borrowed(Atom<'de>),
    Recorded(&'t RecordBuf<'de>),
}

/// Tries a variant of an untagged enum (see `UntaggedVariants`).
pub struct UntaggedTry<'t, 'de, E> {
    input: UntaggedInput<'t, 'de>,
    // the data of the event of an atom
    data: EventData,
    state: &'t mut State,
    value: Option<E>,
}

impl<'t, 'de, E> UntaggedTry<'t, 'de, E> {
    /// Returns the state.
    #[cfg(feature = "open-enums")]
    #[inline]
    pub(crate) fn state(&self) -> &State {
        self.state
    }

    /// Tries a variant whose content is deserialized as `V`.
    ///
    /// If the variant accepts the value, it's converted with `convert`.
    pub fn variant<V: Deserialize<'de>>(&mut self, convert: fn(V) -> E) {
        let mut slot = None;
        let state = &mut *self.state;
        // sinks can take event data (like CBOR tags), every variant gets
        // the data of the event
        if !self.data.is_empty() {
            state.extensions_mut().restore_event_data(&self.data);
        }
        let rv = match self.input {
            UntaggedInput::Atom(ref atom) => {
                V::__private_atom_into(&mut slot, atom.as_borrowed(), state)
            }
            UntaggedInput::Borrowed(ref atom) => {
                V::__private_borrowed_atom_into(&mut slot, atom.clone(), state)
            }
            UntaggedInput::Recorded(buffer) => {
                buffer.replay(V::deserialize_into(&mut slot, state), state)
            }
        };
        if rv.is_ok() {
            self.value = slot.map(convert);
        }
    }
}

/// Tries the variants of an untagged enum until one accepts the value.
///
/// Returns `None` if no variant does.
fn try_untagged<'de, E>(
    variants: UntaggedVariants<'de, E>,
    input: UntaggedInput<'_, 'de>,
    data: EventData,
    state: &mut State,
) -> Option<E> {
    // only whether a variant accepts the value matters
    state.discard_errors(|state| {
        let mut attempt = UntaggedTry {
            input,
            data,
            state,
            value: None,
        };
        let mut index = 0;
        while attempt.value.is_none() && variants(index, &mut attempt) {
            index += 1;
        }
        attempt.value
    })
}

/// Tries the variants of an untagged enum with an atom.
///
/// Atoms are delivered to the variants directly, like the driver delivers
/// a single atom.
fn try_untagged_atom<'de, E>(
    variants: UntaggedVariants<'de, E>,
    input: UntaggedInput<'_, 'de>,
    state: &mut State,
) -> Option<E> {
    // sinks can take event data (like CBOR tags), every variant gets the
    // data of the event
    let data = state.extensions().capture_event_data();
    try_untagged(variants, input, data, state)
}

/// Creates the error for a value that no variant of an untagged enum
/// accepted.
#[cold]
pub(crate) fn no_matching_variant(name: &str, _state: &State) -> Error {
    Error::new(
        ErrorKind::UnknownVariant,
        format!("data did not match any variant of {}", name),
    )
}

/// The sink of an untagged enum passes the value on to this.
struct UntaggedCapture<'a, 'de, E> {
    out: &'a mut Option<E>,
    name: &'static str,
    variants: UntaggedVariants<'de, E>,
    no_match: NoMatch,
}

impl<E> UntaggedCapture<'_, '_, E> {
    /// Stores the value of the variant that accepted it.
    fn set(&mut self, value: Option<E>, state: &State) -> Result<(), Error> {
        match value {
            Some(value) => {
                *self.out = Some(value);
                Ok(())
            }
            None => Err((self.no_match)(self.name, state)),
        }
    }
}

impl<'a, 'de, E: Send> Capture<'de, RecordBuf<'de>> for UntaggedCapture<'a, 'de, E> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.name)
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let value = try_untagged_atom(self.variants, UntaggedInput::Atom(atom), state);
        self.set(value, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        let value = try_untagged_atom(self.variants, UntaggedInput::Borrowed(atom), state);
        self.set(value, state)
    }

    fn recorded(&mut self, buffer: RecordBuf<'de>, state: &mut State) -> Result<(), Error> {
        // replaying restores the event data of the recorded events
        let input = UntaggedInput::Recorded(&buffer);
        let value = try_untagged(self.variants, input, EventData::new(), state);
        self.set(value, state)
    }
}

/// Creates a sink handle for a tagged enum with untagged variants.
///
/// The value is delivered to the sink of the tagged representation which
/// `tagged` creates.  If that fails, it's delivered to the untagged
/// variants in order until one of them accepts it.  If none does, the
/// error of the tagged representation is returned.  Values other than
/// atoms are recorded and replayed, the recording borrows from the data.
pub fn untagged_fallback<'a, 'de, E: Send>(
    out: &'a mut Option<E>,
    name: &'static str,
    tagged: for<'x> fn(&'x mut Option<E>, &mut State) -> SinkHandle<'x, 'de>,
    variants: UntaggedVariants<'de, E>,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    RecordBuf::capture_with(
        FallbackCapture {
            out,
            name,
            tagged,
            variants,
        },
        state,
    )
}

/// The sink of a tagged enum with untagged variants passes the value on to
/// this.
struct FallbackCapture<'a, 'de, E> {
    out: &'a mut Option<E>,
    name: &'static str,
    tagged: for<'x> fn(&'x mut Option<E>, &mut State) -> SinkHandle<'x, 'de>,
    variants: UntaggedVariants<'de, E>,
}

impl<'a, 'de, E: Send> FallbackCapture<'a, 'de, E> {
    /// Tries the tagged representation and then the untagged variants.
    fn deliver(
        &mut self,
        state: &mut State,
        tagged: impl FnOnce(SinkHandle<'_, 'de>, &mut State) -> Result<(), Error>,
        input: UntaggedInput<'_, 'de>,
    ) -> Result<(), Error> {
        // the tagged representation can take event data (like CBOR tags),
        // the untagged variants get the data of the event again
        let data = match input {
            UntaggedInput::Recorded(_) => EventData::new(),
            _ => state.extensions().capture_event_data(),
        };
        let err = match tagged((self.tagged)(self.out, state), state) {
            Ok(()) if self.out.is_some() => return Ok(()),
            Ok(()) => Error::new(ErrorKind::InvalidState, "enum was not deserialized"),
            Err(err) => err,
        };
        *self.out = None;
        // the error of the tagged representation is returned
        *self.out = Some(try_untagged(self.variants, input, data, state).ok_or(err)?);
        Ok(())
    }
}

impl<'a, 'de, E: Send> Capture<'de, RecordBuf<'de>> for FallbackCapture<'a, 'de, E> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.name)
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let input = UntaggedInput::Atom(atom.as_borrowed());
        self.deliver(
            state,
            |mut sink, state| {
                sink.atom(atom.as_borrowed(), state)?;
                sink.finish(state)
            },
            input,
        )
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        let tagged_atom = atom.clone();
        self.deliver(
            state,
            |mut sink, state| {
                sink.borrowed_atom(tagged_atom, state)?;
                sink.finish(state)
            },
            UntaggedInput::Borrowed(atom),
        )
    }

    fn recorded(&mut self, buffer: RecordBuf<'de>, state: &mut State) -> Result<(), Error> {
        self.deliver(
            state,
            |sink, state| buffer.replay(sink, state),
            UntaggedInput::Recorded(&buffer),
        )
    }
}

/// A sink for internally tagged enums.
///
/// Until the tag is known, all key value pairs are recorded.  Once the tag
/// was seen, the recorded pairs are replayed into the variant and all further
/// pairs are forwarded to it directly.
///
/// Newtype variants of unit structs are given by the tag alone: they
/// receive null if there are no other keys (other content that does not
/// accept maps is an error, like `None`, which is not serialized as the tag
/// alone either).
pub struct InternallyTaggedSink<'a, 'de, E> {
    out: &'a mut Option<E>,
    tag: EnumKey,
    name: &'static str,
    variants: Variants<'a, 'de, E>,
    key: RecordBuf<'de>,
    pending: Vec<(RecordBuf<'de>, RecordBuf<'de>)>,
    tag_value: Option<RecordBuf<'de>>,
    variant: Option<ArenaVariant<'a, 'de, E>>,
    // if the key of the variant was recorded to look for the tag
    variant_key: bool,
    // if the enum is flattened into a struct
    flattened: bool,
    // the errors for keys that were taken but the variant did not use
    unclaimed: Vec<Error>,
    // the error of a unit struct variant for the map, which is reported
    // once it receives a key (without keys it receives null instead, see
    // `finish`)
    map_error: Option<Error>,
}

/// Returns the error of a variant that does not accept maps.
///
/// The error is reported once, when the variant receives the first key.
#[inline]
fn check_map(map_error: &mut Option<Error>) -> Result<(), Error> {
    match map_error.take() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

impl<'a, 'de, E: Send> InternallyTaggedSink<'a, 'de, E> {
    /// Creates a sink handle for an internally tagged enum.
    pub fn handle(
        out: &'a mut Option<E>,
        tag: EnumKey,
        name: &'static str,
        variants: Variants<'a, 'de, E>,
        state: &mut State,
    ) -> SinkHandle<'a, 'de> {
        SinkHandle::arena(
            InternallyTaggedSink {
                out,
                tag,
                name,
                variants,
                key: RecordBuf::new(),
                pending: Vec::new(),
                tag_value: None,
                variant: None,
                variant_key: false,
                flattened: false,
                unclaimed: Vec::new(),
                map_error: None,
            },
            state,
        )
    }

    /// Starts a variant and replays the pairs recorded so far into it.
    fn start_variant(
        &mut self,
        mut variant: ArenaVariant<'a, 'de, E>,
        state: &mut State,
    ) -> Result<(), Error> {
        if let Err(err) = variant.sink().map(state) {
            if !self.pending.is_empty() || !variant.unit_struct() {
                return Err(err);
            }
            self.map_error = Some(err);
        }
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

impl<'a, 'de, E: Send> Sink<'de> for InternallyTaggedSink<'a, 'de, E> {
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
        Ok(self.key.recorder(state))
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.ensure_variant(state)?;
        match self.variant {
            Some(ref mut variant) => {
                if atom.as_str().is_some_and(|key| self.tag.matches(key)) {
                    return Err(duplicate_key("tag", self.tag.name));
                }
                check_map(&mut self.map_error)?;
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
                check_map(&mut self.map_error)?;
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
                check_map(&mut self.map_error)?;
                key.replay(variant.sink().next_key(state)?, state)?;
            }
            return variant.sink().next_value(state);
        }
        let key = take(&mut self.key);
        if self.tag.matches_recorded(&key) {
            if self.tag_value.is_some() {
                return Err(duplicate_key("tag", self.tag.name));
            }
            return Ok(self.tag_value.insert(RecordBuf::new()).recorder(state));
        }
        self.pending.push((key, RecordBuf::new()));
        Ok(self.pending.last_mut().unwrap().1.recorder(state))
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
            return Ok(Some(
                self.tag_value.insert(RecordBuf::new()).recorder(state),
            ));
        }
        self.ensure_variant(state)?;
        if let Some(variant) = &mut self.variant {
            check_map(&mut self.map_error)?;
            return variant.sink().value_for_key(key, state);
        }
        let mut recorded_key = RecordBuf::new();
        recorded_key.set_atom(&Atom::Str(Text::borrowed(key)), state);
        self.pending.push((recorded_key, RecordBuf::new()));
        Ok(Some(self.pending.last_mut().unwrap().1.recorder(state)))
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.ensure_variant(state)?;
        if self.variant.is_none() {
            let variant = self.variants.resolve_missing(self.tag.name, state)?;
            self.start_variant(variant, state)?;
        }
        let variant = self.variant.as_mut().unwrap();
        // a unit struct variant and there were no other keys: the variant
        // is the tag alone
        if let Some(err) = self.map_error.take()
            && variant.sink().atom(Atom::Null, state).is_err()
        {
            return Err(err);
        }
        variant.sink().finish(state)?;
        *self.out = variant.build();
        for err in take(&mut self.unclaimed) {
            report_unclaimed_key(err, state);
        }
        Ok(())
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        // once the variant is known the items are the items of the variant,
        // until then they are recorded (which does not fail)
        match self.variant {
            Some(ref mut variant) => variant.sink().recover(err, state),
            None => Err(err),
        }
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.name)
    }
}

/// A unit enum of the derive (an enum with only unit variants).
///
/// The derive generates this as a constant together with the lookup of
/// the names and the functions that set a variant by its index, everything
/// else exists once for all unit enums.
pub struct UnitEnum {
    /// Looks up the index of a variant by name.
    pub lookup: fn(Tag<'_>) -> Option<usize>,
    /// The names of the variants that can be deserialized (for errors).
    pub names: &'static [&'static str],
    /// What is expected in errors.
    pub expecting: &'static str,
    /// The index of the variant for unknown names.
    pub other: Option<usize>,
}

/// A function that sets a value to the variant of a unit enum by index.
pub(crate) type VariantSetter<T> = fn(&mut T, usize);

/// [`VariantSetter`] with the type of the value erased.
type ErasedVariantSetter = fn(NonNull<()>, usize);

/// Erases the type of the target of a [`VariantSetter`].
#[inline]
fn erase_setter<T>(target: &mut T, set: VariantSetter<T>) -> (NonNull<()>, ErasedVariantSetter) {
    // SAFETY: `&mut T` and `NonNull<()>` are ABI compatible (both are
    // pointers to sized types), and the setter is only ever called with
    // the target which is a valid `&mut T`.
    let set = unsafe { core::mem::transmute::<VariantSetter<T>, ErasedVariantSetter>(set) };
    (NonNull::from(target).cast(), set)
}

/// Sets the target to the variant of a unit enum for an atom.
///
/// This is how the fields of structs deserialize atoms into unit enums
/// (without a sink).
#[inline]
pub fn unit_enum_atom_into<T>(
    target: &mut T,
    set: VariantSetter<T>,
    atom: Atom<'_>,
    info: &UnitEnum,
) -> Result<(), Error> {
    let (target, set) = erase_setter(target, set);
    unit_enum_set(target, set, atom, info)
}

/// Sets the target to the variant of a unit enum for an atom.
///
/// This is not inlined so that it exists once for all unit enums.
#[inline(never)]
fn unit_enum_set(
    target: NonNull<()>,
    set: ErasedVariantSetter,
    atom: Atom<'_>,
    info: &UnitEnum,
) -> Result<(), Error> {
    let index = unit_variant(&atom, info.lookup, info.names, info.expecting, info.other)?;
    set(target, index);
    Ok(())
}

/// The sink of unit enums.
///
/// Unit enums of the derive use this for their sinks (to deserialize and
/// to update them), all they have to generate is the lookup and the
/// function that sets a variant.  The sink exists once for all types.
struct UnitEnumSink<'a> {
    target: NonNull<()>,
    set: ErasedVariantSetter,
    info: &'static UnitEnum,
    _marker: PhantomData<&'a mut ()>,
}

// SAFETY: the sink only allows the setter to mutate the target (a
// `&'a mut T` with `T: Send`, see `unit_enum_sink`).
unsafe impl Send for UnitEnumSink<'_> {}

/// Creates the sink of a unit enum which sets the target with the setter.
pub fn unit_enum_sink<'a, 'de, T: Send + 'a>(
    target: &'a mut T,
    set: VariantSetter<T>,
    info: &'static UnitEnum,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    // the setter is only ever called with the target which is a valid
    // `&'a mut T` for the lifetime of the sink
    let (target, set) = erase_setter(target, set);
    unit_enum_handle(target, set, info, state)
}

/// Creates the sink of [`unit_enum_sink`], this exists once for all types.
fn unit_enum_handle<'a, 'de>(
    target: NonNull<()>,
    set: ErasedVariantSetter,
    info: &'static UnitEnum,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    SinkHandle::arena(
        UnitEnumSink {
            target,
            set,
            info,
            _marker: PhantomData,
        },
        state,
    )
}

impl<'de> Sink<'de> for UnitEnumSink<'_> {
    fn atom(&mut self, atom: Atom, _state: &mut State) -> Result<(), Error> {
        unit_enum_set(self.target, self.set, atom, self.info)
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.info.expecting)
    }
}

/// A function that sets a value from an atom.
pub(crate) type AtomSetter<T> = for<'x> fn(&mut T, Atom<'x>, &mut State) -> Result<(), Error>;

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
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    // SAFETY: `&mut T` and `NonNull<()>` are ABI compatible (both are
    // pointers to sized types), and the setter is only ever called with
    // the target which is a valid `&'a mut T` for the lifetime of the sink.
    let set = unsafe { core::mem::transmute::<AtomSetter<T>, ErasedAtomSetter>(set) };
    SinkHandle::arena(
        AtomSink {
            target: NonNull::from(target).cast(),
            set,
            name,
            _marker: PhantomData,
        },
        state,
    )
}

impl<'de> Sink<'de> for AtomSink<'_> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        (self.set)(self.target, atom, state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.name)
    }
}
