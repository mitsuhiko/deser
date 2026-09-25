//! Support for enums with data.
//!
//! This is used by the derive but it's generic over the enum so that the
//! generated code stays small.  The different enum representations are
//! implemented by different sinks, all of which deserialize the content of a
//! variant through a [`VariantBuilder`].
use std::mem::take;

use crate::de::{Deserialize, OwnedSink, Recording, Sink, SinkHandle};
use crate::descriptors::Descriptor;
use crate::error::{Error, ErrorKind};
use crate::State;

/// Builds the value of an enum variant.
pub trait VariantBuilder<E> {
    /// Returns the sink for the variant's fields.
    fn sink(&mut self) -> &mut dyn Sink;
    /// Builds the enum value after the sink finished.
    fn build(&mut self) -> Option<E>;
}

/// A variant that is deserialized as `V` and then converted into `E`.
pub struct Variant<V, E> {
    sink: OwnedSink<V>,
    convert: fn(V) -> E,
}

impl<V: Deserialize + 'static, E: 'static> Variant<V, E> {
    /// Creates a boxed builder for a variant.
    pub fn boxed(convert: fn(V) -> E) -> Box<dyn VariantBuilder<E>> {
        Box::new(Variant {
            sink: OwnedSink::deserialize(),
            convert,
        })
    }
}

impl<V: Deserialize, E> VariantBuilder<E> for Variant<V, E> {
    fn sink(&mut self) -> &mut dyn Sink {
        self.sink.borrow_mut()
    }

    fn build(&mut self) -> Option<E> {
        self.sink.take().map(self.convert)
    }
}

/// A variant that ignores its content (used for `#[deser(other)]`).
pub struct IgnoredVariant<E> {
    sink: SinkHandle<'static>,
    make: fn() -> E,
}

impl<E: 'static> IgnoredVariant<E> {
    /// Creates a boxed builder for a variant which ignores its content.
    pub fn boxed(make: fn() -> E) -> Box<dyn VariantBuilder<E>> {
        Box::new(IgnoredVariant {
            sink: SinkHandle::null(),
            make,
        })
    }
}

impl<E> VariantBuilder<E> for IgnoredVariant<E> {
    fn sink(&mut self) -> &mut dyn Sink {
        &mut self.sink
    }

    fn build(&mut self) -> Option<E> {
        Some((self.make)())
    }
}

/// Looks up the variant for a tag.
pub type VariantLookup<E> = fn(&str) -> Option<Box<dyn VariantBuilder<E>>>;

/// Looks up a unit variant by name.
pub type UnitLookup<E> = fn(&str) -> Option<E>;

/// Creates the builder for the n-th variant of an untagged enum.
pub type CandidateLookup<E> = fn(usize) -> Option<Box<dyn VariantBuilder<E>>>;

fn unknown_variant(tag: &str, descriptor: &dyn Descriptor) -> Error {
    Error::new(
        ErrorKind::Unexpected,
        format!(
            "unknown variant '{}' for {}",
            tag,
            descriptor.name().unwrap_or("enum")
        ),
    )
}

/// A sink for externally tagged enums.
///
/// Unit variants are represented as strings, all other variants as maps with
/// a single key (the variant name) and the content as value.
pub struct ExternallyTaggedSink<'a, E> {
    out: &'a mut Option<E>,
    descriptor: &'static dyn Descriptor,
    lookup: VariantLookup<E>,
    unit: UnitLookup<E>,
    key: Option<String>,
    variant: Option<Box<dyn VariantBuilder<E>>>,
}

impl<'a, E: 'static> ExternallyTaggedSink<'a, E> {
    /// Creates a sink handle for an externally tagged enum.
    pub fn handle(
        out: &'a mut Option<E>,
        descriptor: &'static dyn Descriptor,
        lookup: VariantLookup<E>,
        unit: UnitLookup<E>,
    ) -> SinkHandle<'a> {
        SinkHandle::boxed(ExternallyTaggedSink {
            out,
            descriptor,
            lookup,
            unit,
            key: None,
            variant: None,
        })
    }
}

