//! A byte buffer optimized for many small writes.

/// An output buffer for UTF-8 text.
///
/// The buffer only ever contains valid UTF-8 as long as it's only written
/// with strings (or string slices split at character boundaries) and ASCII
/// bytes.
pub struct Buffer {
    bytes: Vec<u8>,
}

impl Buffer {
    /// Creates a new buffer with a given capacity.
    pub fn with_capacity(capacity: usize) -> Buffer {
        Buffer {
            bytes: Vec::with_capacity(capacity),
        }
    }

    /// Ensures that `additional` bytes can be written without reallocating.
    #[inline(always)]
    pub fn reserve(&mut self, additional: usize) {
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
    pub fn push(&mut self, byte: u8) {
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
    pub unsafe fn push_unchecked(&mut self, byte: u8) {
        unsafe {
            let len = self.bytes.len();
            self.bytes.as_mut_ptr().add(len).write(byte);
            self.bytes.set_len(len + 1);
        }
    }

    /// Writes a string.
    #[inline(always)]
    pub fn push_str(&mut self, s: &str) {
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
    pub unsafe fn push_str_unchecked(&mut self, s: &str) {
        unsafe {
            let len = self.bytes.len();
            copy_small(s.as_ptr(), self.bytes.as_mut_ptr().add(len), s.len());
            self.bytes.set_len(len + s.len());
        }
    }

    /// Converts the buffer into a string.
    pub fn into_string(self) -> String {
        // SAFETY: the buffer only contains valid UTF-8, see above.
        unsafe { String::from_utf8_unchecked(self.bytes) }
    }
}

/// Copies bytes between non overlapping regions.
///
/// Short copies are common when writing text formats and are performed
/// inline with (possibly overlapping) word sized loads and stores rather
/// than calling into `memcpy`.
///
/// # Safety
///
/// Same requirements as `std::ptr::copy_nonoverlapping`.
#[inline(always)]
unsafe fn copy_small(src: *const u8, dst: *mut u8, len: usize) {
    unsafe {
        use std::ptr::{read_unaligned as read, write_unaligned as write};
        if len >= 16 {
            if len <= 32 {
                let a = read(src.cast::<u128>());
                let b = read(src.add(len - 16).cast::<u128>());
                write(dst.cast::<u128>(), a);
                write(dst.add(len - 16).cast::<u128>(), b);
            } else {
                std::ptr::copy_nonoverlapping(src, dst, len);
            }
        } else if len >= 8 {
            let a = read(src.cast::<u64>());
            let b = read(src.add(len - 8).cast::<u64>());
            write(dst.cast::<u64>(), a);
            write(dst.add(len - 8).cast::<u64>(), b);
        } else if len >= 4 {
            let a = read(src.cast::<u32>());
            let b = read(src.add(len - 4).cast::<u32>());
            write(dst.cast::<u32>(), a);
            write(dst.add(len - 4).cast::<u32>(), b);
        } else if len > 0 {
            let a = *src;
            let b = *src.add(len / 2);
            let c = *src.add(len - 1);
            *dst = a;
            *dst.add(len / 2) = b;
            *dst.add(len - 1) = c;
        }
    }
}

#[test]
fn test_copy_small() {
    let source: Vec<u8> = (0..100u8).collect();
    for start in 0..4 {
        for len in 0..(100 - start) {
            let mut buffer = Buffer::with_capacity(0);
            buffer.push(b'x');
            let s = std::str::from_utf8(&source[start..start + len]).unwrap();
            buffer.push_str(s);
            buffer.push(b'y');
            let out = buffer.into_string();
            assert_eq!(&out[1..out.len() - 1], s);
            assert!(out.starts_with('x') && out.ends_with('y'));
        }
    }
}
