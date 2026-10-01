//! The arena that sinks and emitters are allocated in.
//!
//! Deserializing compound values creates a sink for most containers,
//! serializing them an emitter (see [`Boxed`](crate::ser::Boxed)).  Both
//! are created when a container starts and dropped when it ends, so they
//! live and die like the frames of a stack.  They are allocated in an arena
//! which belongs to the [`State`](crate::State) of the deserialization or
//! serialization: allocating bumps a pointer, and the space of the sinks
//! and emitters on top is reused once they are dropped.
//!
//! Every block is followed by a footer with the top of the arena before the
//! block was allocated (the footer of the block below it is right before
//! that).  Dropping a block marks it as dead in its footer, which is all
//! that has to be done without access to the arena.  Before allocating, the
//! arena pops the dead blocks on its top.  A block that is dropped out of
//! order stays until the blocks above it are dropped too.
//!
//! Chunks never move, so sinks can borrow from each other.  When the arena
//! is dropped without live blocks its chunks are freed (or parked for the
//! next deserialization).
//!
//! If blocks are still alive (a sink was kept after the deserialization it
//! was created for), the arena is orphaned: the chunks without live blocks
//! are freed right away, every other chunk counts its live blocks and is
//! freed when the last of them is dropped (also on other threads).  To find
//! their chunk, the footers of the live blocks are rewritten to point to the
//! header of the chunk (tagged with [`ORPHAN`]) as nothing pops them
//! anymore.  Dropping a block marks the footer as dead with an atomic
//! read-modify-write, which tells whether the block was orphaned.  The
//! owner of a live arena pops its blocks without atomics (see
//! [`Arena::pop`]), orphaning is only for blocks that outlive their arena.
//!
//! A block that is never dropped (a sink that is forgotten) keeps its chunk
//! alive, like a leaked `Box`.
use alloc::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use alloc::vec::Vec;
use core::marker::PhantomData;
use core::mem::{align_of, size_of};
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// The size of the first chunk (including its header).
const FIRST_CHUNK_SIZE: usize = 8 * 1024;
/// Chunks larger than this are not parked.
const MAX_PARKED_CHUNK_SIZE: usize = 1024 * 1024;
/// The alignment of the data in chunks.
const CHUNK_ALIGN: usize = 16;

/// The footer of a block: the top of the arena before the block, the lowest
/// bit is set once the block was dropped.  Once the arena is orphaned it's
/// the header of the chunk of the block, tagged with [`ORPHAN`].
type Footer = AtomicPtr<u8>;
const FOOTER_SIZE: usize = size_of::<Footer>();
const FOOTER_ALIGN: usize = align_of::<Footer>();
/// The tag of a footer whose block was dropped.
const DEAD: usize = 1;
/// The tag of a footer that points to the header of its chunk (the arena
/// was dropped while the block was alive).  Footers and chunk headers are
/// at least 4-aligned, the tags do not overlap addresses.
const ORPHAN: usize = 2;
const _: () = assert!(FOOTER_ALIGN > (ORPHAN | DEAD) && align_of::<Chunk>() > (ORPHAN | DEAD));

/// The header of a chunk, the data follows it.
struct Chunk {
    /// The chunk before this one (with the older blocks).
    prev: *mut Chunk,
    /// The chunk after this one, a spare chunk while this is the current one.
    next: *mut Chunk,
    /// The layout the chunk was allocated with.
    layout: Layout,
    /// The buffers of the arena (in the first chunk of the arena, they are
    /// parked with it).
    bufs: Buffers,
    /// The number of live blocks (and walkers) once the arena is orphaned,
    /// the chunk is freed when it reaches zero.
    live: AtomicUsize,
}

/// The buffers of the vectors that are kept for the next driver (see
/// [`Arena::take_vec`]), by [`Buffer`].
type Buffers = [RawBuf; 3];

const NO_BUFFERS: Buffers = [RawBuf::EMPTY; 3];

/// The kinds of vectors whose buffers are kept.
#[derive(Copy, Clone)]
pub(crate) enum Buffer {
    /// The sinks of the containers of a deserialization.
    SinkStack = 0,
    /// The frames of a serialization.
    SerializeStack = 1,
    /// The scratch space of a format (for instance to unescape strings).
    Scratch = 2,
}

/// Buffers larger than this are not kept (by [`Buffer`]).
///
/// The scratch space of formats holds strings with escapes, which can be
/// large (code, documents).  Without keeping it every document grows it
/// again.
const MAX_BUFFER_SIZE: [usize; 3] = [64 * 1024, 64 * 1024, 256 * 1024];

/// The buffer of a vector.
#[derive(Copy, Clone)]
struct RawBuf {
    ptr: *mut u8,
    cap: usize,
    // the size and alignment of the elements
    size: usize,
    align: usize,
}

impl RawBuf {
    const EMPTY: RawBuf = RawBuf {
        ptr: ptr::null_mut(),
        cap: 0,
        size: 0,
        align: 1,
    };

    /// Frees the buffer.
    ///
    /// # Safety
    ///
    /// The buffer must be the buffer of a vector (or empty).
    unsafe fn free(self) {
        if self.cap != 0 {
            // SAFETY: see above, a vector allocated it with this layout
            unsafe {
                dealloc(
                    self.ptr,
                    Layout::from_size_align_unchecked(self.size * self.cap, self.align),
                )
            }
        }
    }
}

