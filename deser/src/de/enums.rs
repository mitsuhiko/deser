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

use crate::de::{Deserialize, OwnedSink, Recording, Sink, SinkHandle};
use crate::descriptors::Descriptor;
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, Event};
use crate::State;

/// Builds the value of an enum variant.
pub trait VariantBuilder<E> {
    /// Returns the sink for the variant's fields.
    fn sink(&mut self) -> &mut dyn Sink;

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

/// A variant that captures its tag (used for `#[deser(other)]`).
///
/// The tag is deserialized as `T` and the content as `C`.
pub struct OtherVariant<T, C, E> {
    tag: Option<T>,
    content: OwnedSink<C>,
    convert: fn(T, C) -> E,
}

impl<T: Deserialize + 'static, C: Deserialize + 'static, E: 'static> OtherVariant<T, C, E> {
    /// Creates a boxed builder for a variant which captures its tag.
    pub fn boxed(convert: fn(T, C) -> E) -> Box<dyn VariantBuilder<E>> {
        Box::new(OtherVariant {
            tag: None,
            content: OwnedSink::deserialize(),
            convert,
        })
    }
}

impl<T: Deserialize, C: Deserialize, E> VariantBuilder<E> for OtherVariant<T, C, E> {
    fn sink(&mut self) -> &mut dyn Sink {
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

impl Deserialize for IgnoredContent {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        struct IgnoredContentSink<'a>(&'a mut Option<IgnoredContent>);

        impl<'a> Sink for IgnoredContentSink<'a> {
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

            fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
                *self.0 = Some(IgnoredContent);
                Ok(())
            }
        }

        SinkHandle::boxed(IgnoredContentSink(out))
    }
}

/// Looks up a variant by tag.
pub type VariantLookup<E> = fn(&str) -> Option<Box<dyn VariantBuilder<E>>>;

/// Creates the builder of a special variant.
pub type VariantMaker<E> = fn() -> Box<dyn VariantBuilder<E>>;

/// Looks up a unit variant by name.
pub type UnitLookup<E> = fn(&str) -> Option<E>;

/// Creates the builder for the n-th variant of an untagged enum.
pub type CandidateLookup<E> = fn(usize) -> Option<Box<dyn VariantBuilder<E>>>;

/// The variants of a tagged enum.
pub struct Variants<E: 'static> {
    /// Looks up the known variants by tag.
    pub lookup: VariantLookup<E>,
    /// Creates the variant for unknown tags (`#[deser(other)]`).
    pub other: Option<VariantMaker<E>>,
    /// Creates the variant for missing tags (`#[deser(default)]`).
    pub default: Option<VariantMaker<E>>,
}

impl<E: 'static> Clone for Variants<E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E: 'static> Copy for Variants<E> {}

impl<E: 'static> Variants<E> {
    /// Returns the variant for a recorded tag.
    fn resolve(
        &self,
        tag: &Recording,
        descriptor: &dyn Descriptor,
        state: &mut State,
    ) -> Result<Box<dyn VariantBuilder<E>>, Error> {
        let name = tag_name(tag);
        if let Some(ref name) = name {
            if let Some(variant) = (self.lookup)(name) {
                return Ok(variant);
            }
        }
        match self.other {
            Some(other) => {
                let mut variant = other();
                variant.set_tag(Some(tag), state)?;
                Ok(variant)
            }
            None => Err(unknown_variant(name.as_deref(), descriptor)),
        }
    }

    /// Returns the variant for a missing tag.
    fn resolve_missing(
        &self,
        tag: &str,
        state: &mut State,
    ) -> Result<Box<dyn VariantBuilder<E>>, Error> {
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
        Atom::Str(name) => Some(Cow::Borrowed(name)),
        Atom::Ext(ext) => match ext.fallback() {
            Atom::Str(name) => Some(Cow::Owned(name.into_owned())),
            _ => None,
        },
        _ => None,
    }
}

fn unknown_variant(tag: Option<&str>, descriptor: &dyn Descriptor) -> Error {
    let name = descriptor.name().unwrap_or("enum");
    Error::new(
        ErrorKind::Unexpected,
        match tag {
            Some(tag) => format!("unknown variant '{}' for {}", tag, name),
            None => format!("unknown variant for {}", name),
        },
    )
}

