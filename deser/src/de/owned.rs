use std::mem::{transmute, ManuallyDrop};
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

use crate::de::{Deserialize, SinkHandle};

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
pub struct OwnedSink<T> {
    storage: NonuniqueBox<Option<T>>,
    sink: Option<ManuallyDrop<SinkHandle<'static>>>,
}

impl<T: Deserialize> OwnedSink<T> {
    /// Creates a new owned sink for a given type.
    ///
    /// This begins the deserialization with [`Deserialize::deserialize_into`]
    /// into a slot contained within the owned sink.  To extract the final
    /// value use [`take`](Self::take).
    pub fn deserialize() -> OwnedSink<T> {
        let mut storage = NonuniqueBox::new(None);
        unsafe {
            let ptr = transmute::<_, &mut Option<T>>(&mut *storage);
            let sink = extend_lifetime!(T::deserialize_into(ptr), SinkHandle<'_>);
            OwnedSink {
                storage,
                sink: Some(ManuallyDrop::new(extend_lifetime!(sink, SinkHandle<'_>))),
            }
        }
    }

    /// Immutably borrows from an owned sink.
    #[allow(clippy::should_implement_trait)]
    pub fn borrow(&self) -> &SinkHandle<'_> {
        unsafe { extend_lifetime!(self.sink.as_ref().unwrap(), &SinkHandle<'_>) }
    }

    /// Mutably borrows from the owned sink.
    #[allow(clippy::should_implement_trait)]
    pub fn borrow_mut(&mut self) -> &mut SinkHandle<'_> {
        unsafe { extend_lifetime!(self.sink.as_mut().unwrap(), &mut SinkHandle<'_>) }
    }

    /// Takes the value produced by the sink.
    pub fn take(&mut self) -> Option<T> {
        self.storage.take()
    }
}

impl<T> Drop for OwnedSink<T> {
    fn drop(&mut self) {
        unsafe {
            if let Some(ref mut sink) = self.sink.take() {
                ManuallyDrop::drop(sink);
            }
        }
    }
}
