use crate::State;
use crate::error::Error;
use crate::event::Event;
use crate::ser::Serialize;

/// The function that receives the events of a [`SerializeDriver`](crate::ser::SerializeDriver).
pub(crate) type EventFn<'f> =
    dyn FnMut(Event<'_>, &dyn Serialize, &mut State) -> Result<(), Error> + 'f;

/// A layer between the serialization and a format.
///
/// Layers are added to a [`SerializeDriver`](crate::ser::SerializeDriver)
/// with [`push_layer`](crate::ser::SerializeDriver::push_layer) and see
/// every event produced by the serialized values before the format receives
/// it.  A layer receives the event together with a [`Next`] which passes
/// events on to the next layer (or the format).  This way a layer can
/// observe events and track information in the [`State`] (which the
/// [`Serialize`](crate::ser::Serialize) implementations of the values that
/// follow can access), reject, change or drop events or emit additional
/// events.
///
/// Layers are only applied by [`drive`](crate::ser::SerializeDriver::drive).
///
/// ```
/// use deser::ser::{Layer, Next, SerializeDriver};
/// use deser::{Atom, Error, Event};
///
/// /// Upper cases all map keys.
/// struct UppercaseKeys;
///
/// impl Layer for UppercaseKeys {
///     fn event(&mut self, event: Event<'_>, next: &mut Next<'_>) -> Result<(), Error> {
///         match event {
///             Event::Atom(Atom::Str(key)) if next.state().is_map_key() => {
///                 next.emit(Event::from(key.to_uppercase()))
///             }
///             event => next.emit(event),
///         }
///     }
/// }
///
/// let mut map = std::collections::BTreeMap::new();
/// map.insert("key", "value");
/// let mut events = Vec::new();
/// let mut driver = SerializeDriver::new(&map);
/// driver.push_layer(UppercaseKeys);
/// driver.drive(|event, _| {
///     events.push(event.to_static());
///     Ok(())
/// }).unwrap();
/// assert_eq!(events.len(), 4);
/// assert_eq!(events[1], "KEY".into());
/// ```
///
/// # Changing the Events
///
/// Layers that emit events other than the one they received should take
/// care of the [`State`]: the next layers and the format see the state as
/// it is when the event is emitted.  For instance a layer which delays an
/// event emits it with the [event data](State::event) of the event that is
/// current when it's emitted.  Map keys that are emitted at another time
/// have to be emitted with [`Next::emit_key`] so that they are recognized
/// as map keys (see [`State::is_map_key`]).
///
/// All events a layer emits are passed on with the value of the event the
/// layer received (see [`Next::value`]), so that formats that
/// [describe](crate::ser::Describe) values see the description of the
/// original value.
pub trait Layer {
    /// Processes an event.
    ///
    /// To pass the event on, invoke [`Next::emit`].
    fn event(&mut self, event: Event<'_>, next: &mut Next<'_>) -> Result<(), Error>;
}

/// Passes events on to the next [`Layer`].
///
/// The last layer passes the events on to the format.
pub struct Next<'n> {
    layers: &'n mut [Box<dyn Layer>],
    state: &'n mut State,
    f: &'n mut EventFn<'n>,
    value: &'n dyn Serialize,
}

impl<'n> Next<'n> {
    #[inline(always)]
    pub(crate) fn new(
        layers: &'n mut [Box<dyn Layer>],
        state: &'n mut State,
        f: &'n mut EventFn<'n>,
        value: &'n dyn Serialize,
    ) -> Next<'n> {
        Next {
            layers,
            state,
            f,
            value,
        }
    }

    /// Returns the value of the current event.
    ///
    /// This is only useful to [describe](crate::ser::Describe) the value.
    /// If the driver does not pass on values (see
    /// [`drive_described`](crate::ser::SerializeDriver::drive_described)),
    /// this is a value that describes nothing.
    pub fn value(&self) -> &dyn Serialize {
        self.value
    }

    /// Returns the state.
    pub fn state(&self) -> &State {
        self.state
    }

    /// Returns the state mutably.
    pub fn state_mut(&mut self) -> &mut State {
        self.state
    }

    /// Passes an event on as map key.
    ///
    /// This is like [`emit`](Self::emit) but the event is passed on with
    /// [`State::is_map_key`] set.  This is useful for layers which hold back
    /// map keys and emit them later, when the state already describes the
    /// value.
    pub fn emit_key(&mut self, event: Event<'_>) -> Result<(), Error> {
        let was_key = std::mem::replace(&mut self.state.is_map_key, true);
        let rv = self.emit(event);
        self.state.is_map_key = was_key;
        rv
    }

    /// Passes an event on.
    ///
    /// This can be invoked any number of times per event.
    pub fn emit(&mut self, event: Event<'_>) -> Result<(), Error> {
        match self.layers.split_first_mut() {
            Some((layer, rest)) => layer.event(
                event,
                &mut Next {
                    layers: rest,
                    state: self.state,
                    f: self.f,
                    value: self.value,
                },
            ),
            None => (self.f)(event, self.value, self.state),
        }
    }
}
