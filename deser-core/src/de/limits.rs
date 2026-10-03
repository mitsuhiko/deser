use alloc::vec::Vec;

use crate::de::{Layer, LayerEvent, Next};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, Event};

/// Limits the size of the deserialized data (a value of the
/// [`Context`](crate::Context)).
///
/// Deser does not use the stack to process nested data, so deeply nested
/// input cannot overflow the stack during deserialization.  Still it can
/// be useful to limit the size of untrusted input, as the values that are
/// deserialized might be processed recursively later, or to limit the
/// memory used.  All limits are off by default.
///
/// The limits are configured in the context, for instance in the
/// configuration of a format:
///
/// ```
/// use deser::de::Limits;
/// use deser::Context;
///
/// let config = deser_json::DeserializerConfig::builder()
///     .context(Context::with(Limits::builder().max_depth(1).build()))
///     .build();
/// let err = config.from_str::<Vec<Vec<u32>>>("[[1]]").unwrap_err();
/// assert_eq!(
///     err.to_string(),
///     "LimitExceeded: recursion limit exceeded at line 1 column 2"
/// );
/// ```
///
/// The [`DeserializeDriver`](crate::de::DeserializeDriver) enforces the
/// limits of its context (see
/// [`set_context`](crate::de::DeserializeDriver::set_context)).  They see
/// the events as the sinks receive them, after all [`Layer`]s, so their
/// errors have the context that layers add (for instance the path).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    max_depth: Option<usize>,
    max_events: Option<usize>,
    max_items: Option<usize>,
    max_len: Option<usize>,
}

impl Limits {
    /// Creates limits that limit nothing.
    pub const fn new() -> Limits {
        Limits {
            max_depth: None,
            max_events: None,
            max_items: None,
            max_len: None,
        }
    }

    /// Returns a builder for the limits (see [`LimitsBuilder`]).
    pub const fn builder() -> LimitsBuilder {
        LimitsBuilder::new()
    }

    /// Returns a builder that starts with these limits.
    pub const fn into_builder(self) -> LimitsBuilder {
        LimitsBuilder { value: self }
    }

    /// Limits the nesting depth of maps and sequences.
    ///
    /// With a depth of 1, maps and sequences cannot contain other maps and
    /// sequences.
    pub const fn set_max_depth(&mut self, depth: usize) {
        self.max_depth = Some(depth);
    }

    /// Returns the limit of the nesting depth of maps and sequences.
    pub const fn max_depth(&self) -> Option<usize> {
        self.max_depth
    }

    /// Limits the total number of events.
    ///
    /// Every atom and every start and end of a map or sequence counts.
    pub const fn set_max_events(&mut self, events: usize) {
        self.max_events = Some(events);
    }

    /// Returns the limit of the total number of events.
    pub const fn max_events(&self) -> Option<usize> {
        self.max_events
    }

    /// Limits the number of items in a sequence and entries in a map.
    pub const fn set_max_items(&mut self, items: usize) {
        self.max_items = Some(items);
    }

    /// Returns the limit of the number of items in a sequence and entries
    /// in a map.
    pub const fn max_items(&self) -> Option<usize> {
        self.max_items
    }

    /// Limits the length of strings and bytes (in bytes).
    pub const fn set_max_len(&mut self, len: usize) {
        self.max_len = Some(len);
    }

    /// Returns the limit of the length of strings and bytes (in bytes).
    pub const fn max_len(&self) -> Option<usize> {
        self.max_len
    }

    /// Returns `true` if nothing is limited.
    pub(crate) const fn is_unlimited(&self) -> bool {
        self.max_depth.is_none()
            && self.max_events.is_none()
            && self.max_items.is_none()
            && self.max_len.is_none()
    }
}

/// Builds [`Limits`].
///
/// The methods have the names of the setters of [`Limits`] (without `set_`).
#[derive(Debug, Clone)]
#[must_use]
pub struct LimitsBuilder {
    value: Limits,
}

impl LimitsBuilder {
    /// Creates a builder that starts with the default.
    pub const fn new() -> LimitsBuilder {
        LimitsBuilder {
            value: Limits::new(),
        }
    }

    /// Limits the nesting depth of maps and sequences.
    ///
    /// See [`Limits::set_max_depth`].
    pub const fn max_depth(mut self, depth: usize) -> LimitsBuilder {
        self.value.set_max_depth(depth);
        self
    }

    /// Limits the total number of events.
    ///
    /// See [`Limits::set_max_events`].
    pub const fn max_events(mut self, events: usize) -> LimitsBuilder {
        self.value.set_max_events(events);
        self
    }

    /// Limits the number of items in a sequence and entries in a map.
    ///
    /// See [`Limits::set_max_items`].
    pub const fn max_items(mut self, items: usize) -> LimitsBuilder {
        self.value.set_max_items(items);
        self
    }

    /// Limits the length of strings and bytes (in bytes).
    ///
    /// See [`Limits::set_max_len`].
    pub const fn max_len(mut self, len: usize) -> LimitsBuilder {
        self.value.set_max_len(len);
        self
    }

    /// Returns the built [`Limits`].
    pub const fn build(self) -> Limits {
        self.value
    }
}

impl Default for LimitsBuilder {
    fn default() -> LimitsBuilder {
        LimitsBuilder::new()
    }
}

/// The layer that enforces [`Limits`] (installed by the driver).
pub(crate) struct LimitsLayer {
    limits: Limits,
    events: usize,
    // the number of items of the open containers if items are limited
    items: Vec<(bool, usize)>,
}

impl LimitsLayer {
    pub(crate) fn new(limits: Limits) -> LimitsLayer {
        LimitsLayer {
            limits,
            events: 0,
            items: Vec::new(),
        }
    }

    /// Accounts for an item in the current container.
    fn count_item(&mut self, is_map_key: bool) -> Result<(), Error> {
        if let (Some(max), Some((is_map, count))) = (self.limits.max_items, self.items.last_mut())
            && (!*is_map || is_map_key)
        {
            *count += 1;
            if *count > max {
                return Err(limit_error("too many items"));
            }
        }
        Ok(())
    }
}

#[cold]
fn limit_error(msg: &'static str) -> Error {
    Error::new(ErrorKind::LimitExceeded, msg)
}

impl Layer for LimitsLayer {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        if let Some(max) = self.limits.max_events {
            self.events += 1;
            if self.events > max {
                return Err(limit_error("too many events"));
            }
        }
        let is_map_key = next.state().is_map_key();
        match event.event() {
            Event::MapStart(_) | Event::SeqStart(_) => {
                if self
                    .limits
                    .max_depth
                    .is_some_and(|max| next.state().depth() >= max)
                {
                    return Err(limit_error("recursion limit exceeded"));
                }
                self.count_item(is_map_key)?;
                if self.limits.max_items.is_some() {
                    self.items
                        .push((matches!(event.event(), Event::MapStart(_)), 0));
                }
            }
            Event::MapEnd | Event::SeqEnd => {
                self.items.pop();
            }
            Event::Atom(atom) => {
                if let Some(max) = self.limits.max_len {
                    let len = match atom {
                        Atom::Str(s) | Atom::Lexical(s) => s.len(),
                        Atom::Bytes(b) => b.len(),
                        _ => 0,
                    };
                    if len > max {
                        return Err(limit_error("string or bytes too long"));
                    }
                }
                self.count_item(is_map_key)?;
            }
        }
        next.emit(event)
    }
}
