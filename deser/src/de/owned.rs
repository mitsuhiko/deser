use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

use crate::de::{Deserialize, Sink, SinkHandle};

struct NonuniqueBox<T: ?Sized> {
    ptr: NonNull<T>,
}

impl<T> NonuniqueBox<T> {
    pub fn new(value: T) -> Self {
        NonuniqueBox::from(Box::new(value))
    }
}

impl<T: ?Sized> From<Box<T>> for NonuniqueBox<T> {
    fn from(boxed: Box<T>) -> Self {
        let ptr = Box::into_raw(boxed);
        let ptr = unsafe { NonNull::new_unchecked(ptr) };
        NonuniqueBox { ptr }
    }
}

impl<T: ?Sized> Deref for NonuniqueBox<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.as_ref() }
    }
}

impl<T: ?Sized> DerefMut for NonuniqueBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.ptr.as_mut() }
    }
}

impl<T: ?Sized> Drop for NonuniqueBox<T> {
    fn drop(&mut self) {
        let ptr = self.ptr.as_ptr();
        let _ = unsafe { Box::from_raw(ptr) };
    }
}

/// Utility to bundle a sink with a slot.
///
/// There are situations where one wants to be able to deserialize into
/// a slot that needs to be allocated on the heap and hold it together
/// with the sink handle.  Rust's lifetimes make this impossible so this
/// abstraction is provided to allow this.
///
/// # Example
///
/// This example demonstrates the use of an [`OwnedSink`] to implement
/// [`Deserialize`] for a newtype wrapper.  For simplicities sake only
/// atoms have been implemented here.
///
/// ```rust
/// use deser::{Atom, Error};
/// use deser::de::{OwnedSink, SinkHandle, Sink, Deserialize, DeserializerState};
///
/// struct AtomWrapper<T>(T);
///
/// impl<T: Deserialize> Deserialize for AtomWrapper<T> {
///     fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
///         SinkHandle::boxed(WrapperSink {
///             out,
///             sink: OwnedSink::deserialize(),
///         })
///     }
/// }
///
/// struct WrapperSink<'a, T> {
///     out: &'a mut Option<AtomWrapper<T>>,
///     sink: OwnedSink<T>,
/// }
///
/// impl<'a, T: Deserialize> Sink for WrapperSink<'a, T> {
///     fn atom(&mut self, atom: Atom, state: &mut DeserializerState) -> Result<(), Error> {
///         self.sink.borrow_mut().atom(atom, state)
///     }
///     fn finish(&mut self, state: &mut DeserializerState) -> Result<(), Error> {
///         self.sink.borrow_mut().finish(state)?;
///         *self.out = self.sink.take().map(AtomWrapper);
///         Ok(())
///     }
/// }
/// ```
pub struct OwnedSink<T> {
    // The sink borrows from the storage.  The sink is always dropped before
    // the storage is accessed (in `take`) or dropped.
    storage: NonuniqueBox<Option<T>>,
    sink: ManuallyDrop<SinkHandle<'static>>,
}

impl<T: Deserialize> OwnedSink<T> {
    /// Creates a new owned sink for a given type.
    ///
    /// This begins the deserialization with [`Deserialize::deserialize_into`]
    /// into a slot contained within the owned sink.  To extract the final
    /// value use [`take`](Self::take).
    pub fn deserialize() -> OwnedSink<T> {
        /// Creates a reference with an unbounded lifetime.
        unsafe fn unbounded<'x, X>(ptr: *mut X) -> &'x mut X {
            &mut *ptr
        }

        let storage = NonuniqueBox::new(None);
        // SAFETY: the storage is heap allocated and not moved.  The sink is
        // dropped before the storage is accessed again or freed.
        let sink = unsafe {
            let slot = unbounded(storage.ptr.as_ptr());
            std::mem::transmute::<SinkHandle<'_>, SinkHandle<'static>>(T::deserialize_into(slot))
        };
        OwnedSink {
            storage,
            sink: ManuallyDrop::new(sink),
        }
    }

    /// Immutably borrows the sink.
    #[allow(clippy::should_implement_trait)]
    pub fn borrow(&self) -> &(dyn Sink + '_) {
        &*self.sink
    }

    /// Mutably borrows the sink.
    #[allow(clippy::should_implement_trait)]
    pub fn borrow_mut(&mut self) -> &mut (dyn Sink + '_) {
        &mut *self.sink
    }

    /// Takes the value produced by the sink.
    ///
    /// This finishes the use of the sink.  After calling this method the
    /// sink will drop all values it receives.
    pub fn take(&mut self) -> Option<T> {
        // the sink borrows from the storage, so it needs to go first.
        *self.sink = SinkHandle::null();
        self.storage.take()
    }
}

impl<T> Drop for OwnedSink<T> {
    fn drop(&mut self) {
        // SAFETY: the sink is never used again and dropped before the
        // storage it borrows from.
        unsafe {
            ManuallyDrop::drop(&mut self.sink);
        }
    }
}
