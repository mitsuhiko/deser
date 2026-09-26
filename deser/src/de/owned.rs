use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

use crate::adapters::DeserializeAs;
use crate::de::{Deserialize, DeserializeDriver, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};

struct NonuniqueBox<T: ?Sized> {
    ptr: NonNull<T>,
}

// SAFETY: the box owns its value like a `Box<T>`.
unsafe impl<T: ?Sized + Send> Send for NonuniqueBox<T> {}

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
/// use deser::de::{OwnedSink, SinkHandle, Sink, Deserialize};
/// use deser::State;
///
/// struct AtomWrapper<T>(T);
///
/// impl<'de, T: Deserialize<'de>> Deserialize<'de> for AtomWrapper<T> {
///     fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
///         SinkHandle::boxed(WrapperSink {
///             out,
///             sink: OwnedSink::deserialize(),
///         })
///     }
/// }
///
/// struct WrapperSink<'a, 'de, T> {
///     out: &'a mut Option<AtomWrapper<T>>,
///     sink: OwnedSink<'de, T>,
/// }
///
/// impl<'a, 'de, T: Deserialize<'de>> Sink<'de> for WrapperSink<'a, 'de, T> {
///     fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
///         self.sink.borrow_mut().atom(atom, state)
///     }
///     fn finish(&mut self, state: &mut State) -> Result<(), Error> {
///         self.sink.borrow_mut().finish(state)?;
///         *self.out = self.sink.take().map(AtomWrapper);
///         Ok(())
///     }
/// }
/// ```
pub struct OwnedSink<'de, T> {
    // The sink borrows from the storage.  The sink is always dropped before
    // the storage is accessed (in `take`) or dropped.  The lifetime of the
    // borrow is erased (to `'de` as the handle cannot outlive that).
    storage: NonuniqueBox<Option<T>>,
    sink: ManuallyDrop<SinkHandle<'de, 'de>>,
}

impl<'de, T: Deserialize<'de>> OwnedSink<'de, T> {
    /// Creates a new owned sink for a given type.
    ///
    /// This begins the deserialization with [`Deserialize::deserialize_into`]
    /// into a slot contained within the owned sink.  To extract the final
    /// value use [`take`](Self::take).
    pub fn deserialize() -> OwnedSink<'de, T> {
        OwnedSink::with(T::deserialize_into)
    }
}

impl<'de, T> OwnedSink<'de, T> {
    /// Creates a new owned sink that deserializes with an adapter.
    ///
    /// This is like [`deserialize`](Self::deserialize) but begins the
    /// deserialization with
    /// [`DeserializeAs::deserialize_into_as`] of the adapter `A`.
    pub fn deserialize_as<A: DeserializeAs<'de, T>>() -> OwnedSink<'de, T> {
        OwnedSink::with(A::deserialize_into_as)
    }

