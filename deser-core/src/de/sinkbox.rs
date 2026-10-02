//! Storage for sinks.
//!
//! Sinks are allocated in the arena of the state (see
//! [`arena`](crate::arena)) unless they are created with
//! [`SinkHandle::heap`](crate::de::SinkHandle::heap), which allocates them
//! from the global allocator.
use alloc::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use alloc::boxed::Box;
use core::marker::PhantomData;
use core::ptr::{self, NonNull};

#[cfg(feature = "derive")]
use crate::arena::Release;
use crate::arena::{Arena, ArenaBox};
use crate::de::Sink;
#[cfg(feature = "derive")]
use crate::de::fields::{StructFields, StructInfo, StructSink};

/// A sink in an arena.
pub(crate) type ArenaSink<'a, 'de> = ArenaBox<dyn Sink<'de> + 'a>;

/// Moves a sink into an arena.
#[inline(always)]
pub(crate) fn arena_sink<'a, 'de, S: Sink<'de> + 'a>(
    sink: S,
    arena: &mut Arena,
) -> ArenaSink<'a, 'de> {
    let sink = ArenaBox::into_raw(ArenaBox::new(sink, arena));
    // SAFETY: the pointer comes from the box
    unsafe { ArenaBox::from_raw(sink.as_ptr() as *mut (dyn Sink<'de> + 'a)) }
}

/// Moves a sink into an arena without requiring it to outlive `'a`.
///
/// # Safety
///
/// See [`SinkHandle::arena_unbounded`](super::SinkHandle::arena_unbounded).
#[inline(always)]
pub(crate) unsafe fn arena_sink_unbounded<'a, 'de, S: Sink<'de>>(
    sink: S,
    arena: &mut Arena,
) -> ArenaSink<'a, 'de> {
    let sink = ArenaBox::into_raw(ArenaBox::new(sink, arena));
    // the sink outlives this function (like every type parameter)
    let sink: *mut (dyn Sink<'de> + '_) = sink.as_ptr();
    // SAFETY: the pointer comes from the box, the caller guarantees that
    // the sink can be used for 'a.
    unsafe {
        ArenaBox::from_raw(core::mem::transmute::<
            *mut (dyn Sink<'de> + '_),
            *mut (dyn Sink<'de> + 'a),
        >(sink))
    }
}

/// An owned sink on the heap.
///
/// This behaves like a `Box<dyn Sink>`.  As it's based on a raw pointer,
/// it can be moved while the sink is borrowed.
pub(crate) struct HeapSink<'a, 'de> {
    ptr: NonNull<dyn Sink<'de> + 'a>,
    _marker: PhantomData<Box<dyn Sink<'de> + 'a>>,
}

// SAFETY: the box owns the sink like a `Box<dyn Sink>` which is `Send` as
// sinks are `Send`.
unsafe impl Send for HeapSink<'_, '_> {}

impl<'a, 'de> HeapSink<'a, 'de> {
    /// Moves a sink to the heap.
    #[inline]
    pub(super) fn new<S: Sink<'de> + 'a>(value: S) -> HeapSink<'a, 'de> {
        let layout = Layout::new::<S>();
        let raw: *mut S = if layout.size() == 0 {
            NonNull::<S>::dangling().as_ptr()
        } else {
            // SAFETY: the layout is non zero sized
            match NonNull::new(unsafe { alloc(layout) }) {
                Some(block) => block.as_ptr().cast(),
                None => handle_alloc_error(layout),
            }
        };
        // SAFETY: the block is valid for writes of `S`
        unsafe {
            raw.write(value);
            HeapSink {
                ptr: NonNull::new_unchecked(raw as *mut (dyn Sink<'de> + 'a)),
                _marker: PhantomData,
            }
        }
    }

    /// Returns a reference to the sink.
    #[inline(always)]
    pub(super) fn get(&self) -> &(dyn Sink<'de> + 'a) {
        // SAFETY: the sink is valid while the box exists
        unsafe { self.ptr.as_ref() }
    }