impl<'a, E: 'static> Sink for ExternallyTaggedSink<'a, E> {
    fn atom(&mut self, atom: crate::Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            crate::Atom::Str(ref name) => match (self.unit)(name) {
                Some(value) => {
                    *self.out = Some(value);
                    Ok(())
                }
                None => Err(unknown_variant(name, self.descriptor)),
            },
            other => self.unexpected_atom(other, state),
        }
    }

    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_>, Error> {
        if self.key.is_some() || self.variant.is_some() {
            return Err(Error::new(
                ErrorKind::Unexpected,
                format!("expected a map with a single key for {}", self.expecting()),
            ));
        }
        Ok(Deserialize::deserialize_into(&mut self.key))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_>, Error> {
        let key = self.key.take().unwrap_or_default();
        let variant = (self.lookup)(&key).ok_or_else(|| unknown_variant(&key, self.descriptor))?;
        Ok(SinkHandle::to(self.variant.insert(variant).sink()))
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        if self.out.is_some() {
            // unit variant from a string
            return Ok(());
        }
        let variant = self.variant.as_mut().ok_or_else(|| {
            Error::new(
                ErrorKind::Unexpected,
                format!(
                    "expected a map with a single key for {}",
                    self.descriptor.name().unwrap_or("enum")
                ),
            )
        })?;
        *self.out = variant.build();
        Ok(())
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.descriptor
    }
}

/// A sink for adjacently tagged enums.
///
/// The variant name is stored in the tag field and the content in the content
/// field.  If the content comes before the tag it's recorded and replayed
/// once the tag is known.
pub struct AdjacentlyTaggedSink<'a, E> {
    out: &'a mut Option<E>,
    tag: &'static str,
    content: &'static str,
    descriptor: &'static dyn Descriptor,
    lookup: VariantLookup<E>,
    key: Recording,
    tag_value: Option<String>,
    recorded_content: Option<Recording>,
    has_content: bool,
    variant: Option<Box<dyn VariantBuilder<E>>>,
}

impl<'a, E: 'static> AdjacentlyTaggedSink<'a, E> {
    /// Creates a sink handle for an adjacently tagged enum.
    pub fn handle(
        out: &'a mut Option<E>,
        tag: &'static str,
        content: &'static str,
        descriptor: &'static dyn Descriptor,
        lookup: VariantLookup<E>,
    ) -> SinkHandle<'a> {
        SinkHandle::boxed(AdjacentlyTaggedSink {
            out,
            tag,
            content,
            descriptor,
            lookup,
            key: Recording::new(),
            tag_value: None,
            recorded_content: None,
            has_content: false,
            variant: None,
        })
    }

    fn ensure_variant(&mut self, state: &mut State) -> Result<(), Error> {
        if self.variant.is_some() {
            return Ok(());
        }
        let tag = match self.tag_value {
            Some(ref tag) => tag,
            None => return Ok(()),
        };
        let mut variant =
            (self.lookup)(tag).ok_or_else(|| unknown_variant(tag, self.descriptor))?;
        if let Some(content) = self.recorded_content.take() {
            content.replay(SinkHandle::to(variant.sink()), state)?;
        }
        self.variant = Some(variant);
        Ok(())
    }

    fn duplicate(&self, key: &str) -> Error {
        Error::new(ErrorKind::Unexpected, format!("duplicate field '{}'", key))
    }
}

