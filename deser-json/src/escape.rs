//! Finds the bytes of strings that need escaping when serializing.
use crate::scan::{ONE_BYTES, block_escape, escape_mask, load_u32, load_u64};

const SPACES: u64 = ONE_BYTES * 0x20;

/// Returns the index of the first byte in `input` which needs special
/// handling within a string or the length of the input if there is none.
///
/// This is optimized for short inputs.  Inputs shorter than a word are
/// loaded into a single word with overlapping loads and the tail of longer
/// inputs is handled with an overlapping load of the last word.
#[inline]
pub fn find_escape(input: &[u8]) -> usize {
    let len = input.len();
    if len < 8 {
        let word = if len >= 4 {
            load_u32(input, 0) | (load_u32(input, len - 4) << ((len - 4) * 8))
        } else if len > 0 {
            u64::from(input[0])
                | (u64::from(input[len / 2]) << ((len / 2) * 8))
                | (u64::from(input[len - 1]) << ((len - 1) * 8))
        } else {
            0
        };
        // the bytes after the input are filled with spaces which do not
        // need escaping.
        let padding = u64::MAX << (len * 8);
        let masked = escape_mask((word & !padding) | (SPACES & padding));
        return if masked != 0 {
            masked.trailing_zeros() as usize / 8
        } else {
            len
        };
    }

    if len < 16 {
        let masked = escape_mask(load_u64(input, 0));
        if masked != 0 {
            return masked.trailing_zeros() as usize / 8;
        }
        // the first 8 bytes do not need escaping, so the lowest flagged
        // byte of the overlapping last word is exact.
        let masked = escape_mask(load_u64(input, len - 8));
        if masked != 0 {
            return len - 8 + masked.trailing_zeros() as usize / 8;
        }
        return len;
    }

    let mut pos = 0;
    while pos + 16 <= len {
        if let Some(offset) = block_escape(input, pos) {
            return pos + offset;
        }
        pos += 16;
    }
    if pos < len {
        // the bytes before `pos` do not need escaping, so the first flagged
        // byte of the overlapping last block is at or after `pos`.
        if let Some(offset) = block_escape(input, len - 16) {
            return len - 16 + offset;
        }
    }
    len
}

#[cfg(test)]
use crate::scan::ESCAPE;

#[test]
fn test_find_escape() {
    fn naive(input: &[u8]) -> usize {
        input
            .iter()
            .position(|&c| ESCAPE[usize::from(c)])
            .unwrap_or(input.len())
    }

    let alphabet: &[u8] = b"a\"\\\x00\x1f\x20\x7f\x80\xff\xe3";
    let mut state = 0x2545f4914f6cdd1du64;
    let rounds = if cfg!(miri) { 2 } else { 500 };
    for len in 0..80 {
        for _ in 0..rounds {
            let input: Vec<u8> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    if state.is_multiple_of(16) {
                        alphabet[(state >> 8) as usize % alphabet.len()]
                    } else {
                        b'x'
                    }
                })
                .collect();
            assert_eq!(find_escape(&input), naive(&input), "{:?}", input);
        }
    }
}
