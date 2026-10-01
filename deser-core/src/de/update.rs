//! Support for updating existing values (see
//! [`Deserialize::deserialize_update`]).
use alloc::borrow::Cow;
use core::marker::PhantomData;
use core::ptr::NonNull;

use crate::State;
use crate::de::arena::ArenaBox;
use crate::de::{Deserialize, OwnedSink, Sink, SinkHandle, is_null_atom};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;

/// Forwards all calls of a sink to an owned sink.
macro_rules! forward_to_owned {
    ($field:ident) => {
        fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.$field.get_mut().next_key(state)
        }

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            self.$field.get_mut().next_value(state)
        }

        fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.$field.get_mut().__private_key_atom(atom, state)
        }

        fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
            self.$field.get_mut().__private_value_atom(atom, state)
        }

        fn __private_borrowed_key_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            self.$field
                .get_mut()
                .__private_borrowed_key_atom(atom, state)
        }

        fn __private_borrowed_value_atom(
            &mut self,
            atom: Atom<'de>,
            state: &mut State,
        ) -> Result<(), Error> {
            self.$field
                .get_mut()
                .__private_borrowed_value_atom(atom, state)
        }

        fn value_for_key(
            &mut self,
            key: &str,
            state: &mut State,
        ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
            self.$field.get_mut().value_for_key(key, state)
        }

        fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
            self.$field.get_mut().recover(err, state)
        }

        fn expecting(&self) -> Cow<'_, str> {
            self.$field.get().expecting()
        }
    };
}

/// The part of replacing a value that depends on its type.
///
/// [`ReplaceSink`] is the same for all types (it exists once), it only
/// calls into this.
trait Replace<'de>: Send {
    /// Returns the sink of the new value.
    fn sink(&mut self) -> &mut (dyn Sink<'de> + '_);

    /// Returns the sink of the new value.
    fn sink_ref(&self) -> &(dyn Sink<'de> + '_);

    /// Replaces the value with the new value (if there is one).
    fn replace(&mut self);
}

/// Replaces a value of a type.
struct Replacer<'a, 'de, T> {
    out: &'a mut T,
    sink: OwnedSink<'de, T>,
}

impl<'a, 'de, T: Send> Replace<'de> for Replacer<'a, 'de, T> {
    fn sink(&mut self) -> &mut (dyn Sink<'de> + '_) {
        self.sink.get_mut()
    }

    fn sink_ref(&self) -> &(dyn Sink<'de> + '_) {
        self.sink.get()
    }

    fn replace(&mut self) {
        if let Some(value) = self.sink.take() {
            *self.out = value;
        }
    }
}

/// A sink that replaces a value.
///
/// The new value is deserialized into an owned sink and replaces the value
/// once it's complete.
struct ReplaceSink<'a, 'de> {
    // in the arena, the sink exists once for all types
    inner: ArenaBox<dyn Replace<'de> + 'a>,
}

/// Creates a sink handle that replaces a value with a value that is
/// deserialized with a sink the function creates.
///
/// This is the default implementation of
/// [`Deserialize::deserialize_update`].
pub(crate) fn replace_handle_with<'a, 'de, T: Send + 'a>(
    out: &'a mut T,
    make: for<'x> fn(&'x mut Option<T>, &mut State) -> SinkHandle<'x, 'de>,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    replace_with(out, OwnedSink::with(make, state), state)
}

/// Creates a sink handle that updates a value and checks it once the update
/// is complete.
///
/// `update` creates the sink that updates the value (for instance
/// [`Deserialize::deserialize_update`] of the type or an adapter).  Once
/// the update is complete, `check` is invoked with the updated value.
/// Errors it returns point at the start of the value in the input.  The
/// value is updated in place, if the check fails it's updated anyway (like
/// when an update fails, see [`Deserialize::deserialize_update`]).  This is
/// for adapters that check values to support updates:
///
/// ```
/// use deser::de::{DeserializeDriver, checked_update};
/// use deser::{Deserialize, Error, ErrorKind, Event};
///
/// #[derive(Deserialize)]
/// struct Range {
///     min: u32,
///     max: u32,
/// }
///
/// fn check(range: &Range) -> Result<(), Error> {
///     if range.min > range.max {
///         return Err(Error::new(
///             ErrorKind::Unexpected,
///             "min is larger than max",
///         ));
///     }
///     Ok(())
/// }
///
/// let mut range = Range { min: 1, max: 5 };
/// let mut driver = DeserializeDriver::from_fn(|state| {
///     checked_update(&mut range, Range::deserialize_update, check, state)
/// });
/// driver.emit(Event::map_start()).unwrap();
/// driver.emit("min").unwrap();
/// driver.emit(10u64).unwrap();
/// let err = driver.emit(Event::MapEnd).unwrap_err();
/// assert_eq!(err.message(), "min is larger than max");
/// ```
pub fn checked_update<'a, 'de, T: Send + 'a>(
    value: &'a mut T,
    update: for<'x> fn(&'x mut T, &mut State) -> SinkHandle<'x, 'de>,
    check: fn(&T) -> Result<(), Error>,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    let ptr = NonNull::from(value);
    // SAFETY: the sink borrows the value for 'a, the pointer is only used
    // again once the sink was dropped (in `finish`).
    let sink = update(unsafe { &mut *ptr.as_ptr() }, state);
    SinkHandle::arena(
        CheckedUpdateSink {
            value: ptr,
            sink: Some(sink),
            check,
            start: None,
            _marker: PhantomData,
        },
        state,
    )
}

