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
use std::mem::take;

use crate::State;
use crate::de::{Deserialize, OwnedSink, Recording, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
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

            fn key_atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
                Ok(())
            }

            fn value_atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
                Ok(())
            }

            fn borrowed_key_atom(
                &mut self,
                _atom: Atom<'de>,
                _state: &mut State,
            ) -> Result<(), Error> {
                Ok(())
            }

            fn borrowed_value_atom(
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

/// Looks up a variant by tag.
pub type VariantLookup<'de, E> = fn(&str) -> Option<BoxedVariant<'de, E>>;

/// Creates the builder of a special variant.
pub type VariantMaker<'de, E> = fn() -> BoxedVariant<'de, E>;

/// Looks up a unit variant by name.
pub type UnitLookup<E> = fn(&str) -> Option<E>;

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
}

impl<'de, E> Clone for Variants<'de, E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'de, E> Copy for Variants<'de, E> {}

impl<'de, E> Variants<'de, E> {
    /// Returns the variant for a recorded tag.
    fn resolve(
        &self,
        tag: &Recording,
        name: &str,
        state: &mut State,
    ) -> Result<BoxedVariant<'de, E>, Error> {
        let tag_name = tag_name(tag);
        if let Some(ref tag_name) = tag_name
            && let Some(variant) = (self.lookup)(tag_name)
        {
            return Ok(variant);
        }
        match self.other {
            Some(other) => {
                let mut variant = other();
                variant.set_tag(Some(tag), state)?;
                Ok(variant)
            }
            None => Err(unknown_variant(tag_name.as_deref(), name)),
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
                format!("missing tag '{}'", tag),
            )),
        }
    }
}

/// Returns the name of a recorded tag if it's a string.
///
/// Extension values are lowered to their fallback.
fn tag_name(tag: &Recording) -> Option<Cow<'_, str>> {
    let mut events = tag.events();
    let atom = match (events.next(), events.next()) {
        (Some(Event::Atom(atom)), None) => atom,
        _ => return None,
    };
    match atom {
        Atom::Str(name) | Atom::Lexical(name) => Some(Cow::Borrowed(name)),
        Atom::Ext(ext) => match ext.fallback() {
            Atom::Str(name) => Some(Cow::Owned(name.into_owned())),
            _ => None,
        },
        _ => None,
    }
}

fn unknown_variant(tag: Option<&str>, name: &str) -> Error {
    Error::new(
        ErrorKind::Unexpected,
        match tag {
            Some(tag) => format!("unknown variant '{}' for {}", tag, name),
            None => format!("unknown variant for {}", name),
        },
    )
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
        let mut variant = match atom {
            Atom::Ext(_) => return self.unexpected_atom(atom, state),
            Atom::Str(ref name) | Atom::Lexical(ref name) => {
                if let Some(value) = (self.unit)(name) {
                    *self.out = Some(value);
                    self.done = true;
                    return Ok(());
                }
                (self.variants.lookup)(name)
            }
            _ => None,
        };
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
                return match atom {
                    Atom::Str(ref name) | Atom::Lexical(ref name) => {
                        Err(unknown_variant(Some(name), self.name))
                    }
                    other => self.unexpected_atom(other, state),
                };
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

    fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.begin_key()?;
        self.key.set_atom(&atom, state);
        Ok(())
    }

    fn borrowed_key_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.key_atom(atom, state)
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

/// A sink for adjacently tagged enums.
///
/// The variant name is stored in the tag field and the content in the content
/// field.  If the content comes before the tag it's recorded and replayed
/// once the tag is known.
pub struct AdjacentlyTaggedSink<'a, 'de, E> {
    out: &'a mut Option<E>,
    tag: &'static str,
    content: &'static str,
    name: &'static str,
    variants: Variants<'de, E>,
    key: Recording,
    tag_value: Option<Recording>,
    recorded_content: Option<Recording>,
    has_content: bool,
    variant: Option<BoxedVariant<'de, E>>,
}

impl<'a, 'de, E: Send + 'de> AdjacentlyTaggedSink<'a, 'de, E> {
    /// Creates a sink handle for an adjacently tagged enum.
    pub fn handle(
        out: &'a mut Option<E>,
        tag: &'static str,
        content: &'static str,
        name: &'static str,
        variants: Variants<'de, E>,
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

    fn duplicate(&self, key: &str) -> Error {
        Error::new(ErrorKind::Unexpected, format!("duplicate field '{}'", key))
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

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        let key = take(&mut self.key);
        if key.as_str() == Some(self.tag) {
            if self.tag_value.is_some() {
                return Err(self.duplicate(self.tag));
            }
            Ok(self.tag_value.insert(Recording::new()).recorder())
        } else if key.as_str() == Some(self.content) {
            if self.has_content {
                return Err(self.duplicate(self.content));
            }
            self.has_content = true;
            Ok(match self.variant {
                Some(ref mut variant) => SinkHandle::to(variant.sink()),
                None => self.recorded_content.insert(Recording::new()).recorder(),
            })
        } else {
            Ok(SinkHandle::null())
        }
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.ensure_variant(state)?;
        if self.variant.is_none() {
            let variant = self.variants.resolve_missing(self.tag, state)?;
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
    tag: &'static str,
    name: &'static str,
    variants: Variants<'de, E>,
    key: Recording,
    pending: Vec<(Recording, Recording)>,
    tag_value: Option<Recording>,
    variant: Option<BoxedVariant<'de, E>>,
}

impl<'a, 'de, E: Send + 'de> InternallyTaggedSink<'a, 'de, E> {
    /// Creates a sink handle for an internally tagged enum.
    pub fn handle(
        out: &'a mut Option<E>,
        tag: &'static str,
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
            key.replay(variant.sink().next_key(state)?, state)?;
            value.replay(variant.sink().next_value(state)?, state)?;
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

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.ensure_variant(state)?;
        if let Some(variant) = &mut self.variant {
            return variant.sink().next_key(state);
        }
        Ok(self.key.recorder())
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        if let Some(variant) = &mut self.variant {
            return variant.sink().next_value(state);
        }
        let key = take(&mut self.key);
        if key.as_str() == Some(self.tag) {
            if self.tag_value.is_some() {
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    format!("duplicate tag '{}'", self.tag),
                ));
            }
            return Ok(self.tag_value.insert(Recording::new()).recorder());
        }
        self.pending.push((key, Recording::new()));
        Ok(self.pending.last_mut().unwrap().1.recorder())
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.ensure_variant(state)?;
        if self.variant.is_none() {
            let variant = self.variants.resolve_missing(self.tag, state)?;
            self.start_variant(variant, state)?;
        }
        let variant = self.variant.as_mut().unwrap();
        variant.sink().finish(state)?;
        *self.out = variant.build();
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.name)
    }
}
