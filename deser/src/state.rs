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
/// [`depth`](Self::depth)) it holds extension values: arbitrary typed values
/// that can be used by formats and types to exchange information that is not
/// part of the data model, for instance source locations or paths.
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
    /// replayed.  This is used for information that formats or wrappers put
    /// into the state for the current event, such as source locations or
    /// paths.
    pub fn set_replayable<T: Clone + Default + fmt::Debug + Send + 'static>(&mut self) {
        self.extensions.set_replayable::<T>();
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
