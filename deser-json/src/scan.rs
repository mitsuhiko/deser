//! Shared scanning utilities for the parser and serializer.

/// Returns the index of the first byte at or after `pos` which needs special
/// handling within a string (a quote, a backslash or a control character).
///
/// This processes a word at a time and falls back to a byte-wise scan for the
/// tail of the input.  This is used by the parser where strings are typically
/// short, so this does not use SIMD which has a higher latency.
#[inline]
pub fn skip_to_escape(input: &[u8], mut pos: usize) -> usize {
    if pos >= input.len() || ESCAPE[usize::from(input[pos])] {
        return pos;
    }
    pos += 1;

    while pos + 8 <= input.len() {
        let masked = escape_mask(load_u64(input, pos));
        if masked != 0 {
            return pos + masked.trailing_zeros() as usize / 8;
        }
        pos += 8;
    }

    while pos < input.len() && !ESCAPE[usize::from(input[pos])] {
        pos += 1;
    }
    pos
}

/// Returns the offset of the first byte in the 16 bytes starting at `pos`
/// which needs escaping.
#[cfg(all(target_arch = "aarch64", target_feature = "neon", not(miri)))]
#[inline(always)]
fn block_escape(input: &[u8], pos: usize) -> Option<usize> {
    use std::arch::aarch64::*;
    let block: &[u8; 16] = input[pos..pos + 16].try_into().unwrap();
    // SAFETY: neon is available and the block is 16 bytes long
    let nibbles = unsafe {
        let chars = vld1q_u8(block.as_ptr());
        let ctrl = vcltq_u8(chars, vdupq_n_u8(0x20));
        let quote = vceqq_u8(chars, vdupq_n_u8(b'"'));
        let backslash = vceqq_u8(chars, vdupq_n_u8(b'\\'));
        let flagged = vorrq_u8(ctrl, vorrq_u8(quote, backslash));
        // narrow every byte into a nibble of a 64 bit mask
        let narrowed = vshrn_n_u16::<4>(vreinterpretq_u16_u8(flagged));
        vget_lane_u64::<0>(vreinterpret_u64_u8(narrowed))
    };
    if nibbles != 0 {
        Some(nibbles.trailing_zeros() as usize / 4)
    } else {
        None
    }
}

/// Returns the offset of the first byte in the 16 bytes starting at `pos`
/// which needs escaping.
#[cfg(all(target_arch = "x86_64", target_feature = "sse2", not(miri)))]
#[inline(always)]
fn block_escape(input: &[u8], pos: usize) -> Option<usize> {
    use std::arch::x86_64::*;
    let block: &[u8; 16] = input[pos..pos + 16].try_into().unwrap();
    // SAFETY: sse2 is available and the block is 16 bytes long
    let mask = unsafe {
        let chars = _mm_loadu_si128(block.as_ptr().cast::<__m128i>());
        // unsigned `chars <= 0x1f` is `min(chars, 0x1f) == chars`
        let ctrl = _mm_cmpeq_epi8(_mm_min_epu8(chars, _mm_set1_epi8(0x1f)), chars);
        let quote = _mm_cmpeq_epi8(chars, _mm_set1_epi8(b'"' as i8));
        let backslash = _mm_cmpeq_epi8(chars, _mm_set1_epi8(b'\\' as i8));
        _mm_movemask_epi8(_mm_or_si128(ctrl, _mm_or_si128(quote, backslash))) as u32
    };
    if mask != 0 {
        Some(mask.trailing_zeros() as usize)
    } else {
        None
    }
}

/// Returns the offset of the first byte in the 16 bytes starting at `pos`
/// which needs escaping.
#[cfg(not(any(
    all(target_arch = "aarch64", target_feature = "neon", not(miri)),
    all(target_arch = "x86_64", target_feature = "sse2", not(miri))
)))]
#[inline(always)]
fn block_escape(input: &[u8], pos: usize) -> Option<usize> {
    let masked = escape_mask(load_u64(input, pos));
    if masked != 0 {
        return Some(masked.trailing_zeros() as usize / 8);
    }
    let masked = escape_mask(load_u64(input, pos + 8));
    if masked != 0 {
        return Some(8 + masked.trailing_zeros() as usize / 8);
    }
    None
}