impl<'a, E: 'static> Sink for AdjacentlyTaggedSink<'a, E> {
    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_>, Error> {
        self.ensure_variant(state)?;
        Ok(self.key.recorder())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_>, Error> {
        let key = take(&mut self.key);
        if key.as_str() == Some(self.tag) {
            if self.tag_value.is_some() {
                return Err(self.duplicate(self.tag));
            }
            Ok(Deserialize::deserialize_into(&mut self.tag_value))
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
        let variant = self.variant.as_mut().ok_or_else(|| {
            Error::new(
                ErrorKind::MissingField,
                format!("missing tag '{}'", self.tag),
            )
        })?;
        if !self.has_content {
            // variants without content (unit variants) are fed a null
            let sink = variant.sink();
            sink.atom(crate::Atom::Null, state)?;
            sink.finish(state)?;
        }
        *self.out = variant.build();
        Ok(())
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.descriptor
    }
}

/// Creates a sink handle for an untagged enum.
///
/// The value is recorded and replayed into the variants in order until one
/// of them accepts it.
pub fn untagged_handle<'a, E: 'static>(
    out: &'a mut Option<E>,
    descriptor: &'static dyn Descriptor,
    candidates: CandidateLookup<E>,
) -> SinkHandle<'a> {
    Recording::capture(move |recording, state| {
        for index in 0.. {
            let mut variant = match candidates(index) {
                Some(variant) => variant,
                None => break,
            };
            if recording
                .replay(SinkHandle::to(variant.sink()), state)
                .is_ok()
            {
                if let Some(value) = variant.build() {
                    *out = Some(value);
                    return Ok(());
                }
            }
        }
        Err(Error::new(
            ErrorKind::Unexpected,
            format!(
                "data did not match any variant of {}",
                descriptor.name().unwrap_or("enum")
            ),
        ))
    })
}

/// A sink for internally tagged enums.
///
/// Until the tag is known, all key value pairs are recorded.  Once the tag
/// was seen, the recorded pairs are replayed into the variant and all further
/// pairs are forwarded to it directly.
pub struct InternallyTaggedSink<'a, E> {
    out: &'a mut Option<E>,
    tag: &'static str,
    descriptor: &'static dyn Descriptor,
    lookup: VariantLookup<E>,
    key: Recording,
    pending: Vec<(Recording, Recording)>,
    tag_value: Option<String>,
    variant: Option<Box<dyn VariantBuilder<E>>>,
}

impl<'a, E: 'static> InternallyTaggedSink<'a, E> {
    /// Creates a sink handle for an internally tagged enum.
    pub fn handle(
        out: &'a mut Option<E>,
        tag: &'static str,
        descriptor: &'static dyn Descriptor,
        lookup: VariantLookup<E>,
    ) -> SinkHandle<'a> {
        SinkHandle::boxed(InternallyTaggedSink {
            out,
            tag,
            descriptor,
            lookup,
            key: Recording::new(),
            pending: Vec::new(),
            tag_value: None,
            variant: None,
        })
    }

    /// Creates the variant once the tag is known and replays the pairs
    /// recorded so far into it.
    fn ensure_variant(&mut self, state: &mut State) -> Result<(), Error> {
        if self.variant.is_some() {
            return Ok(());
        }
        let tag = match self.tag_value {
            Some(ref tag) => tag,
            None => return Ok(()),
        };
        let mut variant =
            (self.lookup)(tag).ok_or_else(|| unknown_variant(tag, self.descriptor))?;
        variant.sink().map(state)?;
        for (key, value) in take(&mut self.pending) {
            key.replay(variant.sink().next_key(state)?, state)?;
            value.replay(variant.sink().next_value(state)?, state)?;
        }
        self.variant = Some(variant);
        Ok(())
    }
}

impl<'a, E: 'static> Sink for InternallyTaggedSink<'a, E> {
    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_>, Error> {
        self.ensure_variant(state)?;
        if let Some(variant) = &mut self.variant {
            return variant.sink().next_key(state);
        }
        Ok(self.key.recorder())
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_>, Error> {
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
            return Ok(Deserialize::deserialize_into(&mut self.tag_value));
        }
        self.pending.push((key, Recording::new()));
        Ok(self.pending.last_mut().unwrap().1.recorder())
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.ensure_variant(state)?;
        let variant = self.variant.as_mut().ok_or_else(|| {
            Error::new(
                ErrorKind::MissingField,
                format!("missing tag '{}'", self.tag),
            )
        })?;
        variant.sink().finish(state)?;
        *self.out = variant.build();
        Ok(())
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.descriptor
    }
}