/// The sink of [`checked_update`].
struct CheckedUpdateSink<'a, 'de, T> {
    value: NonNull<T>,
    // borrows the value, `None` once the update is complete
    sink: Option<SinkHandle<'a, 'de>>,
    check: fn(&T) -> Result<(), Error>,
    // the start of the value in the input
    start: Option<usize>,
    _marker: PhantomData<&'a mut T>,
}

// SAFETY: the sink holds a mutable reference to the value (as pointer).
unsafe impl<T: Send> Send for CheckedUpdateSink<'_, '_, T> {}

impl<'a, 'de, T> CheckedUpdateSink<'a, 'de, T> {
    fn sink(&mut self) -> &mut SinkHandle<'a, 'de> {
        self.sink.as_mut().expect("update is complete")
    }

    fn begin(&mut self, state: &State) -> &mut SinkHandle<'a, 'de> {
        if self.start.is_none() {
            self.start = state.input_range().map(|x| x.start);
        }
        self.sink()
    }
}

impl<'a, 'de, T: Send> Sink<'de> for CheckedUpdateSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.begin(state).atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.begin(state).borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.begin(state).map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.begin(state).seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink().next_value(state)
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink().__private_key_atom(atom, state)
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.sink().__private_value_atom(atom, state)
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.sink().__private_borrowed_key_atom(atom, state)
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.sink().__private_borrowed_value_atom(atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.sink().value_for_key(key, state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.sink().recover(err, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let rv = self.sink().finish(state);
        // the sink borrows the value, it's dropped before the value is used
        self.sink = None;
        rv?;
        // SAFETY: nothing borrows the value anymore
        let value = unsafe { self.value.as_ref() };
        (self.check)(value).map_err(|err| match (err.offset(), self.start) {
            (None, Some(start)) => err.with_offset(start),
            _ => err,
        })
    }

    fn expecting(&self) -> Cow<'_, str> {
        match self.sink {
            Some(ref sink) => sink.expecting(),
            None => Cow::Borrowed("compatible type"),
        }
    }
}

/// Creates a sink handle that replaces a value with the value of an owned
/// sink.
pub fn replace_with<'a, 'de, T: Send + 'a>(
    out: &'a mut T,
    sink: OwnedSink<'de, T>,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    let inner = ArenaBox::into_raw(ArenaBox::new(Replacer { out, sink }, &mut state.arena));
    // SAFETY: the pointer comes from the box
    let inner = unsafe { ArenaBox::from_raw(inner.as_ptr() as *mut (dyn Replace<'de> + 'a)) };
    SinkHandle::arena(ReplaceSink { inner }, state)
}

