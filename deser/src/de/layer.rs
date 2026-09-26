use crate::State;
use crate::de::driver::DriverCore;
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, Event};

/// A layer between a format and the sinks.
///
/// Layers are added to a [`DeserializeDriver`](crate::de::DeserializeDriver)
/// with [`push_layer`](crate::de::DeserializeDriver::push_layer).  Every
/// event that is emitted into the driver passes through the layers before
/// it's delivered to the sinks.  A layer receives the event together with
/// a [`Next`] which passes events on to the next layer (or the sinks).  This
/// way a layer can:
///
/// * observe events and track information in the [`State`] (for instance
///   the current path),
/// * reject events by returning an error (for instance to enforce limits),
/// * change events, drop them or emit additional events,
/// * act on the result of the events that it passed on.
///
/// The position of the event is known when a layer is invoked:
/// [`State::is_map_key`] tells if the event is (the start of) a map key
/// and [`State::depth`] is the number of open containers.
///
/// ```
/// use deser::de::{DeserializeDriver, Layer, LayerEvent, Next};
/// use deser::{Atom, Error, Event};
///
/// /// Upper cases all strings that are not map keys.
/// struct Uppercase;
///
/// impl Layer for Uppercase {
///     fn event<'de>(
///         &mut self,
///         event: LayerEvent<'_, 'de>,
///         next: &mut Next<'_, 'de>,
///     ) -> Result<(), Error> {
///         match event.event() {
///             Event::Atom(Atom::Str(s)) if !next.state().is_map_key() => {
///                 next.emit(LayerEvent::new(Event::from(s.to_uppercase())))
///             }
///             _ => next.emit(event),
///         }
///     }
/// }
///
/// let mut out = None::<Vec<String>>;
/// {
///     let mut driver = DeserializeDriver::new(&mut out);
///     driver.push_layer(Uppercase);
///     driver.emit(Event::seq_start()).unwrap();
///     driver.emit("hello").unwrap();
///     driver.emit(Event::SeqEnd).unwrap();
/// }
/// assert_eq!(out.unwrap(), ["HELLO"]);
/// ```
///
/// # Buffered Values
///
/// Some types buffer values and replay them later (see
/// [`Recording`](crate::de::Recording)).  Replayed events do not pass
/// through the layers again as they already did when they were recorded,
/// this means that changes made by layers are retained and not applied
/// twice.  Information that layers keep in the state and that is needed
/// for replayed values should be kept in replayable extensions (see
/// [`State::set_replayable`]) which are captured with the events.
pub trait Layer: Send {
    /// Processes an event.
    ///
    /// To pass the event on, invoke [`Next::emit`].
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error>;
}

/// An event that passes through the [`Layer`]s.
///
/// Events emitted with
/// [`emit_borrowed`](crate::de::DeserializeDriver::emit_borrowed) borrow
/// from the data that is deserialized for `'de`, other events are only
/// valid for `'e`.  Layers that pass on the event they received retain
/// this.
pub struct LayerEvent<'e, 'de>(Repr<'e, 'de>);

enum Repr<'e, 'de> {
    Borrowed(Event<'de>),
    Transient(Event<'e>),
}

impl<'e, 'de> LayerEvent<'e, 'de> {
    /// Creates an event that is only valid for `'e`.
    #[inline(always)]
    pub fn new(event: Event<'e>) -> LayerEvent<'e, 'de> {
        LayerEvent(Repr::Transient(event))
    }

    /// Creates an event that borrows from the data being deserialized.
    #[inline(always)]
    pub fn borrowed(event: Event<'de>) -> LayerEvent<'e, 'de> {
        LayerEvent(Repr::Borrowed(event))
    }

    /// Returns the event.
    #[inline(always)]
    pub fn event(&self) -> &Event<'_> {
        match self.0 {
            Repr::Borrowed(ref event) => event,
            Repr::Transient(ref event) => event,
        }
    }

    /// Returns `true` if the event borrows from the data being deserialized.
    pub fn is_borrowed(&self) -> bool {
        matches!(self.0, Repr::Borrowed(_))
    }
}

/// Passes events on to the next [`Layer`].
///
/// The last layer passes the events on to the sinks.
pub struct Next<'n, 'de> {
    layers: &'n mut [Box<dyn Layer>],
    core: &'n mut DriverCore<'de>,
}

impl<'n, 'de> Next<'n, 'de> {
    #[inline(always)]
    pub(crate) fn new(layers: &'n mut [Box<dyn Layer>], core: &'n mut DriverCore<'de>) -> Self {
        Next { layers, core }
    }