const ONE_BYTES: u64 = u64::MAX / 255;
const SPACES: u64 = ONE_BYTES * 0x20;

/// Flags the bytes in a word (in little endian order) which need escaping.
///
/// This is the classic "has zero byte" trick applied to control characters,
/// quotes and backslashes.  Only the lowest flagged byte is exact, bytes
/// above it might be flagged falsely.
#[inline(always)]
fn escape_mask(chars: u64) -> u64 {
    let contains_ctrl = chars.wrapping_sub(ONE_BYTES * 0x20) & !chars;
    let chars_quote = chars ^ (ONE_BYTES * u64::from(b'"'));
    let contains_quote = chars_quote.wrapping_sub(ONE_BYTES) & !chars_quote;
    let chars_backslash = chars ^ (ONE_BYTES * u64::from(b'\\'));
    let contains_backslash = chars_backslash.wrapping_sub(ONE_BYTES) & !chars_backslash;
    (contains_ctrl | contains_quote | contains_backslash) & (ONE_BYTES << 7)
}

#[inline(always)]
fn load_u64(input: &[u8], pos: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&input[pos..pos + 8]);
    u64::from_le_bytes(bytes)
}

#[inline(always)]
fn load_u32(input: &[u8], pos: usize) -> u64 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&input[pos..pos + 4]);
    u64::from(u32::from_le_bytes(bytes))
}

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

/// Checks if the bytes are ASCII.
///
/// Most strings are short, these are checked with (possibly overlapping)
/// word sized loads.
#[inline(always)]
pub fn is_ascii(bytes: &[u8]) -> bool {
    let len = bytes.len();
    if len > 16 {
        bytes.is_ascii()
    } else if len >= 8 {
        (load_u64(bytes, 0) | load_u64(bytes, len - 8)) & 0x8080_8080_8080_8080 == 0
    } else if len >= 4 {
        (load_u32(bytes, 0) | load_u32(bytes, len - 4)) & 0x8080_8080 == 0
    } else if len > 0 {
        (bytes[0] | bytes[len / 2] | bytes[len - 1]) < 0x80
    } else {
        true
    }
}

/// Checks if the bytes are valid UTF-8.
#[inline]
pub fn validate_utf8_slice(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

const CT: bool = true; // control character \x00..=\x1F
const QU: bool = true; // quote \x22
const BS: bool = true; // backslash \x5C
const O: bool = false; // allow unescaped

// Lookup table of bytes that must be escaped. A value of true at index i means
// that byte i requires an escape sequence in the input.
#[rustfmt::skip]
static ESCAPE: [bool; 256] = [
    //   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
    CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, // 0
    CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, // 1
     O,  O, QU,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 2
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 3
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 4
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, BS,  O,  O,  O, // 5
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 6
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 7
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 8
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 9
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // A
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // B
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // C
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // D
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // E
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // F
];

#[test]
fn test_is_ascii() {
    for len in 0..40 {
        let mut bytes = vec![b'a'; len];
        assert!(is_ascii(&bytes));
        for idx in 0..len {
            bytes[idx] = 0xc3;
            assert!(!is_ascii(&bytes), "{} {}", len, idx);
            bytes[idx] = b'a';
        }
    }
}

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

#[test]
fn test_skip_to_escape() {
    fn naive(input: &[u8], mut pos: usize) -> usize {
        while pos < input.len() && !ESCAPE[usize::from(input[pos])] {
            pos += 1;
        }
        pos
    }

    let alphabet: &[u8] = b"a\"\\\x00\x1f\x20\x7f\x80\xff\xe3";
    let mut state = 0x2545f4914f6cdd1du64;
    let rounds = if cfg!(miri) { 2 } else { 200 };
    for len in 0..80 {
        for _ in 0..rounds {
            let input: Vec<u8> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    // bias towards plain bytes
                    if state.is_multiple_of(4) {
                        alphabet[(state >> 8) as usize % alphabet.len()]
                    } else {
                        b'x'
                    }
                })
                .collect();
            for pos in 0..=len {
                assert_eq!(skip_to_escape(&input, pos), naive(&input, pos));
            }
        }
    }
}
