//! Owned values of serializations (emitters and forwarded values).
use alloc::boxed::Box;
use core::fmt;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;

use crate::State;
use crate::arena::ArenaBox;

/// An owned value of a serialization, like a `Box`.
///
/// The value is either in the arena of the state (which is how
/// [`Chunk::seq`](crate::ser::Chunk::seq), [`Chunk::map`](crate::ser::Chunk::map),
/// [`Chunk::structure`](crate::ser::Chunk::structure) and
/// [`SerializeHandle::arena`](crate::ser::SerializeHandle::arena) allocate
/// it) or on the heap (`Box::new(value).into()`).  The arena belongs to the
/// state, the emitters of the open containers are on top of each other in
/// it, allocating one bumps a pointer and the space is reused once it's
/// dropped.  A value in the arena can be kept after the serialization, the
/// chunk of the arena it's in is then freed when it's dropped (the rest of
/// the arena right away).  A value that is meant to be kept should rather
/// be on the heap.
pub struct Boxed<T: ?Sized> {
    ptr: NonNull<T>,
    in_arena: bool,
    _marker: PhantomData<T>,
}

// SAFETY: the box owns the value like a `Box`
unsafe impl<T: ?Sized + Send> Send for Boxed<T> {}
unsafe impl<T: ?Sized + Sync> Sync for Boxed<T> {}

impl<T: ?Sized> Boxed<T> {
    /// Creates a box from a value in the arena.
    #[inline(always)]
    pub(crate) fn from_arena(value: ArenaBox<T>) -> Boxed<T> {
        Boxed {
            ptr: ArenaBox::into_raw(value),
            in_arena: true,
            _marker: PhantomData,
        }
    }

    /// Takes the pointer out of the box, the flag is `true` if the value is
    /// in an arena.
    #[inline(always)]
    pub(crate) fn into_raw(this: Boxed<T>) -> (NonNull<T>, bool) {
        let rv = (this.ptr, this.in_arena);
        core::mem::forget(this);
        rv
    }

    /// Creates a box from a pointer of [`into_raw`](Self::into_raw).
    ///
    /// # Safety
    ///
    /// The pointer and flag must come from `into_raw` and the box must
    /// only be created once.
    #[inline(always)]
    pub(crate) unsafe fn from_raw(ptr: NonNull<T>, in_arena: bool) -> Boxed<T> {
        Boxed {
            ptr,
            in_arena,
            _marker: PhantomData,
        }
    }

    /// Drops the value, a value in the arena of the state is popped right
    /// away if it's on the top (see [`SinkHandle::arena`](crate::de::SinkHandle::arena)).
    #[inline(always)]
    pub(crate) fn release(this: Boxed<T>, state: &mut State) {
        let (ptr, in_arena) = Boxed::into_raw(this);
        // SAFETY: the pointer comes from a box of the kind of the flag
        unsafe {
            if in_arena {
                ArenaBox::release_in(ArenaBox::from_raw(ptr.as_ptr()), &mut state.arena)
            } else {
                drop(Box::from_raw(ptr.as_ptr()))
            }
        }
    }
}

impl<T> Boxed<T> {
    /// Moves a value into the arena of the state.
    #[inline(always)]
    pub(crate) fn arena(value: T, state: &mut State) -> Boxed<T> {
        Boxed::from_arena(ArenaBox::new(value, &mut state.arena))
    }
}

impl<T: ?Sized> From<Box<T>> for Boxed<T> {
    /// Moves a value on the heap into the box.
    fn from(value: Box<T>) -> Boxed<T> {
        Boxed {
            // SAFETY: the pointer of a box is not null
            ptr: unsafe { NonNull::new_unchecked(Box::into_raw(value)) },
            in_arena: false,
            _marker: PhantomData,
        }
    }
}

impl<T: ?Sized> Deref for Boxed<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        // SAFETY: the value is valid while the box exists
        unsafe { self.ptr.as_ref() }
    }
}

impl<T: ?Sized> DerefMut for Boxed<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: the value is valid while the box exists
        unsafe { self.ptr.as_mut() }
    }
}

impl<T: ?Sized> Drop for Boxed<T> {
    fn drop(&mut self) {
        // SAFETY: the pointer comes from a box of the kind of the flag
        unsafe {
            if self.in_arena {
                drop(ArenaBox::from_raw(self.ptr.as_ptr()))
            } else {
                drop(Box::from_raw(self.ptr.as_ptr()))
            }
        }
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for Boxed<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

/// Converts a box of a sized value into a box of a trait object.
///
/// The trait object is created by `cast`, which is `|x| x as *mut dyn Trait`.
#[inline(always)]
pub(crate) fn unsize<T, U: ?Sized>(value: Boxed<T>, cast: fn(*mut T) -> *mut U) -> Boxed<U> {
    let (ptr, in_arena) = Boxed::into_raw(value);
    // SAFETY: the cast only changes the type of the pointer
    unsafe { Boxed::from_raw(NonNull::new_unchecked(cast(ptr.as_ptr())), in_arena) }
}
