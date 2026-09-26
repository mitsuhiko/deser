//! Heap storage for sinks.
//!
//! Deserializing compound values requires a heap allocated sink for most
//! containers.  These sinks are short lived and are typically created and
//! destroyed in quick succession with the same sizes, so freed blocks are
//! cached per thread and size class and reused for the next sinks.
use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::ptr::{self, NonNull};

use crate::de::Sink;

/// The granularity of the size classes.
const CLASS_SIZE: usize = 16;
/// The alignment of all cached blocks.
const CLASS_ALIGN: usize = 16;
/// Blocks larger than this are not cached.
const MAX_CACHED_SIZE: usize = 4096;
const CLASSES: usize = MAX_CACHED_SIZE / CLASS_SIZE;
/// The maximum number of cached blocks per size class.
const MAX_PER_CLASS: u32 = 32;

/// A freed block, the link to the next free block is stored in the block.
struct FreeBlock {
    next: *mut FreeBlock,
}

#[derive(Copy, Clone)]
struct FreeList {
    head: *mut FreeBlock,
    len: u32,
}

struct Cache {
    lists: UnsafeCell<[FreeList; CLASSES]>,
}

impl Drop for Cache {
    fn drop(&mut self) {
        for (class, list) in self.lists.get_mut().iter_mut().enumerate() {
            let layout = class_layout(class);
            let mut block = list.head;
            while !block.is_null() {
                // SAFETY: all cached blocks were allocated with the layout
                // of their class.
                unsafe {
                    let next = (*block).next;
                    dealloc(block.cast(), layout);
                    block = next;
                }
            }
            list.head = ptr::null_mut();
            list.len = 0;
        }
    }
}

thread_local! {
    static CACHE: Cache = const {
        Cache {
            lists: UnsafeCell::new(
                [FreeList {
                    head: ptr::null_mut(),
                    len: 0,
                }; CLASSES],
            ),
        }
    };
}

/// Returns the size class for a layout if it can be cached.
#[inline(always)]
fn size_class(layout: Layout) -> Option<usize> {
    if layout.size() <= MAX_CACHED_SIZE && layout.align() <= CLASS_ALIGN {
        // zero sized layouts never get here
        Some((layout.size() - 1) / CLASS_SIZE)
    } else {
        None
    }
}

#[inline(always)]
fn class_layout(class: usize) -> Layout {
    // SAFETY: the size is non zero and the alignment is a power of two
    unsafe { Layout::from_size_align_unchecked((class + 1) * CLASS_SIZE, CLASS_ALIGN) }
}

/// Allocates a block for the given (non zero sized) layout.
#[inline]
fn alloc_block(layout: Layout) -> NonNull<u8> {
    let layout = match size_class(layout) {
        Some(class) => {
            // SAFETY: the cache is only accessed from this thread and no
            // references into it are held across calls.
            let cached = CACHE
                .try_with(|cache| unsafe {
                    let list = &mut (*cache.lists.get())[class];
                    let block = list.head;
                    if !block.is_null() {
                        list.head = (*block).next;
                        list.len -= 1;
                    }
                    block
                })
                .unwrap_or(ptr::null_mut());
            if let Some(block) = NonNull::new(cached) {
                return block.cast();
            }
            class_layout(class)
        }
        None => layout,
    };
    // SAFETY: the layout is non zero sized
    match NonNull::new(unsafe { alloc(layout) }) {
        Some(block) => block,
        None => handle_alloc_error(layout),
    }
}