/// Frees buffers.
///
/// # Safety
///
/// The buffers must not be used after.
unsafe fn free_buffers(bufs: &mut Buffers) {
    for buf in bufs.iter_mut() {
        // SAFETY: see above
        unsafe { core::mem::replace(buf, RawBuf::EMPTY).free() };
    }
}

const CHUNK_HEADER: usize = size_of::<Chunk>().next_multiple_of(CHUNK_ALIGN);

impl Chunk {
    /// Allocates a chunk with room for at least `data` bytes.
    fn alloc(size: usize, data: usize) -> NonNull<Chunk> {
        let size = size.max(CHUNK_HEADER + data);
        let Ok(layout) = Layout::from_size_align(size, CHUNK_ALIGN) else {
            panic!("sink too large");
        };
        // SAFETY: the layout is not zero sized
        let Some(chunk) = NonNull::new(unsafe { alloc(layout) }) else {
            handle_alloc_error(layout)
        };
        let chunk = chunk.cast::<Chunk>();
        // SAFETY: the chunk is valid for writes of the header
        unsafe {
            chunk.as_ptr().write(Chunk {
                prev: ptr::null_mut(),
                next: ptr::null_mut(),
                layout,
                bufs: NO_BUFFERS,
                live: AtomicUsize::new(0),
            })
        };
        chunk
    }

    /// Frees a chunk.
    ///
    /// # Safety
    ///
    /// The chunk must not hold live blocks and not be used after.
    unsafe fn free(chunk: *mut Chunk) {
        // SAFETY: see above
        unsafe {
            free_buffers(&mut (*chunk).bufs);
            dealloc(chunk.cast(), (*chunk).layout)
        }
    }

    #[inline(always)]
    fn start(chunk: *mut Chunk) -> *mut u8 {
        chunk.cast::<u8>().wrapping_add(CHUNK_HEADER)
    }

    #[inline(always)]
    fn end(chunk: *mut Chunk) -> *mut u8 {
        // SAFETY: only called for valid chunks
        chunk
            .cast::<u8>()
            .wrapping_add(unsafe { (*chunk).layout.size() })
    }
}

/// Where an owned value is allocated: in the arena of a state or on the
/// heap.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Alloc {
    Arena,
    Heap,
}

/// The arena of a state, see the module documentation.
pub(crate) struct Arena {
    /// The end of the last block (or the start of the first chunk if there
    /// are no blocks).  Null if there is no chunk yet.
    top: *mut u8,
    /// The end of the current chunk.
    end: *mut u8,
    /// The start of the first chunk, the arena is empty if `top` is here.
    base: *mut u8,
    /// The chunk `top` is in.
    chunk: *mut Chunk,
}

// SAFETY: the arena owns its chunks, they can be used and freed on any
// thread.  All access goes through `&mut Arena`.
unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

impl Default for Arena {
    fn default() -> Arena {
        Arena::new()
    }
}

impl Arena {
    /// Creates an arena, the first chunk is allocated with the first block.
    pub(crate) const fn new() -> Arena {
        Arena {
            top: ptr::null_mut(),
            end: ptr::null_mut(),
            base: ptr::null_mut(),
            chunk: ptr::null_mut(),
        }
    }

    /// Returns the buffers, they are in the first chunk.
    #[inline(always)]
    fn bufs(&mut self) -> Option<&mut Buffers> {
        if self.chunk.is_null() {
            return None;
        }
        // SAFETY: the first chunk is valid, its data starts at the base
        unsafe { Some(&mut (*self.base.wrapping_sub(CHUNK_HEADER).cast::<Chunk>()).bufs) }
    }

    /// Takes the buffer of a vector that was kept, the vector is empty.
    ///
    /// This also takes a parked chunk if the arena has none yet (the
    /// buffers are parked with it), so that a driver which is created
    /// before anything is allocated gets the buffers of the last one.
    #[inline]
    pub(crate) fn take_vec<T>(&mut self, kind: Buffer) -> Option<Vec<T>> {
        if self.chunk.is_null() {
            self.take_parked();
        }
        let buf = core::mem::replace(&mut self.bufs()?[kind as usize], RawBuf::EMPTY);
        if buf.cap == 0 {
            return None;
        }
        if buf.size == size_of::<T>() && buf.align == align_of::<T>() {
            // SAFETY: the buffer comes from a vector with elements of this
            // size and alignment
            Some(unsafe { Vec::from_raw_parts(buf.ptr.cast::<T>(), 0, buf.cap) })
        } else {
            // SAFETY: the buffer is not used after
            unsafe { buf.free() };
            None
        }
    }

    /// Keeps the buffer of a vector for the next driver.
    ///
    /// The elements of the vector are dropped.
    #[inline]
    pub(crate) fn put_vec<T>(&mut self, kind: Buffer, vec: Vec<T>) {
        let size = size_of::<T>();
        if vec.capacity() == 0
            || size == 0
            || vec.capacity() * size > MAX_BUFFER_SIZE[kind as usize]
        {
            return;
        }
        let Some(bufs) = self.bufs() else {
            // without a chunk there is nothing to keep it with
            return;
        };
        let mut vec = core::mem::ManuallyDrop::new(vec);
        vec.clear();
        let buf = RawBuf {
            ptr: vec.as_mut_ptr().cast(),
            cap: vec.capacity(),
            size,
            align: align_of::<T>(),
        };
        let old = core::mem::replace(&mut bufs[kind as usize], buf);
        // SAFETY: the old buffer is not used after
        unsafe { old.free() };
    }

