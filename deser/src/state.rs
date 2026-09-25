//! The state shared between data formats and the types they process.
use std::fmt;

use crate::descriptors::Descriptor;
use crate::extensions::Extensions;

const STACK_CAPACITY: usize = 128;

/// Gives access to the state of an ongoing serialization or deserialization.
///
/// The state acts as a communication channel between the data format and
/// the types that are serialized or deserialized.  It is used in both
/// directions: [`Sink`](crate::de::Sink)s receive it during deserialization
/// and [`Serialize`](crate::ser::Serialize) implementations and emitters
/// receive it during serialization.  Formats get mutable access to it through
/// the drivers.
///
/// Besides some information about the current position (such as the
/// [`depth`](Self::depth)) it holds typed values that can be used by formats
/// and types to exchange information that is not part of the data model:
///
/// * Extension values ([`get`](Self::get) and [`get_mut`](Self::get_mut))
///   remain in the state until they are changed.  They are used for
///   information that spans many events such as the current path.
/// * Event data ([`event`](Self::event) and [`event_mut`](Self::event_mut))
///   is attached to a single event and detached by the drivers after the
///   event was delivered.  It is used for information about an individual
///   value, such as its source location or a tag.
///
/// Extension values have to be [`Send`] so that the state is [`Send`] too.
/// This means that the state never prevents an ongoing serialization or
/// deserialization from moving between threads.
pub struct State {
    extensions: Extensions,
    pub(crate) descriptor_stack: Vec<&'static dyn Descriptor>,
    pub(crate) is_map_key: bool,
}

impl State {
    /// Creates a new state for a driver.
    pub(crate) fn new() -> State {
        State {
            extensions: Extensions::default(),
            descriptor_stack: Vec::with_capacity(STACK_CAPACITY),
            is_map_key: false,
        }
    }

    /// Takes the state out, leaving an empty state that does not allocate.
    pub(crate) fn take(&mut self) -> State {
        std::mem::replace(
            self,
            State {
                extensions: Extensions::default(),
                descriptor_stack: Vec::new(),
                is_map_key: false,
            },
        )
    }

    #[inline]
    pub(crate) fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    #[inline]
    pub(crate) fn extensions_mut(&mut self) -> &mut Extensions {
        &mut self.extensions
    }