impl<'a, 'de> Sink<'de> for ReplaceSink<'a, 'de> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.inner.get_mut().sink().atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.inner.get_mut().sink().borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.inner.get_mut().sink().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.inner.get_mut().sink().seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.inner.get_mut().sink().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.inner.get_mut().sink().next_value(state)
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.inner.get_mut().sink().__private_key_atom(atom, state)
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.inner
            .get_mut()
            .sink()
            .__private_value_atom(atom, state)
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.inner
            .get_mut()
            .sink()
            .__private_borrowed_key_atom(atom, state)
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.inner
            .get_mut()
            .sink()
            .__private_borrowed_value_atom(atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.inner.get_mut().sink().value_for_key(key, state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.inner.get_mut().sink().recover(err, state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.inner.get().sink_ref().expecting()
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.inner.get_mut().sink().finish(state)?;
        self.inner.get_mut().replace();
        Ok(())
    }
}

/// A sink that updates the value of an `Option` which is set.
///
/// The value is moved into an owned sink which updates it.  Null clears the
/// option, everything else updates the value.  The value is moved back
/// when the update finished, or failed.
struct OptionUpdateSink<'a, 'de, T> {
    out: &'a mut Option<T>,
    sink: OwnedSink<'de, T>,
}

/// Creates a sink handle that updates an `Option`.
///
/// If the option is set, the value in it is updated, otherwise a new value
/// is deserialized.  Null clears the option.
/// The value is updated with the adapter `A`.
pub(crate) fn update_option<'a, 'de, T: Send + 'a, A: Deserialize<'de, T>>(
    out: &'a mut Option<T>,
    state: &mut State,
) -> SinkHandle<'a, 'de> {
    match out.take() {
        Some(value) => SinkHandle::arena(
            OptionUpdateSink {
                out,
                sink: OwnedSink::update(value, A::deserialize_update, state),
            },
            state,
        ),
        None => replace_handle_with(
            out,
            <Option<A> as Deserialize<'de, Option<T>>>::deserialize_into,
            state,
        ),
    }
}

impl<'a, 'de, T: Send> Sink<'de> for OptionUpdateSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        if is_null_atom(&atom) {
            // the value is dropped, the option remains empty
            drop(self.sink.take());
            return Ok(());
        }
        self.sink.get_mut().atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        if is_null_atom(&atom) {
            drop(self.sink.take());
            return Ok(());
        }
        self.sink.get_mut().borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().seq(state)
    }

    forward_to_owned!(sink);

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.get_mut().finish(state)?;
        *self.out = self.sink.take();
        Ok(())
    }
}

impl<'a, 'de, T> Drop for OptionUpdateSink<'a, 'de, T> {
    fn drop(&mut self) {
        // if the update failed, the (partially updated) value is put back
        if let Some(value) = self.sink.take() {
            *self.out = Some(value);
        }
    }
}

/// A struct that is updated field by field.
///
/// The update sinks of derived structs with flattened fields keep the sinks
/// of the flattened fields (which borrow them) for the whole update while
/// they update the other fields, and look at the whole struct once they are
/// done (to validate it).  A mutable reference to the struct cannot be used
/// for this, so the fields are borrowed through a pointer.  The derive
/// borrows every field at most once at a time and only borrows the struct
/// as a whole once the borrows of all fields ended.
#[cfg(feature = "derive")]
#[doc(hidden)]
pub struct UpdateTarget<'a, T> {
    ptr: core::ptr::NonNull<T>,
    _marker: core::marker::PhantomData<&'a mut T>,
}

// SAFETY: this is a mutable reference to `T`.
#[cfg(feature = "derive")]
unsafe impl<T: Send> Send for UpdateTarget<'_, T> {}

#[cfg(feature = "derive")]
impl<'a, T> UpdateTarget<'a, T> {
    /// Creates the target for a value.
    #[inline]
    pub fn new(value: &'a mut T) -> UpdateTarget<'a, T> {
        UpdateTarget {
            ptr: core::ptr::NonNull::from(value),
            _marker: core::marker::PhantomData,
        }
    }

    /// Returns the pointer to the value.
    ///
    /// Fields are borrowed with `&mut (*target.as_ptr()).field`, which does
    /// not borrow the other fields.
    #[inline]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }
}

/// Collections that collect the values of a repeated key one at a time.
///
/// In a multimap (see
/// [`ContainerShape::with_multimap`](crate::ContainerShape::with_multimap))
/// the fields of structs and the values of maps with these types receive
/// every value of their key (see [`Deserialize::__private_collects`]).
pub(crate) trait Collection<T>: Sized + Send {
    /// Returns an empty collection.
    fn empty() -> Self;

