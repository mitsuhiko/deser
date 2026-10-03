use crate::State;
use crate::de::driver::DriverCore;
use crate::error::Error;
use crate::event::Event;
use alloc::boxed::Box;

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