    /// Takes a parked chunk as the first chunk.
    #[cold]
    fn take_parked(&mut self) {
        if let Some(chunk) = parked::take(0) {
            self.set_first_chunk(chunk);
        }
    }

    /// Makes a chunk the first chunk, the arena has none yet.
    fn set_first_chunk(&mut self, chunk: NonNull<Chunk>) {
        debug_assert!(self.chunk.is_null());
        self.chunk = chunk.as_ptr();
        self.base = Chunk::start(self.chunk);
        self.top = self.base;
        self.end = Chunk::end(self.chunk);
    }

    /// Allocates a block for a (non zero sized) layout.
    #[inline]
    pub(crate) fn alloc(&mut self, layout: Layout) -> NonNull<u8> {
        self.reclaim();
        let top = self.top;
        match place(top, self.end, layout) {
            // SAFETY: the block and its footer fit into the current chunk
            Some((block, footer)) => unsafe { self.commit(block, footer, top) },
            None => self.alloc_slow(layout),
        }
    }

    /// Writes the footer of a block and moves the top after it.
    ///
    /// # Safety
    ///
    /// The block and the footer must be in the current chunk.
    #[inline(always)]
    unsafe fn commit(&mut self, block: *mut u8, footer: *mut u8, prev_top: *mut u8) -> NonNull<u8> {
        // SAFETY: see above, the footer is aligned
        unsafe {
            footer.cast::<Footer>().write(AtomicPtr::new(prev_top));
            self.top = footer.add(FOOTER_SIZE);
            NonNull::new_unchecked(block)
        }
    }

    #[cold]
    #[inline(never)]
    fn alloc_slow(&mut self, layout: Layout) -> NonNull<u8> {
        let needed = layout.size() + layout.align() + FOOTER_SIZE;
        let prev_top = self.top;
        if self.chunk.is_null() {
            let chunk =
                parked::take(needed).unwrap_or_else(|| Chunk::alloc(FIRST_CHUNK_SIZE, needed));
            self.set_first_chunk(chunk);
            let top = self.top;
            let (block, footer) = place(top, self.end, layout).expect("chunk too small");
            // the first block of the arena
            // SAFETY: the block fits
            return unsafe { self.commit(block, footer, top) };
        }
        // SAFETY: the chunks of the arena are valid
        unsafe {
            let mut next = (*self.chunk).next;
            if !next.is_null() && place(Chunk::start(next), Chunk::end(next), layout).is_none() {
                // the spare chunk is too small
                free_chunks(next);
                (*self.chunk).next = ptr::null_mut();
                next = ptr::null_mut();
            }
            if next.is_null() {
                let size = (*self.chunk).layout.size().saturating_mul(2);
                next = Chunk::alloc(size, needed).as_ptr();
                (*next).prev = self.chunk;
                (*self.chunk).next = next;
            }
            self.chunk = next;
            self.end = Chunk::end(next);
            let start = Chunk::start(next);
            let (block, footer) = place(start, self.end, layout).expect("chunk too small");
            // popping the block returns to the top in the previous chunk
            self.commit(block, footer, prev_top)
        }
    }

    /// Pops the dead blocks on the top.
    #[inline(always)]
    fn reclaim(&mut self) {
        if self.top != self.base && is_dead(self.top) {
            self.reclaim_slow();
        }
    }

    /// Pops a block if it's the top block, returns `false` otherwise.
    ///
    /// This is how the owner of the arena releases a block: without
    /// marking it as dead and popping it on the next allocation.
    ///
    /// # Safety
    ///
    /// The block must have been allocated with this size (in any arena) and
    /// the value in it must have been dropped.
    #[inline(always)]
    pub(crate) unsafe fn pop(&mut self, block: *mut u8, size: usize) -> bool {
        // SAFETY: see above, the footer follows the block
        unsafe {
            let footer = footer_of(block, size);
            if footer.wrapping_add(FOOTER_SIZE) != self.top {
                return false;
            }
            // the block is the top block of this arena (the top is in a
            // chunk of this arena, so a block that ends there is in it)
            let prev = (*footer.cast::<Footer>()).load(Ordering::Relaxed);
            if prev >= Chunk::start(self.chunk) && prev <= self.end {
                self.top = prev;
                return true;
            }
        }
        false
    }

    #[inline(never)]
    fn reclaim_slow(&mut self) {
        while self.top != self.base {
            // SAFETY: the top is the end of a block, its footer is before it
            let prev = unsafe { footer_of_top(self.top).load(Ordering::Acquire) };
            if prev.addr() & DEAD == 0 {
                break;
            }
            self.top = prev.map_addr(|addr| addr & !DEAD);
            // the block was the first in its chunk, continue in the chunk
            // before it
            // SAFETY: the chunks of the arena are valid
            unsafe {
                while self.top < Chunk::start(self.chunk) || self.top > Chunk::end(self.chunk) {
                    self.chunk = (*self.chunk).prev;
                    self.end = Chunk::end(self.chunk);
                }
            }
        }
    }

