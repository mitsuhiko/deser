//! Support for internally tagged enums.
//!
//! This is used by the derive but it's generic over the enum so that the
//! generated code stays small.
use std::mem::take;

use crate::de::{Deserialize, DeserializerState, OwnedSink, Recording, Sink, SinkHandle};
use crate::descriptors::Descriptor;
use crate::error::{Error, ErrorKind};

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

/// Looks up the variant for a tag.
pub type VariantLookup<E> = fn(&str) -> Option<Box<dyn VariantBuilder<E>>>;

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
    fn ensure_variant(&mut self, state: &DeserializerState) -> Result<(), Error> {
        if self.variant.is_some() {
            return Ok(());
        }
        let tag = match self.tag_value {
            Some(ref tag) => tag,
            None => return Ok(()),
        };
        let mut variant = (self.lookup)(tag).ok_or_else(|| {
            Error::new(
                ErrorKind::Unexpected,
                format!("unknown variant '{}' for {}", tag, self.expecting()),
            )
        })?;
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
    fn map(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, state: &DeserializerState) -> Result<SinkHandle<'_>, Error> {
        self.ensure_variant(state)?;
        if let Some(variant) = &mut self.variant {
            return variant.sink().next_key(state);
        }
        Ok(self.key.recorder())
    }

    fn next_value(&mut self, state: &DeserializerState) -> Result<SinkHandle<'_>, Error> {
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

    fn finish(&mut self, state: &DeserializerState) -> Result<(), Error> {
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