    /// Adds a value to the collection.
    fn add(&mut self, value: T) -> Result<(), Error>;
}

/// Where a collected value goes.
enum CollectTarget<'a, C> {
    /// The slot of a value that is deserialized, the collection is created
    /// if it's empty.
    Slot(&'a mut Option<C>),
    /// A collection that is updated.
    Value(&'a mut C),
}

/// The sink of one value of a repeated key that is added to a collection
/// once it's complete.
///
/// A value that is a sequence which the element rejects is the values of
/// the key: they are all added to the collection (like `multi[]=a` in a
/// query string for a `Vec<String>`).
struct CollectSink<'a, 'de, C, T, A> {
    target: CollectTarget<'a, C>,
    element: OwnedSink<'de, T>,
    // `true` if the value is a sequence whose items are added
    extend: bool,
    _marker: PhantomData<fn() -> A>,
}

impl<'a, 'de, C: Collection<T>, T: Send> CollectSink<'a, 'de, C, T, ()> {
    /// Adds the value of the element (if there is one).
    fn add(
        target: &mut CollectTarget<'a, C>,
        element: &mut OwnedSink<'de, T>,
    ) -> Result<(), Error> {
        // a null (for an optional element) leaves no value
        if let Some(value) = element.take() {
            match *target {
                CollectTarget::Slot(ref mut slot) => {
                    slot.get_or_insert_with(C::empty).add(value)?
                }
                CollectTarget::Value(ref mut collection) => collection.add(value)?,
            }
        }
        Ok(())
    }
}

impl<'a, 'de, C, T, A> Sink<'de> for CollectSink<'a, 'de, C, T, A>
where
    C: Collection<T>,
    T: Send,
    A: Deserialize<'de, T>,
{
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.element.get_mut().atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.element.get_mut().borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.element.get_mut().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        match self.element.get_mut().seq(state) {
            Err(err) if err.kind() == ErrorKind::Unexpected => {
                // the element that rejected the sequence is not a value
                // (the slots of optionals are set when they are created)
                self.element = OwnedSink::null(state);
                self.extend = true;
                Ok(())
            }
            rv => rv,
        }
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.element.get_mut().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        if self.extend {
            // the previous item was finished by the driver
            CollectSink::<C, T, ()>::add(&mut self.target, &mut self.element)?;
            self.element = OwnedSink::deserialize_as::<A>(state);
            return Ok(SinkHandle::to(self.element.get_mut()));
        }
        self.element.get_mut().next_value(state)
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.element.get_mut().__private_key_atom(atom, state)
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        if self.extend {
            return crate::de::atom_into_handle(self.next_value(state)?, atom, state);
        }
        self.element.get_mut().__private_value_atom(atom, state)
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.element
            .get_mut()
            .__private_borrowed_key_atom(atom, state)
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        if self.extend {
            return crate::de::borrowed_atom_into_handle(self.next_value(state)?, atom, state);
        }
        self.element
            .get_mut()
            .__private_borrowed_value_atom(atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        if self.extend {
            return Ok(None);
        }
        self.element.get_mut().value_for_key(key, state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        if self.extend {
            return Err(err);
        }
        self.element.get_mut().recover(err, state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.element.get().expecting()
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        if !self.extend {
            self.element.get_mut().finish(state)?;
        }
        CollectSink::<C, T, ()>::add(&mut self.target, &mut self.element)
    }
}

/// Creates the sink of a value that is added to the collection in a slot.
///
/// This implements [`Deserialize::__private_collect_into`] for collections.
pub(crate) fn collect_into<'a, 'de, C, T, A>(
    out: &'a mut Option<C>,
    state: &mut State,
) -> SinkHandle<'a, 'de>
where
    C: Collection<T> + 'a,
    T: Send + 'a,
    A: Deserialize<'de, T>,
{
    let element = OwnedSink::deserialize_as::<A>(state);
    // SAFETY: `A` is an adapter, the sink only holds a marker of it
    unsafe {
        SinkHandle::arena_unbounded(
            CollectSink {
                target: CollectTarget::Slot(out),
                element,
                extend: false,
                _marker: PhantomData::<fn() -> A>,
            },
            state,
        )
    }
}

/// Creates the sink of a value that is added to a collection that is
/// updated.
///
/// This implements [`Deserialize::__private_collect_update`] for
/// collections.  The first value of a key replaces the collection.
pub(crate) fn collect_update<'a, 'de, C, T, A>(
    value: &'a mut C,
    first: bool,
    state: &mut State,
) -> SinkHandle<'a, 'de>
where
    C: Collection<T> + 'a,
    T: Send + 'a,
    A: Deserialize<'de, T>,
{
    if first {
        *value = C::empty();
    }
    let element = OwnedSink::deserialize_as::<A>(state);
    // SAFETY: `A` is an adapter, the sink only holds a marker of it
    unsafe {
        SinkHandle::arena_unbounded(
            CollectSink {
                target: CollectTarget::Value(value),
                element,
                extend: false,
                _marker: PhantomData::<fn() -> A>,
            },
            state,
        )
    }
}