    /// Returns a mutable reference to the sink.
    #[inline(always)]
    pub(super) fn get_mut(&mut self) -> &mut (dyn Sink<'de> + 'a) {
        // SAFETY: the sink is valid while the box exists
        unsafe { self.ptr.as_mut() }
    }
}

impl<'a, 'de> Drop for HeapSink<'a, 'de> {
    fn drop(&mut self) {
        // SAFETY: the sink is valid and was allocated with its own layout.
        unsafe {
            let layout = Layout::for_value(self.ptr.as_ref());
            // the block is freed even if the drop of the sink panics
            struct Free(NonNull<u8>, Layout);
            impl Drop for Free {
                fn drop(&mut self) {
                    if self.1.size() != 0 {
                        // SAFETY: see above
                        unsafe { dealloc(self.0.as_ptr(), self.1) };
                    }
                }
            }
            let _free = Free(self.ptr.cast(), layout);
            ptr::drop_in_place(self.ptr.as_ptr());
        }
    }
}

/// The sink of a derived struct together with its fields in one block of
/// an arena.
///
/// The block holds the [`StructSink`] followed by the fields, the sink
/// points to the fields.  This behaves like an [`ArenaSink`] of the struct
/// sink which owns the fields.
#[cfg(feature = "derive")]
pub(crate) struct ArenaStruct<'a, 'de> {
    // a pointer to the `StructSink` at the start of the block, it's a
    // `dyn Sink` so that the handle can use it like the one of an `ArenaSink`
    ptr: NonNull<dyn Sink<'de> + 'a>,
    _marker: PhantomData<Box<dyn Sink<'de> + 'a>>,
}

// SAFETY: see `ArenaBox`, the sink and the fields are `Send`.
#[cfg(feature = "derive")]
unsafe impl Send for ArenaStruct<'_, '_> {}

/// Returns the layout of the block of a struct sink and the offset of the
/// fields in it.
#[cfg(feature = "derive")]
#[inline(always)]
fn struct_block_layout(fields: Layout) -> (Layout, usize) {
    match Layout::new::<StructSink<'_, '_>>().extend(fields) {
        Ok((layout, offset)) => (layout.pad_to_align(), offset),
        Err(_) => panic!("struct too large"),
    }
}

#[cfg(feature = "derive")]
impl<'a, 'de: 'a> ArenaStruct<'a, 'de> {
    /// Moves the fields to the heap together with a sink for them.
    #[inline]
    pub(crate) fn new<F: StructFields<'de> + 'a>(
        fields: F,
        info: &'static StructInfo,
        raw: u64,
        collects: u64,
        arena: &mut Arena,
    ) -> ArenaStruct<'a, 'de> {
        let (layout, offset) = struct_block_layout(Layout::new::<F>());
        let block = arena.alloc(layout);
        // SAFETY: the block is valid for writes of the sink and the fields
        // at their offsets.
        unsafe {
            let raw_fields = block.as_ptr().add(offset).cast::<F>();
            raw_fields.write(fields);
            let raw_fields =
                NonNull::new_unchecked(raw_fields as *mut (dyn StructFields<'de> + 'a));
            ArenaStruct::init(block, raw_fields, info, raw, collects)
        }
    }

    /// Writes the sink into the block (before the fields).
    ///
    /// # Safety
    ///
    /// The block must be valid for writes of the sink and hold the fields.
    #[inline(never)]
    unsafe fn init(
        block: NonNull<u8>,
        fields: NonNull<dyn StructFields<'de> + 'a>,
        info: &'static StructInfo,
        raw: u64,
        collects: u64,
    ) -> ArenaStruct<'a, 'de> {
        let ptr = block.cast::<StructSink<'a, 'de>>();
        // SAFETY: the block is valid for writes of the sink, the sink owns
        // the fields
        unsafe {
            ptr.as_ptr()
                .write(StructSink::new(fields, info, raw, collects))
        };
        ArenaStruct {
            ptr: ptr as NonNull<dyn Sink<'de> + 'a>,
            _marker: PhantomData,
        }
    }

    /// Returns a reference to the sink.
    #[inline(always)]
    pub(crate) fn get(&self) -> &(dyn Sink<'de> + 'a) {
        // SAFETY: the sink is valid while the box exists
        unsafe { self.ptr.as_ref() }
    }

    /// Returns a mutable reference to the sink.
    #[inline(always)]
    pub(crate) fn get_mut(&mut self) -> &mut (dyn Sink<'de> + 'a) {
        // SAFETY: the sink is valid while the box exists
        unsafe { self.ptr.as_mut() }
    }
}