    /// Returns an extension value.
    ///
    /// Returns `None` if the value was never set.
    #[inline]
    pub fn get<T: fmt::Debug + Send + 'static>(&self) -> Option<&T> {
        self.extensions.get()
    }

    /// Returns a mutable extension value.
    ///
    /// If the value was never set, it's initialized with the default value.
    #[inline]
    pub fn get_mut<T: Default + fmt::Debug + Send + 'static>(&mut self) -> &mut T {
        self.extensions.get_mut()
    }

    /// Marks an extension type as replayable.
    ///
    /// When a value is internally buffered during deserialization (for
    /// instance for internally tagged enums, see
    /// [`Recording`](crate::de::Recording)) the values of replayable
    /// extensions are captured for every event and restored when the event is
    /// replayed.  This is used for information that changes from event to
    /// event but remains in the state, such as the current path.  Event data
    /// is always captured, it does not need to be marked.
    pub fn set_replayable<T: Clone + Default + fmt::Debug + Send + 'static>(&mut self) {
        self.extensions.set_replayable::<T>();
    }

    /// Returns the data of a type attached to the current event.
    ///
    /// Returns `None` if no such data is attached to the event.
    ///
    /// Event data is attached to the next event and detached by the driver
    /// after that event was delivered:
    ///
    /// * During deserialization, formats attach data with
    ///   [`DeserializeDriver::emit_with`](crate::de::DeserializeDriver::emit_with).
    ///   The sinks that receive the event (including the
    ///   [`finish`](crate::de::Sink::finish) of a container on its end event)
    ///   can access it.
    /// * During serialization, [`Serialize`](crate::ser::Serialize)
    ///   implementations and emitters attach data while they produce a
    ///   value.  The format receives it together with the first event of the
    ///   value from the [`SerializeDriver`](crate::ser::SerializeDriver).
    ///
    /// Event data is captured by a [`Recording`](crate::de::Recording) and
    /// restored when the events are replayed.
    #[inline]
    pub fn event<T: fmt::Debug + Send + 'static>(&self) -> Option<&T> {
        self.extensions.event()
    }

    /// Returns the data of a type attached to the current event mutably.
    ///
    /// If no data of this type is attached to the current event yet, the
    /// default value is attached.  See [`event`](Self::event) for more
    /// information.
    ///
    /// Detached values are retained and reused for later events.  They are
    /// reset with [`clone_from`](Clone::clone_from) from the default value,
    /// which means that types which forward `clone_from` to their fields
    /// (unlike derived implementations of [`Clone`]) reuse the memory of
    /// collections such as [`Vec`].
    ///
    /// ```
    /// # use deser::State;
    /// #[derive(Debug, Default, Clone)]
    /// struct Tags(Vec<u64>);
    ///
    /// fn push_tag(state: &mut State, tag: u64) {
    ///     state.event_mut::<Tags>().0.push(tag);
    /// }
    /// ```
    #[inline]
    pub fn event_mut<T: Default + Clone + fmt::Debug + Send + 'static>(&mut self) -> &mut T {
        self.extensions.event_mut()
    }

    /// Returns `true` if any data is attached to the current event.
    ///
    /// This is a cheap check that formats can use to skip looking up event
    /// data for the vast majority of events that have none.
    #[inline(always)]
    pub fn has_event_data(&self) -> bool {
        self.extensions.has_event_data()
    }

    /// Detaches all data from the current event.
    ///
    /// The drivers call this after the events that carry data.  Formats
    /// which attach data for every event they emit (such as source
    /// locations) do not need to detach it between events as it's replaced,
    /// but they should detach it once they are done.
    #[inline(always)]
    pub fn clear_event_data(&mut self) {
        self.extensions.clear_event_data();
    }

    /// Returns the current recursion depth.
    ///
    /// This is the number of containers (maps and sequences) that are
    /// currently open.
    pub fn depth(&self) -> usize {
        self.descriptor_stack.len()
    }

    /// Returns the topmost descriptor.
    ///
    /// This descriptor always points to a container.  During serialization
    /// the descriptor of a value itself is passed to the format explicitly.
    pub fn top_descriptor(&self) -> Option<&'static dyn Descriptor> {
        self.descriptor_stack.last().copied()
    }

    /// Returns `true` if the value currently being processed is a map key.
    ///
    /// Many formats (such as JSON) can only represent string keys.  During
    /// deserialization sinks can use this to accept a stringified
    /// representation of their value when it's used as a key.  For instance
    /// the integer sinks will parse `"42"` as a number when in key position.
    ///
    /// During serialization this is `true` while a map key (including the
    /// keys of structs) is serialized and emitted.
    pub fn is_map_key(&self) -> bool {
        self.is_map_key
    }
}

// the state must never prevent an ongoing serialization or deserialization
// from moving between threads.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<State>();
};

impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Stack<'a>(&'a [&'a dyn Descriptor]);
        struct Entry<'a>(&'a dyn Descriptor);

        impl<'a> fmt::Debug for Entry<'a> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("Layer")
                    .field("type_name", &self.0.name())
                    .field("precision", &self.0.precision())
                    .field("unordered", &self.0.unordered())
                    .finish()
            }
        }

        impl<'a> fmt::Debug for Stack<'a> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut l = f.debug_list();
                for item in self.0.iter() {
                    l.entry(&Entry(*item));
                }
                l.finish()
            }
        }

        f.debug_struct("State")
            .field("extensions", &self.extensions)
            .field("stack", &Stack(&self.descriptor_stack))
            .field("is_map_key", &self.is_map_key)
            .finish()
    }
}