/// Frees a block previously allocated with [`alloc_block`].
///
/// # Safety
///
/// The block must have been allocated with `alloc_block` with the same
/// layout.
#[inline]
unsafe fn free_block(block: NonNull<u8>, layout: Layout) {
    unsafe {
        let layout = match size_class(layout) {
            Some(class) => {
                let cached = CACHE
                    .try_with(|cache| {
                        let list = &mut (*cache.lists.get())[class];
                        if list.len < MAX_PER_CLASS {
                            let block = block.as_ptr().cast::<FreeBlock>();
                            (*block).next = list.head;
                            list.head = block;
                            list.len += 1;
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if cached {
                    return;
                }
                class_layout(class)
            }
            None => layout,
        };
        dealloc(block.as_ptr(), layout);
    }
}

/// An owned, heap allocated sink.
///
/// This behaves like a `Box<dyn Sink>` but uses the block cache of the
/// current thread.  As it's based on a raw pointer, it can be moved while
/// the sink is borrowed.
pub(crate) struct SinkBox<'a, 'de> {
    ptr: NonNull<dyn Sink<'de> + 'a>,
    _marker: PhantomData<Box<dyn Sink<'de> + 'a>>,
}

impl<'a, 'de> SinkBox<'a, 'de> {
    /// Moves a sink to the heap.
    #[inline]
    pub fn new<S: Sink<'de> + 'a>(value: S) -> SinkBox<'a, 'de> {
        let layout = Layout::new::<S>();
        let raw: *mut S = if layout.size() == 0 {
            NonNull::<S>::dangling().as_ptr()
        } else {
            alloc_block(layout).as_ptr().cast::<S>()
        };
        // SAFETY: the block is valid for writes of `S`
        unsafe {
            raw.write(value);
            SinkBox {
                ptr: NonNull::new_unchecked(raw as *mut (dyn Sink<'de> + 'a)),
                _marker: PhantomData,
            }
        }
    }

    /// Returns a reference to the sink.
    #[inline(always)]
    pub fn get(&self) -> &(dyn Sink<'de> + 'a) {
        // SAFETY: the sink is valid while the box exists
        unsafe { self.ptr.as_ref() }
    }

    /// Returns a mutable reference to the sink.
    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut (dyn Sink<'de> + 'a) {
        // SAFETY: the sink is valid while the box exists
        unsafe { self.ptr.as_mut() }
    }
}

impl<'a, 'de> Drop for SinkBox<'a, 'de> {
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
                        unsafe { free_block(self.0, self.1) };
                    }
                }
            }
            let _free = Free(self.ptr.cast(), layout);
            ptr::drop_in_place(self.ptr.as_ptr());
        }
    }
}

#[test]
fn test_sink_box() {
    use crate::State;
    use crate::de::SinkHandle;
    use crate::{Atom, Error};
    use std::rc::Rc;

    struct Tracked<const N: usize>(#[allow(dead_code)] Rc<()>, [u8; N]);

    impl<'de, const N: usize> Sink<'de> for Tracked<N> {
        fn atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
            Ok(())
        }
    }

    struct Zst;
    impl Sink<'_> for Zst {}

    let rc = Rc::new(());
    let mut boxes = Vec::new();
    for _ in 0..3 {
        for _ in 0..40 {
            boxes.push(SinkBox::<'_, '_>::new(Tracked(rc.clone(), [0u8; 1])));
            boxes.push(SinkBox::new(Tracked(rc.clone(), [0u8; 100])));
            boxes.push(SinkBox::new(Tracked(rc.clone(), [0u8; 5000])));
            boxes.push(SinkBox::new(Zst));
        }
        assert_eq!(Rc::strong_count(&rc), 121);
        // drop in a mixed order
        let mut index = 0;
        while !boxes.is_empty() {
            index = (index + 7) % boxes.len();
            boxes.swap_remove(index);
        }
        assert_eq!(Rc::strong_count(&rc), 1);
    }

    // boxes also work through handles and across threads
    let handle = SinkHandle::<'_, '_>::boxed(Tracked(rc.clone(), [0u8; 10]));
    drop(handle);
    std::thread::spawn(|| {
        let _a = SinkBox::<'_, '_>::new(Zst);
        let _b = SinkBox::<'_, '_>::new(Tracked(Rc::new(()), [0u8; 10]));
    })
    .join()
    .unwrap();
    assert_eq!(Rc::strong_count(&rc), 1);
}