    fn with(make: for<'x> fn(&'x mut Option<T>) -> SinkHandle<'x, 'de>) -> OwnedSink<'de, T> {
        /// Creates a reference with an unbounded lifetime.
        unsafe fn unbounded<'x, X>(ptr: *mut X) -> &'x mut X {
            unsafe { &mut *ptr }
        }

        let storage = NonuniqueBox::new(None);
        // SAFETY: the storage is heap allocated and not moved.  The sink is
        // dropped before the storage is accessed again or freed.
        let sink = unsafe {
            let slot = unbounded(storage.ptr.as_ptr());
            std::mem::transmute::<SinkHandle<'_, 'de>, SinkHandle<'de, 'de>>(make(slot))
        };
        OwnedSink {
            storage,
            sink: ManuallyDrop::new(sink),
        }
    }

    /// Immutably borrows the sink.
    #[allow(clippy::should_implement_trait)]
    pub fn borrow(&self) -> &(dyn Sink<'de> + '_) {
        &*self.sink
    }

    /// Mutably borrows the sink.
    #[allow(clippy::should_implement_trait)]
    pub fn borrow_mut(&mut self) -> &mut (dyn Sink<'de> + '_) {
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

impl<'de, T> Drop for OwnedSink<'de, T> {
    fn drop(&mut self) {
        // SAFETY: the sink is never used again and dropped before the
        // storage it borrows from.
        unsafe {
            ManuallyDrop::drop(&mut self.sink);
        }
    }
}

/// A [`DeserializeDriver`] which owns the value it deserializes.
///
/// A [`DeserializeDriver`] borrows the slot of the value it deserializes,
/// which means that it cannot be held together with the slot, for instance
/// in a struct that deserializes a value from input which arrives over
/// time.  This bundles a driver with its slot.  The driver is lent out with
/// [`with`](Self::with) and the value is taken with
/// [`finish`](Self::finish):
///
/// ```
/// use deser::de::OwnedDriver;
/// use deser::Event;
///
/// let mut driver = OwnedDriver::<Vec<u32>>::new();
/// driver.with(|driver| driver.emit(Event::seq_start())).unwrap();
/// // ... later, when more input arrived
/// driver.with(|driver| {
///     driver.emit(1u64)?;
///     driver.emit(Event::SeqEnd)
/// }).unwrap();
/// assert_eq!(driver.finish().unwrap(), [1]);
/// ```
pub struct OwnedDriver<'de, T> {
    // The driver borrows from the storage.  It's dropped before the
    // storage is accessed (in `finish`) or dropped.  The lifetime of the
    // borrow is erased (to `'de` as the driver cannot outlive that).
    driver: ManuallyDrop<DeserializeDriver<'de, 'de>>,
    storage: NonuniqueBox<Option<T>>,
}

impl<'de, T: Deserialize<'de>> OwnedDriver<'de, T> {
    /// Creates a driver for a value.
    pub fn new() -> OwnedDriver<'de, T> {
        let storage = NonuniqueBox::new(None);
        // SAFETY: the storage is heap allocated and not moved.  The driver
        // is dropped before the storage is accessed again or freed.
        let driver = unsafe {
            let slot = &mut *storage.ptr.as_ptr();
            std::mem::transmute::<DeserializeDriver<'_, 'de>, DeserializeDriver<'de, 'de>>(
                DeserializeDriver::new(slot),
            )
        };
        OwnedDriver {
            driver: ManuallyDrop::new(driver),
            storage,
        }
    }
}

impl<'de, T: Deserialize<'de>> Default for OwnedDriver<'de, T> {
    fn default() -> OwnedDriver<'de, T> {
        OwnedDriver::new()
    }
}

impl<'de, T> OwnedDriver<'de, T> {
    /// Invokes a function with the driver.
    ///
    /// The function has to accept a driver of any lifetime which ensures
    /// that it cannot keep the driver or replace it.
    pub fn with<R, F>(&mut self, f: F) -> R
    where
        F: for<'a> FnOnce(&mut DeserializeDriver<'a, 'de>) -> R,
    {
        f(&mut self.driver)
    }

    /// Returns a reference to the driver.
    pub fn driver(&self) -> &DeserializeDriver<'_, 'de> {
        &self.driver
    }

    /// Finishes the deserialization and returns the value.
    ///
    /// Fails with [`ErrorKind::EndOfFile`] if the value is incomplete.
    pub fn finish(self) -> Result<T, Error> {
        let mut this = ManuallyDrop::new(self);
        // SAFETY: the driver is dropped before the storage it borrows from
        // is accessed.  The storage is moved out of the forgotten value
        // exactly once.
        let mut storage = unsafe {
            ManuallyDrop::drop(&mut this.driver);
            std::ptr::read(&this.storage)
        };
        storage
            .take()
            .ok_or_else(|| Error::new(ErrorKind::EndOfFile, "unexpected end of input"))
    }
}

impl<'de, T> Drop for OwnedDriver<'de, T> {
    fn drop(&mut self) {
        // SAFETY: the driver is never used again and dropped before the
        // storage it borrows from.
        unsafe {
            ManuallyDrop::drop(&mut self.driver);
        }
    }
}