#[cfg(feature = "derive")]
impl<'a, 'de> ArenaStruct<'a, 'de> {
    /// Drops the sink and the fields, returns the size of the block.
    ///
    /// # Safety
    ///
    /// The box must not be used after.
    #[inline(always)]
    unsafe fn drop_values(&mut self) -> Release {
        // SAFETY: the sink and the fields are valid, the block was
        // allocated with the layout of both.
        unsafe {
            let sink = self.ptr.cast::<StructSink<'a, 'de>>();
            let (fields, drop_fields) = sink.as_ref().fields_ptr();
            let (layout, _) = struct_block_layout(Layout::for_value(fields.as_ref()));
            // the block is released even if a drop panics
            let release = Release(sink.cast(), layout.size());
            ptr::drop_in_place(sink.as_ptr());
            if drop_fields {
                ptr::drop_in_place(fields.as_ptr());
            }
            release
        }
    }

    /// Drops the box and returns its block to the arena right away if it's
    /// the top block (see [`Arena::pop`]).
    #[inline(always)]
    pub(crate) fn release_in(self, arena: &mut Arena) {
        let mut this = core::mem::ManuallyDrop::new(self);
        // SAFETY: the box is not used after
        unsafe {
            let release = this.drop_values();
            if arena.pop(release.0.as_ptr(), release.1) {
                core::mem::forget(release);
            }
        }
    }
}

#[cfg(feature = "derive")]
impl<'a, 'de> Drop for ArenaStruct<'a, 'de> {
    fn drop(&mut self) {
        // SAFETY: the box is not used after
        drop(unsafe { self.drop_values() });
    }
}

#[test]
fn test_sinks() {
    use crate::State;
    use crate::de::SinkHandle;
    use crate::{Atom, Error};
    use alloc::sync::Arc;
    use alloc::vec::Vec;

    struct Tracked<const N: usize>(#[allow(dead_code)] Arc<()>, [u8; N]);

    impl<'de, const N: usize> Sink<'de> for Tracked<N> {
        fn atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
            Ok(())
        }
    }

    struct Zst;
    impl Sink<'_> for Zst {}

    let rc = Arc::new(());
    let mut state = State::new();
    let mut handles = Vec::new();
    for _ in 0..3 {
        for _ in 0..40 {
            handles.push(SinkHandle::<'_, '_>::arena(
                Tracked(rc.clone(), [0u8; 1]),
                &mut state,
            ));
            handles.push(SinkHandle::heap(Tracked(rc.clone(), [0u8; 100])));
            handles.push(SinkHandle::arena(
                Tracked(rc.clone(), [0u8; 5000]),
                &mut state,
            ));
            handles.push(SinkHandle::arena(Zst, &mut state));
            handles.push(SinkHandle::heap(Zst));
        }
        assert_eq!(Arc::strong_count(&rc), 121);
        // drop in a mixed order
        let mut index = 0;
        while !handles.is_empty() {
            index = (index + 7) % handles.len();
            handles.swap_remove(index);
        }
        assert_eq!(Arc::strong_count(&rc), 1);
    }
    assert!(state.arena.is_empty());

    // handles can be dropped on other threads
    let handle = SinkHandle::<'_, '_>::arena(Tracked(rc.clone(), [0u8; 10]), &mut state);
    std::thread::spawn(move || drop(handle)).join().unwrap();
    assert_eq!(Arc::strong_count(&rc), 1);
    assert!(state.arena.is_empty());
}