    /// Returns `true` if the arena has no live blocks.
    #[cfg(test)]
    pub(crate) fn is_empty(&mut self) -> bool {
        self.reclaim();
        self.top == self.base
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        if self.chunk.is_null() {
            return;
        }
        self.reclaim();
        // SAFETY: the chunks of the arena are valid, the chunks after the
        // current one only hold dead blocks
        unsafe {
            let next = (*self.chunk).next;
            if !next.is_null() {
                (*self.chunk).next = ptr::null_mut();
                free_chunks(next);
            }
            if self.top != self.base {
                // blocks are still alive, their chunks must stay valid
                if let Some(bufs) = self.bufs() {
                    free_buffers(bufs);
                }
                self.orphan();
                return;
            }
            // the arena is empty, the largest chunk (the current one) is
            // parked for the next arena
            let chunk = self.chunk;
            // the buffers are in the first chunk, they are parked with the
            // chunk that is parked (before the other chunks are freed)
            let first = self.base.wrapping_sub(CHUNK_HEADER).cast::<Chunk>();
            if first != chunk {
                (*chunk).bufs = core::mem::replace(&mut (*first).bufs, NO_BUFFERS);
            }
            let prev = (*chunk).prev;
            if !prev.is_null() {
                (*chunk).prev = ptr::null_mut();
                let mut chunk = prev;
                while !chunk.is_null() {
                    let prev = (*chunk).prev;
                    Chunk::free(chunk);
                    chunk = prev;
                }
            }
            if (*chunk).layout.size() > MAX_PARKED_CHUNK_SIZE {
                Chunk::free(chunk);
            } else if let Some(chunk) = parked::park(NonNull::new_unchecked(chunk)) {
                Chunk::free(chunk.as_ptr());
            }
        }
    }
}

impl Arena {
    /// Orphans the arena (it's dropped while blocks are alive).
    ///
    /// Every chunk counts its live blocks, the footers of the live blocks
    /// point to their chunk.  The chunks are freed when their last block is
    /// dropped, the chunks without live blocks right away.
    ///
    /// # Safety
    ///
    /// The arena must not be used after.  The chunks after the current one
    /// must have been freed.
    #[cold]
    #[inline(never)]
    #[cfg(target_has_atomic = "ptr")]
    unsafe fn orphan(&mut self) {
        #[cfg(test)]
        ORPHANED.with(|orphaned| orphaned.set(orphaned.get() + 1));
        // SAFETY: the chunks of the arena are valid until their counts
        // reach zero.  This holds a reference on every chunk while it walks
        // the blocks (they could be dropped on other threads meanwhile).
        unsafe {
            let mut chunk = self.chunk;
            while !chunk.is_null() {
                (*chunk).live.store(1, Ordering::Relaxed);
                chunk = (*chunk).prev;
            }
            let mut top = self.top;
            let mut chunk = self.chunk;
            while top != self.base {
                let footer = footer_of_top(top);
                let live = &(*chunk).live;
                let orphan = chunk.cast::<u8>().map_addr(|addr| addr | ORPHAN);
                let mut prev = footer.load(Ordering::Acquire);
                while prev.addr() & DEAD == 0 {
                    // counted before the footer points to the chunk, a drop
                    // of the block decrements it right after
                    live.fetch_add(1, Ordering::Relaxed);
                    match footer.compare_exchange(prev, orphan, Ordering::AcqRel, Ordering::Acquire)
                    {
                        Ok(_) => break,
                        Err(current) => {
                            // the block was dropped meanwhile (this holds a
                            // reference, the count stays above zero)
                            live.fetch_sub(1, Ordering::Relaxed);
                            prev = current;
                        }
                    }
                }
                top = prev.map_addr(|addr| addr & !DEAD);
                // the block was the first in its chunk, continue in the
                // chunk before it
                while top < Chunk::start(chunk) || top > Chunk::end(chunk) {
                    chunk = (*chunk).prev;
                }
            }
            let mut chunk = self.chunk;
            while !chunk.is_null() {
                // the chunk can be freed by its last block after this
                let prev = (*chunk).prev;
                release_orphaned_chunk(chunk);
                chunk = prev;
            }
        }
        self.chunk = ptr::null_mut();
        self.top = ptr::null_mut();
        self.base = ptr::null_mut();
        self.end = ptr::null_mut();
    }

    /// Without atomic read-modify-writes the blocks cannot count
    /// themselves, the chunks are leaked (the blocks stay valid).
    #[cfg(not(target_has_atomic = "ptr"))]
    unsafe fn orphan(&mut self) {
        #[cfg(test)]
        ORPHANED.with(|orphaned| orphaned.set(orphaned.get() + 1));
    }
}