    /// Returns the state.
    pub fn state(&self) -> &State {
        &self.core.state
    }

    /// Returns the state mutably.
    pub fn state_mut(&mut self) -> &mut State {
        &mut self.core.state
    }

    /// Passes an event on.
    ///
    /// This can be invoked any number of times per event.  Events that were
    /// passed on get the same input range (see
    /// [`State::input_range`]) and event data (see [`State::event`]),
    /// unless the layer changes them in the state.
    pub fn emit(&mut self, event: LayerEvent<'_, 'de>) -> Result<(), Error> {
        self.core.update_position(event.event());
        match self.layers.split_first_mut() {
            Some((layer, rest)) => layer.event(event, &mut Next::new(rest, self.core)),
            None => match event.0 {
                Repr::Borrowed(event) => self.core.dispatch_borrowed(event),
                Repr::Transient(event) => self.core.dispatch(event),
            },
        }
    }
}

/// A layer that limits the size of the deserialized data.
///
/// Deser does not use the stack to process nested data, so deeply nested
/// input cannot overflow the stack during deserialization.  Still it can
/// be useful to limit the size of untrusted input, as the values that are
/// deserialized might be processed recursively later, or to limit the
/// memory used.  All limits are off by default.
///
/// ```
/// use deser::de::{DeserializeDriver, Limits};
/// use deser::Event;
///
/// let mut out = None::<Vec<Vec<u32>>>;
/// let mut driver = DeserializeDriver::new(&mut out);
/// driver.push_layer(Limits::new().max_depth(1));
/// driver.emit(Event::seq_start()).unwrap();
/// let err = driver.emit(Event::seq_start()).unwrap_err();
/// assert_eq!(err.to_string(), "Unexpected: recursion limit exceeded");
/// ```
///
/// Formats typically accept a limits layer when deserializing with
/// [`Deserializer::deserialize_with`](crate::de::Deserializer::deserialize_with).
#[derive(Debug, Clone, Default)]
pub struct Limits {
    max_depth: Option<usize>,
    max_events: Option<usize>,
    max_items: Option<usize>,
    max_len: Option<usize>,
    events: usize,
    // the number of items of the open containers if items are limited
    items: Vec<(bool, usize)>,
}

impl Limits {
    /// Creates a layer without limits.
    pub const fn new() -> Limits {
        Limits {
            max_depth: None,
            max_events: None,
            max_items: None,
            max_len: None,
            events: 0,
            items: Vec::new(),
        }
    }

    /// Limits the nesting depth of maps and sequences.
    ///
    /// With a depth of 1, maps and sequences cannot contain other maps and
    /// sequences.
    pub const fn max_depth(mut self, depth: usize) -> Limits {
        self.max_depth = Some(depth);
        self
    }

    /// Limits the total number of events.
    ///
    /// Every atom and every start and end of a map or sequence counts.
    pub const fn max_events(mut self, events: usize) -> Limits {
        self.max_events = Some(events);
        self
    }

    /// Limits the number of items in a sequence and entries in a map.
    pub const fn max_items(mut self, items: usize) -> Limits {
        self.max_items = Some(items);
        self
    }

    /// Limits the length of strings and bytes (in bytes).
    pub const fn max_len(mut self, len: usize) -> Limits {
        self.max_len = Some(len);
        self
    }

    /// Accounts for an item in the current container.
    fn count_item(&mut self, is_map_key: bool) -> Result<(), Error> {
        if let (Some(max), Some((is_map, count))) = (self.max_items, self.items.last_mut())
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
    Error::new(ErrorKind::Unexpected, msg)
}

impl Layer for Limits {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        if let Some(max) = self.max_events {
            self.events += 1;
            if self.events > max {
                return Err(limit_error("too many events"));
            }
        }
        let is_map_key = next.state().is_map_key();
        match event.event() {
            Event::MapStart(_) | Event::SeqStart(_) => {
                if self
                    .max_depth
                    .is_some_and(|max| next.state().depth() >= max)
                {
                    return Err(limit_error("recursion limit exceeded"));
                }
                self.count_item(is_map_key)?;
                if self.max_items.is_some() {
                    self.items
                        .push((matches!(event.event(), Event::MapStart(_)), 0));
                }
            }
            Event::MapEnd | Event::SeqEnd => {
                self.items.pop();
            }
            Event::Atom(atom) => {
                if let Some(max) = self.max_len {
                    let len = match atom {
                        Atom::Str(s) => s.len(),
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