/// Feeds a null to a variant which has no content.
fn feed_null<E>(variant: &mut dyn VariantBuilder<E>, state: &mut State) -> Result<(), Error> {
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
pub struct ExternallyTaggedSink<'a, E: 'static> {
    out: &'a mut Option<E>,
    descriptor: &'static dyn Descriptor,
    variants: Variants<E>,
    unit: UnitLookup<E>,
    key: Recording,
    has_key: bool,
    done: bool,
    variant: Option<Box<dyn VariantBuilder<E>>>,
}

impl<'a, E: 'static> ExternallyTaggedSink<'a, E> {
    /// Creates a sink handle for an externally tagged enum.
    pub fn handle(
        out: &'a mut Option<E>,
        descriptor: &'static dyn Descriptor,
        variants: Variants<E>,
        unit: UnitLookup<E>,
    ) -> SinkHandle<'a> {
        SinkHandle::boxed(ExternallyTaggedSink {
            out,
            descriptor,
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

impl<'a, E: 'static> Sink for ExternallyTaggedSink<'a, E> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let mut variant = match atom {
            Atom::Ext(_) => return self.unexpected_atom(atom, state),
            Atom::Str(ref name) => {
                if let Some(value) = (self.unit)(name) {
                    *self.out = Some(value);
                    self.done = true;
                    return Ok(());
                }
                (self.variants.lookup)(name)
            }
            _ => None,
        };
        if variant.is_none() {
            if let Some(other) = self.variants.other {
                let mut tag = Recording::new();
                tag.set_atom(&atom, state);
                let mut other = other();
                other.set_tag(Some(&tag), state)?;
                variant = Some(other);
            }
        }
        let mut variant = match variant {
            Some(variant) => variant,
            None => {
                return match atom {
                    Atom::Str(ref name) => Err(unknown_variant(Some(name), self.descriptor)),
                    other => self.unexpected_atom(other, state),
                }
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

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_>, Error> {
        self.begin_key()?;
        Ok(self.key.recorder())
    }

    fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.begin_key()?;
        self.key.set_atom(&atom, state);
        Ok(())
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_>, Error> {
        let variant = self.variants.resolve(&self.key, self.descriptor, state)?;
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
pub struct AdjacentlyTaggedSink<'a, E: 'static> {
    out: &'a mut Option<E>,
    tag: &'static str,
    content: &'static str,
    descriptor: &'static dyn Descriptor,
    variants: Variants<E>,
    key: Recording,
    tag_value: Option<Recording>,
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
        variants: Variants<E>,
    ) -> SinkHandle<'a> {
        SinkHandle::boxed(AdjacentlyTaggedSink {
            out,
            tag,
            content,
            descriptor,
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
        mut variant: Box<dyn VariantBuilder<E>>,
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
            Some(ref tag) => self.variants.resolve(tag, self.descriptor, state)?,
            None => return Ok(()),
        };
        self.start_variant(variant, state)
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
pub struct InternallyTaggedSink<'a, E: 'static> {
    out: &'a mut Option<E>,
    tag: &'static str,
    descriptor: &'static dyn Descriptor,
    variants: Variants<E>,
    key: Recording,
    pending: Vec<(Recording, Recording)>,
    tag_value: Option<Recording>,
    variant: Option<Box<dyn VariantBuilder<E>>>,
}

impl<'a, E: 'static> InternallyTaggedSink<'a, E> {
    /// Creates a sink handle for an internally tagged enum.
    pub fn handle(
        out: &'a mut Option<E>,
        tag: &'static str,
        descriptor: &'static dyn Descriptor,
        variants: Variants<E>,
    ) -> SinkHandle<'a> {
        SinkHandle::boxed(InternallyTaggedSink {
            out,
            tag,
            descriptor,
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
        mut variant: Box<dyn VariantBuilder<E>>,
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
            Some(ref tag) => self.variants.resolve(tag, self.descriptor, state)?,
            None => return Ok(()),
        };
        self.start_variant(variant, state)
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

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.descriptor
    }
}
