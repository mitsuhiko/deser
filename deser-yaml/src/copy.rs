//! Copies of short bytes that do not call into `memcpy`.

/// Appends bytes to a vector.
///
/// Most writes of the serializer are short, these are performed inline rather
/// than by calling into `memcpy`.
#[inline(always)]
pub(crate) fn extend(out: &mut Vec<u8>, bytes: &[u8]) {
    out.reserve(bytes.len());
    let len = out.len();
    // SAFETY: the capacity was reserved above and the regions cannot overlap
    // as the vector is borrowed mutably.
    unsafe {
        copy_small(bytes.as_ptr(), out.as_mut_ptr().add(len), bytes.len());
        out.set_len(len + bytes.len());
    }
}

/// Appends a string to a string, see [`extend`].
#[inline(always)]
pub(crate) fn push_str(out: &mut String, s: &str) {
    // SAFETY: only a complete string is appended
    extend(unsafe { out.as_mut_vec() }, s.as_bytes());
}

/// Copies bytes between non overlapping regions.
///
/// Short copies are performed inline with (possibly overlapping) word
/// sized loads and stores rather than calling into `memcpy`.
///
/// # Safety
///
/// Same requirements as `std::ptr::copy_nonoverlapping`.
#[inline(always)]
pub(crate) unsafe fn copy_small(src: *const u8, dst: *mut u8, len: usize) {
    unsafe {
        use core::ptr::{read_unaligned as read, write_unaligned as write};
        if len >= 16 {
            if len <= 32 {
                let a = read(src.cast::<u128>());
                let b = read(src.add(len - 16).cast::<u128>());
                write(dst.cast::<u128>(), a);
                write(dst.add(len - 16).cast::<u128>(), b);
            } else {
                core::ptr::copy_nonoverlapping(src, dst, len);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extend() {
        let source: Vec<u8> = (0..100u8).collect();
        for start in 0..4 {
            for len in 0..(100 - start) {
                let mut out = vec![b'x'];
                extend(&mut out, &source[start..start + len]);
                out.push(b'y');
                assert_eq!(&out[1..out.len() - 1], &source[start..start + len]);
                assert_eq!((out[0], out[out.len() - 1]), (b'x', b'y'));
            }
        }
        let mut out = String::from("x");
        push_str(&mut out, "äöü");
        assert_eq!(out, "xäöü");
    }
}
