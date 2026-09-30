//! A byte buffer optimized for many small writes.

use alloc::string::String;
use alloc::vec::Vec;

/// An output buffer for UTF-8 text.
///
/// The buffer only ever contains valid UTF-8 as long as it's only written
/// with strings (or string slices split at character boundaries) and ASCII
/// bytes.
pub(crate) struct Buffer {
    bytes: Vec<u8>,
}

impl Buffer {
    /// Creates a new buffer with a given capacity.
    pub(crate) fn with_capacity(capacity: usize) -> Buffer {
        Buffer {
            bytes: Vec::with_capacity(capacity),
        }
    }

    /// Ensures that `additional` bytes can be written without reallocating.
    #[inline(always)]
    pub(crate) fn reserve(&mut self, additional: usize) {
        if self.bytes.capacity() - self.bytes.len() < additional {
            self.grow(additional);
        }
    }

    #[cold]
    #[inline(never)]
    fn grow(&mut self, additional: usize) {
        self.bytes.reserve(additional);
    }

    /// Writes an ASCII byte.
    #[inline(always)]
    pub(crate) fn push(&mut self, byte: u8) {
        debug_assert!(byte.is_ascii());
        self.reserve(1);
        // SAFETY: the capacity was reserved above
        unsafe { self.push_unchecked(byte) };
    }

    /// Writes an ASCII byte without checking the capacity.
    ///
    /// # Safety
    ///
    /// The capacity must have been reserved.
    #[inline(always)]
    pub(crate) unsafe fn push_unchecked(&mut self, byte: u8) {
        unsafe {
            let len = self.bytes.len();
            self.bytes.as_mut_ptr().add(len).write(byte);
            self.bytes.set_len(len + 1);
        }
    }

    /// Writes a string.
    #[inline(always)]
    pub(crate) fn push_str(&mut self, s: &str) {
        self.reserve(s.len());
        // SAFETY: the capacity was reserved above
        unsafe { self.push_str_unchecked(s) };
    }

    /// Writes a string without checking the capacity.
    ///
    /// # Safety
    ///
    /// The capacity must have been reserved.
    #[inline(always)]
    pub(crate) unsafe fn push_str_unchecked(&mut self, s: &str) {
        unsafe {
            let len = self.bytes.len();
            crate::copy::copy_small(s.as_ptr(), self.bytes.as_mut_ptr().add(len), s.len());
            self.bytes.set_len(len + s.len());
        }
    }

    /// Returns the number of bytes written.
    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns the written text.
    pub(crate) fn as_str(&self) -> &str {
        // SAFETY: the buffer only contains valid UTF-8, see above.
        unsafe { core::str::from_utf8_unchecked(&self.bytes) }
    }

    /// Shortens the buffer to the given length.
    ///
    /// The length must be at a character boundary.
    pub(crate) fn truncate(&mut self, len: usize) {
        assert!(self.as_str().is_char_boundary(len));
        self.bytes.truncate(len);
    }

    /// Creates a buffer that appends to a vector.
    ///
    /// The vector must only contain valid UTF-8.
    pub(crate) fn from_vec(bytes: Vec<u8>) -> Buffer {
        debug_assert!(core::str::from_utf8(&bytes).is_ok());
        Buffer { bytes }
    }

    /// Takes the text out of the buffer, which is empty afterwards.
    pub(crate) fn take(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.bytes)
    }

    /// Converts the buffer into a string.
    pub(crate) fn into_string(self) -> String {
        // SAFETY: the buffer only contains valid UTF-8, see above.
        unsafe { String::from_utf8_unchecked(self.bytes) }
    }
}

#[test]
fn test_copy_small() {
    let source: Vec<u8> = (0..100u8).collect();
    for start in 0..4 {
        for len in 0..(100 - start) {
            let mut buffer = Buffer::with_capacity(0);
            buffer.push(b'x');
            let s = core::str::from_utf8(&source[start..start + len]).unwrap();
            buffer.push_str(s);
            buffer.push(b'y');
            let out = buffer.into_string();
            assert_eq!(&out[1..out.len() - 1], s);
            assert!(out.starts_with('x') && out.ends_with('y'));
        }
    }
}