/// Drops a reference to an orphaned chunk, frees it if it was the last.
///
/// # Safety
///
/// The chunk must be orphaned, the caller must hold a reference on it (a
/// live block or the walker in [`Arena::orphan`]) and not use it after.
#[cfg(target_has_atomic = "ptr")]
unsafe fn release_orphaned_chunk(chunk: *mut Chunk) {
    // SAFETY: see above, the reference keeps the chunk valid
    unsafe {
        if (*chunk).live.fetch_sub(1, Ordering::Release) == 1 {
            // the drops of all blocks happen before the chunk is freed
            core::sync::atomic::fence(Ordering::Acquire);
            Chunk::free(chunk);
        }
    }
}

#[cfg(test)]
std::thread_local! {
    /// The number of arenas that were orphaned on this thread (for tests).
    pub(crate) static ORPHANED: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// Frees a chunk and the chunks after it.
///
/// # Safety
///
/// The chunks must only hold dead blocks.
unsafe fn free_chunks(mut chunk: *mut Chunk) {
    while !chunk.is_null() {
        // SAFETY: see above
        unsafe {
            let next = (*chunk).next;
            Chunk::free(chunk);
            chunk = next;
        }
    }
}

/// Places a block with its footer at the top, returns the block and the
/// footer if they fit.
#[inline(always)]
fn place(top: *mut u8, end: *mut u8, layout: Layout) -> Option<(*mut u8, *mut u8)> {
    let block_pad = top.addr().wrapping_neg() & (layout.align() - 1);
    let footer_offset = (block_pad + layout.size()).next_multiple_of(FOOTER_ALIGN);
    let new_top_offset = footer_offset + FOOTER_SIZE;
    if top.is_null() || new_top_offset > end.addr().wrapping_sub(top.addr()) {
        return None;
    }
    Some((top.wrapping_add(block_pad), top.wrapping_add(footer_offset)))
}

/// Returns the footer of the block that ends at `top`.
///
/// # Safety
///
/// `top` must be the end of a block.
#[inline(always)]
unsafe fn footer_of_top<'x>(top: *mut u8) -> &'x Footer {
    // SAFETY: see above
    unsafe { &*top.sub(FOOTER_SIZE).cast::<Footer>() }
}

/// Returns the footer of a block.
///
/// # Safety
///
/// The block must have been allocated in an arena with this size.
#[inline(always)]
unsafe fn footer_of(block: *mut u8, size: usize) -> *mut u8 {
    // SAFETY: see above, the footer follows the block (see `place`)
    unsafe {
        block
            .add(size)
            .map_addr(|addr| addr.next_multiple_of(FOOTER_ALIGN))
    }
}

/// Returns `true` if the block that ends at `top` is dead.
#[inline(always)]
fn is_dead(top: *mut u8) -> bool {
    // SAFETY: only called with the top of a non empty arena
    unsafe { footer_of_top(top).load(Ordering::Acquire).addr() & DEAD != 0 }
}

/// Marks a block as dead, its space is reused once the blocks above it are
/// dead too.  If the arena was orphaned, the chunk of the block is freed
/// when this was its last live block.
///
/// # Safety
///
/// The block must have been allocated in an arena with this size and not
/// be used after.  The value in it must have been dropped.
#[inline]
pub(crate) unsafe fn release(block: *mut u8, size: usize) {
    // SAFETY: the footer follows the block, see `place`
    let footer = unsafe { &*footer_of(block, size).cast::<Footer>() };
    #[cfg(target_has_atomic = "ptr")]
    {
        // the arena can be orphaned concurrently, which rewrites the footer
        let mut prev = footer.load(Ordering::Relaxed);
        // the release orders the drop of the value before the reuse of the
        // block (which loads with acquire) or the free of its chunk
        while let Err(current) = footer.compare_exchange_weak(
            prev,
            prev.map_addr(|addr| addr | DEAD),
            Ordering::Release,
            Ordering::Relaxed,
        ) {
            prev = current;
        }
        if prev.addr() & ORPHAN != 0 {
            // SAFETY: the footer of an orphaned block points to its chunk,
            // the block held a reference on it
            unsafe { release_orphaned_block(prev) };
        }
    }
    #[cfg(not(target_has_atomic = "ptr"))]
    {
        let prev = footer.load(Ordering::Relaxed);
        footer.store(prev.map_addr(|addr| addr | DEAD), Ordering::Release);
    }
}

/// Releases the reference of an orphaned block on its chunk.
///
/// # Safety
///
/// `footer` must be the value of the footer of an orphaned block before it
/// was marked as dead (by the caller, which does not use the block after).
#[cold]
#[inline(never)]
#[cfg(target_has_atomic = "ptr")]
unsafe fn release_orphaned_block(footer: *mut u8) {
    // the footer was read with relaxed, this synchronizes with the
    // orphaning (which initialized the count of the chunk)
    core::sync::atomic::fence(Ordering::Acquire);
    let chunk = footer.map_addr(|addr| addr & !ORPHAN).cast::<Chunk>();
    // SAFETY: see above
    unsafe { release_orphaned_chunk(chunk) };
}

/// A value in an arena, like a `Box`.
///
/// As it's based on a raw pointer (like the sinks it holds, which borrow
/// from each other) it can be moved while the value is borrowed.
pub(crate) struct ArenaBox<T: ?Sized> {
    ptr: NonNull<T>,
    _marker: PhantomData<T>,
}

// SAFETY: the box owns the value like a `Box`, the footer of the block is
// atomic.
unsafe impl<T: ?Sized + Send> Send for ArenaBox<T> {}
unsafe impl<T: ?Sized + Sync> Sync for ArenaBox<T> {}

impl<T> ArenaBox<T> {
    /// Moves a value into the arena.
    #[inline(always)]
    pub(crate) fn new(value: T, arena: &mut Arena) -> ArenaBox<T> {
        let layout = Layout::new::<T>();
        let ptr = if layout.size() == 0 {
            NonNull::dangling()
        } else {
            arena.alloc(layout).cast::<T>()
        };
        // SAFETY: the block is valid for writes of `T`
        unsafe { ptr.as_ptr().write(value) };
        ArenaBox {
            ptr,
            _marker: PhantomData,
        }
    }
}

impl<T: ?Sized> ArenaBox<T> {
    /// Creates a box from a pointer to a value in an arena.
    ///
    /// This is used to convert boxes to boxes of trait objects:
    /// `ArenaBox::from_raw(ArenaBox::into_raw(b).as_ptr() as *mut dyn Trait)`.
    ///
    /// # Safety
    ///
    /// The pointer must come from [`into_raw`](Self::into_raw) (or point to a
    /// value in a block of an arena that is owned by the box).
    #[inline(always)]
    pub(crate) unsafe fn from_raw(ptr: *mut T) -> ArenaBox<T> {
        ArenaBox {
            // SAFETY: see above
            ptr: unsafe { NonNull::new_unchecked(ptr) },
            _marker: PhantomData,
        }
    }

    /// Takes the pointer out of the box without dropping the value.
    #[inline(always)]
    pub(crate) fn into_raw(this: ArenaBox<T>) -> NonNull<T> {
        let ptr = this.ptr;
        core::mem::forget(this);
        ptr
    }

    #[cfg(test)]
    pub fn as_ptr(&self) -> NonNull<T> {
        self.ptr
    }

    /// Returns the pointer to the value (for values that are borrowed while
    /// the box is moved).
    #[inline(always)]
    pub(crate) fn ptr(&self) -> NonNull<T> {
        self.ptr
    }

    #[inline(always)]
    pub(crate) fn get(&self) -> &T {
        // SAFETY: the value is valid while the box exists
        unsafe { self.ptr.as_ref() }
    }

    #[inline(always)]
    pub(crate) fn get_mut(&mut self) -> &mut T {
        // SAFETY: the value is valid while the box exists
        unsafe { self.ptr.as_mut() }
    }
}

impl<T: ?Sized> ArenaBox<T> {
    /// Drops the box and returns its block to the arena right away if it's
    /// the top block (see [`Arena::pop`]).
    #[inline(always)]
    pub(crate) fn release_in(this: ArenaBox<T>, arena: &mut Arena) {
        let ptr = ArenaBox::into_raw(this);
        // SAFETY: the value is valid and owned by the box
        unsafe {
            let size = size_of_val(ptr.as_ref());
            // the block is released even if the drop panics
            let release = Release(ptr.cast(), size);
            ptr::drop_in_place(ptr.as_ptr());
            if size != 0 && arena.pop(ptr.as_ptr().cast(), size) {
                core::mem::forget(release);
            }
        }
    }
}

impl<T: ?Sized> Drop for ArenaBox<T> {
    fn drop(&mut self) {
        // SAFETY: the value is valid and owned by the box
        unsafe {
            let size = size_of_val(self.ptr.as_ref());
            // the block is released even if the drop panics
            let _release = Release(self.ptr.cast(), size);
            ptr::drop_in_place(self.ptr.as_ptr());
        }
    }
}

/// Releases a block when dropped.
pub(crate) struct Release(pub(crate) NonNull<u8>, pub(crate) usize);

impl Drop for Release {
    #[inline(always)]
    fn drop(&mut self) {
        if self.1 != 0 {
            // SAFETY: created for blocks of arenas whose values were dropped
            unsafe { release(self.0.as_ptr(), self.1) };
        }
    }
}

/// The chunks that are parked between deserializations.
///
/// Small documents only need a few sinks, a chunk that is allocated for
/// every one of them would make them slower than necessary.  The chunk of
/// an arena is parked when it's dropped and taken by the next arena.  That
/// is an atomic operation at the start and at the end of a deserialization.
#[cfg(target_has_atomic = "ptr")]
mod parked {
    use core::ptr::{self, NonNull};
    use core::sync::atomic::{AtomicPtr, Ordering};

    use super::Chunk;

    /// The number of parked chunks.
    const SLOTS: usize = 8;

    /// A slot on a cache line of its own so that threads which park and
    /// take chunks do not contend on other slots.
    #[repr(align(128))]
    struct Slot(AtomicPtr<Chunk>);

    static PARKED: [Slot; SLOTS] = [const { Slot(AtomicPtr::new(ptr::null_mut())) }; SLOTS];

    /// Returns the slot a thread tries first.
    ///
    /// Threads have stacks of their own, so the address of a local tells
    /// them apart (well enough) without thread locals.  A thread tends to
    /// get the chunk back that it parked.
    #[inline(always)]
    fn first_slot() -> usize {
        let local = 0u8;
        (ptr::addr_of!(local).addr() >> 16) % SLOTS
    }

    /// Takes a parked chunk that has room for `needed` bytes.
    pub(super) fn take(needed: usize) -> Option<NonNull<Chunk>> {
        let first = first_slot();
        for idx in 0..SLOTS {
            let slot = &PARKED[(first + idx) % SLOTS].0;
            if slot.load(Ordering::Relaxed).is_null() {
                continue;
            }
            let chunk = NonNull::new(slot.swap(ptr::null_mut(), Ordering::Acquire))?;
            let start = Chunk::start(chunk.as_ptr());
            let end = Chunk::end(chunk.as_ptr());
            if needed <= end.addr() - start.addr() {
                return Some(chunk);
            }
            // SAFETY: parked chunks are empty and owned by whoever took them
            unsafe { Chunk::free(chunk.as_ptr()) };
            return None;
        }
        None
    }

    /// Parks an empty chunk, it's returned if all slots are taken.
    pub(super) fn park(chunk: NonNull<Chunk>) -> Option<NonNull<Chunk>> {
        let first = first_slot();
        for idx in 0..SLOTS {
            let slot = &PARKED[(first + idx) % SLOTS].0;
            if slot
                .compare_exchange(
                    ptr::null_mut(),
                    chunk.as_ptr(),
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return None;
            }
        }
        Some(chunk)
    }
}

/// Without atomics chunks are not parked.
#[cfg(not(target_has_atomic = "ptr"))]
mod parked {
    use core::ptr::NonNull;

    use super::Chunk;

    pub(super) fn take(_needed: usize) -> Option<NonNull<Chunk>> {
        None
    }

    pub(super) fn park(chunk: NonNull<Chunk>) -> Option<NonNull<Chunk>> {
        Some(chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;
    use alloc::vec::Vec;

    #[test]
    fn test_lifo() {
        let mut arena = Arena::new();
        let a = ArenaBox::new([1u64; 4], &mut arena);
        let b = ArenaBox::new(2u8, &mut arena);
        let b_addr = b.as_ptr().addr();
        drop(b);
        // the space of `b` is reused
        let c = ArenaBox::new(3u8, &mut arena);
        assert_eq!(c.as_ptr().addr(), b_addr);
        assert_eq!(*a.get(), [1; 4]);
        drop(c);
        drop(a);
        assert!(arena.is_empty());
    }

    #[test]
    fn test_out_of_order() {
        let mut arena = Arena::new();
        let a = ArenaBox::new(1u32, &mut arena);
        let b = ArenaBox::new(2u32, &mut arena);
        let c = ArenaBox::new(3u32, &mut arena);
        drop(a);
        drop(c);
        assert!(!arena.is_empty());
        assert_eq!(*b.get(), 2);
        drop(b);
        assert!(arena.is_empty());
    }

    #[test]
    fn test_chunks() {
        let rc = Arc::new(());
        let mut arena = Arena::new();
        let mut big_boxes = Vec::new();
        for round in 0..3 {
            let mut boxes = Vec::new();
            // more than fits into one chunk, also blocks larger than a chunk
            for idx in 0..if cfg!(miri) { 300 } else { 3000 } {
                boxes.push(ArenaBox::new((rc.clone(), [idx as u8; 24]), &mut arena));
                if idx % 1000 == 0 {
                    let big = ArenaBox::new([round as u8; 20000], &mut arena);
                    assert_eq!(big.get()[19999], round as u8);
                    // dropped at the end, the blocks above it are reused
                    big_boxes.push(big);
                }
            }
            for (idx, b) in boxes.iter().enumerate() {
                assert_eq!(b.get().1[0], idx as u8);
            }
            // dropped in a mixed order
            let mut index = 0;
            while !boxes.is_empty() {
                index = (index + 7) % boxes.len();
                boxes.swap_remove(index);
            }
            assert_eq!(Arc::strong_count(&rc), 1);
        }
        assert!(!arena.is_empty());
        big_boxes.clear();
        assert!(arena.is_empty());
    }

    #[test]
    fn test_buffers() {
        // the buffers are kept in the first chunk and parked with the
        // largest chunk, also if the arena has more than one
        for chunks in [1, 3] {
            let mut arena = Arena::new();
            let boxes = (0..chunks * 300)
                .map(|idx| ArenaBox::new([idx as u64; 4], &mut arena))
                .collect::<Vec<_>>();
            let mut vec = arena.take_vec::<u64>(Buffer::SinkStack).unwrap_or_default();
            vec.extend(0..100u64);
            let cap = vec.capacity();
            arena.put_vec(Buffer::SinkStack, vec);
            // a vector of another type does not get the buffer
            assert!(arena.take_vec::<u8>(Buffer::SerializeStack).is_none());
            let vec = arena.take_vec::<u64>(Buffer::SinkStack).unwrap();
            assert!(vec.is_empty() && vec.capacity() == cap);
            arena.put_vec(Buffer::SinkStack, vec);
            drop(boxes);
            drop(arena);
            // the next arena gets them back (unless another thread took the
            // parked chunk), with the right type only
            let mut arena = Arena::new();
            if let Some(vec) = arena.take_vec::<u64>(Buffer::SinkStack) {
                assert!(vec.is_empty() && vec.capacity() == cap);
            }
        }
    }

    #[test]
    fn test_alignment() {
        #[repr(align(64))]
        struct Aligned(u8);
        let mut arena = Arena::new();
        let a = ArenaBox::new(1u8, &mut arena);
        let b = ArenaBox::new(Aligned(2), &mut arena);
        assert_eq!(b.as_ptr().addr().get() % 64, 0);
        assert_eq!(b.get().0, 2);
        let c = ArenaBox::new((), &mut arena);
        drop((a, b, c));
        assert!(arena.is_empty());
    }

    #[test]
    fn test_other_threads() {
        let mut arena = Arena::new();
        let boxes = (0..10)
            .map(|idx| ArenaBox::new(idx, &mut arena))
            .collect::<Vec<_>>();
        std::thread::spawn(move || drop(boxes)).join().unwrap();
        assert!(arena.is_empty());

        // arenas are parked and taken by other threads
        let threads = (0..4)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..if cfg!(miri) { 3 } else { 100 } {
                        let mut arena = Arena::new();
                        let a = ArenaBox::new([0u8; 100], &mut arena);
                        let b = ArenaBox::new(1u64, &mut arena);
                        drop((a, b));
                    }
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().unwrap();
        }
    }

    fn orphaned() -> usize {
        ORPHANED.with(|orphaned| orphaned.get())
    }

    #[test]
    fn test_leaked_arena() {
        // a block that outlives its arena stays valid, the chunk is freed
        // when it's dropped (miri checks that nothing leaks)
        let orphaned_before = orphaned();
        let mut arena = Arena::new();
        let dead = ArenaBox::new(1u64, &mut arena);
        let a = ArenaBox::new(alloc::string::String::from("alive"), &mut arena);
        let b = ArenaBox::new(2u64, &mut arena);
        drop(dead);
        drop(b);
        drop(arena);
        assert_eq!(orphaned(), orphaned_before + 1);
        assert_eq!(a.get(), "alive");
        drop(a);
    }

    #[test]
    fn test_orphaned_block_on_other_thread() {
        let mut arena = Arena::new();
        let a = ArenaBox::new(alloc::string::String::from("a"), &mut arena);
        let b = ArenaBox::new(alloc::string::String::from("b"), &mut arena);
        drop(arena);
        std::thread::spawn(move || {
            assert_eq!(a.get(), "a");
            drop(a);
            // the last block frees the chunk on this thread
            assert_eq!(b.get(), "b");
            drop(b);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn test_orphaned_chunks() {
        let rc = Arc::new(());
        let mut arena = Arena::new();
        // several chunks, only some of them keep live blocks
        let mut kept = Vec::new();
        let mut boxes = Vec::new();
        for idx in 0..1200 {
            let b = ArenaBox::new((rc.clone(), [idx as u8; 32]), &mut arena);
            if idx == 10 || idx == 900 || idx == 901 {
                kept.push(b);
            } else {
                boxes.push(b);
            }
        }
        // a block larger than the chunks, popped by the owner: its chunk is
        // a spare chunk after the current one
        let big = ArenaBox::new([7u8; 40000], &mut arena);
        ArenaBox::release_in(big, &mut arena);
        drop(boxes);
        let orphaned_before = orphaned();
        drop(arena);
        assert_eq!(orphaned(), orphaned_before + 1);
        assert_eq!(Arc::strong_count(&rc), 4);
        for b in &kept {
            assert_eq!(b.get().1[0], b.get().1[31]);
        }
        // the blocks of the same chunk in both orders
        kept.swap(1, 2);
        drop(kept);
        assert_eq!(Arc::strong_count(&rc), 1);
    }

    #[test]
    fn test_orphaned_in_other_arena() {
        // a block of an orphaned arena that is released into another arena
        // is not popped from it
        let mut arena = Arena::new();
        let a = ArenaBox::new(1u64, &mut arena);
        drop(arena);
        let mut arena = Arena::new();
        let b = ArenaBox::new(2u64, &mut arena);
        ArenaBox::release_in(a, &mut arena);
        assert!(!arena.is_empty());
        assert_eq!(*b.get(), 2);
        ArenaBox::release_in(b, &mut arena);
        assert!(arena.is_empty());
    }

    #[test]
    fn test_orphaned_concurrently() {
        // blocks are dropped on other threads while the arena is orphaned
        let rounds = if cfg!(miri) { 4 } else { 200 };
        for _ in 0..rounds {
            let rc = Arc::new(());
            let mut arena = Arena::new();
            let barrier = Arc::new(std::sync::Barrier::new(4));
            let mut threads = Vec::new();
            for thread in 0..3 {
                let boxes = (0..if cfg!(miri) { 20 } else { 300 })
                    .map(|idx| ArenaBox::new((rc.clone(), [(thread + idx) as u8; 24]), &mut arena))
                    .collect::<Vec<_>>();
                let barrier = barrier.clone();
                threads.push(std::thread::spawn(move || {
                    barrier.wait();
                    for (idx, b) in boxes.into_iter().enumerate() {
                        assert_eq!(b.get().1[0], (thread + idx) as u8);
                        drop(b);
                    }
                }));
            }
            barrier.wait();
            drop(arena);
            for thread in threads {
                thread.join().unwrap();
            }
            assert_eq!(Arc::strong_count(&rc), 1);
        }
    }
}
