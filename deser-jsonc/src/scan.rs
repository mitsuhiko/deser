// @generated from deser-template-json/src/scan.rs by
// deser-template-json/generate.py.  Do not edit.
//! Shared scanning utilities for the parser and serializer.

/// Returns the index of the first byte at or after `pos` which needs special
/// handling within a string (a quote, a backslash or a control character).
///
/// Most strings are short, the first 33 bytes are checked a byte and then
/// a word at a time (inlined into the parser).  Longer strings continue
/// with SIMD in [`skip_to_escape_long`], out of line.
#[inline]
pub(crate) fn skip_to_escape(input: &[u8], mut pos: usize) -> usize {
    if pos >= input.len() || ESCAPE[usize::from(input[pos])] {
        return pos;
    }
    pos += 1;

    for _ in 0..4 {
        if pos + 8 > input.len() {
            break;
        }
        let masked = escape_mask(load_u64(input, pos));
        if masked != 0 {
            return pos + masked.trailing_zeros() as usize / 8;
        }
        pos += 8;
    }
    skip_to_escape_long(input, pos)
}

/// Continues [`skip_to_escape`] for long strings.
///
/// After two blocks of 16 bytes (strings of medium length), blocks of 64
/// bytes are checked at once, the block with the byte is searched 16
/// bytes at a time.
#[inline(never)]
fn skip_to_escape_long(input: &[u8], mut pos: usize) -> usize {
    for _ in 0..2 {
        if pos + 16 > input.len() {
            break;
        }
        if let Some(offset) = block_escape(input, pos) {
            return pos + offset;
        }
        pos += 16;
    }
    while pos + 64 <= input.len() {
        if block64_has_escape(input, pos) {
            break;
        }
        pos += 64;
    }
    while pos + 16 <= input.len() {
        if let Some(offset) = block_escape(input, pos) {
            return pos + offset;
        }
        pos += 16;
    }
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

/// Finds the bytes which need special handling within a string, for
/// strings which have many of them (like code with its line breaks and
/// quotes).
///
/// The bytes are found in blocks of 64 bytes: a bit per byte is computed
/// once per block, the following bytes of the block are found by the bits
/// that remain.
pub(crate) struct EscapeScanner {
    /// The end of the block (0 before the first one).
    end: usize,
    /// The bytes of the block that need escaping, bit 0 is the first one.
    bits: u64,
}

impl EscapeScanner {
    pub(crate) fn new() -> EscapeScanner {
        EscapeScanner { end: 0, bits: 0 }
    }

    /// Returns the index of the first byte at or after `pos` which needs
    /// special handling, like [`skip_to_escape`].
    ///
    /// The positions must not go backwards.
    #[inline(always)]
    pub(crate) fn next(&mut self, input: &[u8], mut pos: usize) -> usize {
        loop {
            if pos < self.end {
                let start = self.end - 64;
                let bits = self.bits & (u64::MAX << (pos - start));
                if bits != 0 {
                    return start + bits.trailing_zeros() as usize;
                }
                pos = self.end;
            }
            if pos + 64 > input.len() {
                return skip_to_escape(input, pos);
            }
            self.bits = escape_bits(input, pos);
            self.end = pos + 64;
        }
    }
}

/// Returns a bit for each of the 64 bytes starting at `pos` which is set if
/// the byte needs escaping (bit 0 for the first byte).
#[cfg(all(target_arch = "aarch64", target_feature = "neon", not(miri)))]
#[inline(always)]
fn escape_bits(input: &[u8], pos: usize) -> u64 {
    use core::arch::aarch64::*;
    const BITS: [u8; 16] = [1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128];
    let block: &[u8; 64] = input[pos..pos + 64].try_into().unwrap();
    // SAFETY: neon is available
    unsafe {
        let bits = vld1q_u8(BITS.as_ptr());
        let a = vandq_u8(escape_flags(block[..16].try_into().unwrap()), bits);
        let b = vandq_u8(escape_flags(block[16..32].try_into().unwrap()), bits);
        let c = vandq_u8(escape_flags(block[32..48].try_into().unwrap()), bits);
        let d = vandq_u8(escape_flags(block[48..].try_into().unwrap()), bits);
        // adding neighbors three times combines the bits of 8 bytes into one
        let ab = vpaddq_u8(a, b);
        let cd = vpaddq_u8(c, d);
        let abcd = vpaddq_u8(ab, cd);
        vgetq_lane_u64::<0>(vreinterpretq_u64_u8(vpaddq_u8(abcd, abcd)))
    }
}

/// Returns a bit for each of the 64 bytes starting at `pos` which is set if
/// the byte needs escaping (bit 0 for the first byte).
#[cfg(all(target_arch = "x86_64", target_feature = "sse2", not(miri)))]
#[inline(always)]
fn escape_bits(input: &[u8], pos: usize) -> u64 {
    use core::arch::x86_64::*;
    let block: &[u8; 64] = input[pos..pos + 64].try_into().unwrap();
    // SAFETY: sse2 is available
    unsafe {
        let a = _mm_movemask_epi8(escape_flags(block[..16].try_into().unwrap())) as u16;
        let b = _mm_movemask_epi8(escape_flags(block[16..32].try_into().unwrap())) as u16;
        let c = _mm_movemask_epi8(escape_flags(block[32..48].try_into().unwrap())) as u16;
        let d = _mm_movemask_epi8(escape_flags(block[48..].try_into().unwrap())) as u16;
        u64::from(a) | u64::from(b) << 16 | u64::from(c) << 32 | u64::from(d) << 48
    }
}

/// Returns a bit for each of the 64 bytes starting at `pos` which is set if
/// the byte needs escaping (bit 0 for the first byte).
#[cfg(not(any(
    all(target_arch = "aarch64", target_feature = "neon", not(miri)),
    all(target_arch = "x86_64", target_feature = "sse2", not(miri))
)))]
#[inline(always)]
fn escape_bits(input: &[u8], pos: usize) -> u64 {
    input[pos..pos + 64]
        .iter()
        .enumerate()
        .fold(0, |bits, (idx, &byte)| {
            bits | u64::from(ESCAPE[usize::from(byte)]) << idx
        })
}

/// Returns the flags of the 16 bytes (`0xff` for a byte that needs
/// escaping).
///
/// # Safety
///
/// neon must be available.
#[cfg(all(target_arch = "aarch64", target_feature = "neon", not(miri)))]
#[inline(always)]
unsafe fn escape_flags(block: &[u8; 16]) -> core::arch::aarch64::uint8x16_t {
    use core::arch::aarch64::*;
    // SAFETY: neon is available and the block is 16 bytes long
    unsafe {
        let chars = vld1q_u8(block.as_ptr());
        let ctrl = vcltq_u8(chars, vdupq_n_u8(0x20));
        let quote = vceqq_u8(chars, vdupq_n_u8(b'"'));
        let backslash = vceqq_u8(chars, vdupq_n_u8(b'\\'));
        vorrq_u8(ctrl, vorrq_u8(quote, backslash))
    }
}

/// Returns the offset of the first byte in the 16 bytes starting at `pos`
/// which needs escaping.
#[cfg(all(target_arch = "aarch64", target_feature = "neon", not(miri)))]
#[inline(always)]
pub(crate) fn block_escape(input: &[u8], pos: usize) -> Option<usize> {
    use core::arch::aarch64::*;
    let block: &[u8; 16] = input[pos..pos + 16].try_into().unwrap();
    // SAFETY: neon is available
    let nibbles = unsafe {
        let flagged = escape_flags(block);
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

/// Returns `true` if one of the 64 bytes starting at `pos` needs escaping.
#[cfg(all(target_arch = "aarch64", target_feature = "neon", not(miri)))]
#[inline(always)]
fn block64_has_escape(input: &[u8], pos: usize) -> bool {
    use core::arch::aarch64::*;
    let block: &[u8; 64] = input[pos..pos + 64].try_into().unwrap();
    // SAFETY: neon is available
    unsafe {
        let a = escape_flags(block[..16].try_into().unwrap());
        let b = escape_flags(block[16..32].try_into().unwrap());
        let c = escape_flags(block[32..48].try_into().unwrap());
        let d = escape_flags(block[48..].try_into().unwrap());
        vmaxvq_u8(vorrq_u8(vorrq_u8(a, b), vorrq_u8(c, d))) != 0
    }
}

/// Returns the flags of the 16 bytes (`0xff` for a byte that needs
/// escaping).
///
/// # Safety
///
/// sse2 must be available.
#[cfg(all(target_arch = "x86_64", target_feature = "sse2", not(miri)))]
#[inline(always)]
unsafe fn escape_flags(block: &[u8; 16]) -> core::arch::x86_64::__m128i {
    use core::arch::x86_64::*;
    // SAFETY: sse2 is available and the block is 16 bytes long
    unsafe {
        let chars = _mm_loadu_si128(block.as_ptr().cast::<__m128i>());
        // unsigned `chars <= 0x1f` is `min(chars, 0x1f) == chars`
        let ctrl = _mm_cmpeq_epi8(_mm_min_epu8(chars, _mm_set1_epi8(0x1f)), chars);
        let quote = _mm_cmpeq_epi8(chars, _mm_set1_epi8(b'"' as i8));
        let backslash = _mm_cmpeq_epi8(chars, _mm_set1_epi8(b'\\' as i8));
        _mm_or_si128(ctrl, _mm_or_si128(quote, backslash))
    }
}

/// Returns the offset of the first byte in the 16 bytes starting at `pos`
/// which needs escaping.
#[cfg(all(target_arch = "x86_64", target_feature = "sse2", not(miri)))]
#[inline(always)]
pub fn block_escape(input: &[u8], pos: usize) -> Option<usize> {
    use core::arch::x86_64::*;
    let block: &[u8; 16] = input[pos..pos + 16].try_into().unwrap();
    // SAFETY: sse2 is available
    let mask = unsafe { _mm_movemask_epi8(escape_flags(block)) as u32 };
    if mask != 0 {
        Some(mask.trailing_zeros() as usize)
    } else {
        None
    }
}

/// Returns `true` if one of the 64 bytes starting at `pos` needs escaping.
#[cfg(all(target_arch = "x86_64", target_feature = "sse2", not(miri)))]
#[inline(always)]
fn block64_has_escape(input: &[u8], pos: usize) -> bool {
    use core::arch::x86_64::*;
    let block: &[u8; 64] = input[pos..pos + 64].try_into().unwrap();
    // SAFETY: sse2 is available
    unsafe {
        let a = escape_flags(block[..16].try_into().unwrap());
        let b = escape_flags(block[16..32].try_into().unwrap());
        let c = escape_flags(block[32..48].try_into().unwrap());
        let d = escape_flags(block[48..].try_into().unwrap());
        _mm_movemask_epi8(_mm_or_si128(_mm_or_si128(a, b), _mm_or_si128(c, d))) != 0
    }
}

/// Returns the offset of the first byte in the 16 bytes starting at `pos`
/// which needs escaping.
#[cfg(not(any(
    all(target_arch = "aarch64", target_feature = "neon", not(miri)),
    all(target_arch = "x86_64", target_feature = "sse2", not(miri))
)))]
#[inline(always)]
pub fn block_escape(input: &[u8], pos: usize) -> Option<usize> {
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

/// Returns `true` if one of the 64 bytes starting at `pos` needs escaping.
#[cfg(not(any(
    all(target_arch = "aarch64", target_feature = "neon", not(miri)),
    all(target_arch = "x86_64", target_feature = "sse2", not(miri))
)))]
#[inline(always)]
fn block64_has_escape(input: &[u8], pos: usize) -> bool {
    (0..8).fold(0, |acc, idx| {
        acc | escape_mask(load_u64(input, pos + idx * 8))
    }) != 0
}

/// Finds the ends of lines (for JSON Lines) in input with comments.
///
/// Only line breaks outside of comments and strings end a line, so a
/// comment can span lines and a string can contain what looks like a
/// comment.  The scan can be continued with more input.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineScan {
    #[default]
    Code,
    /// After a slash.
    Slash,
    LineComment,
    BlockComment,
    /// After a star in a block comment.
    BlockCommentStar,
    /// In a string with the quote.
    Str(u8),
    /// After a backslash in a string.
    StrEscape(u8),
}

impl LineScan {
    /// Returns the position of the line feed that ends the line.
    ///
    /// The input is scanned from `pos`, if it does not contain the end of
    /// the line, the scan continues where it stopped with more input.
    pub(crate) fn find_end(&mut self, input: &[u8], mut pos: usize) -> Option<usize> {
        while pos < input.len() {
            let byte = input[pos];
            *self = match (*self, byte) {
                // a line feed in a string is invalid, it still ends the line
                (LineScan::Code | LineScan::LineComment | LineScan::Str(_), b'\n') => {
                    *self = LineScan::Code;
                    return Some(pos);
                }
                (LineScan::Code, b'/') => LineScan::Slash,
                (LineScan::Code, b'"') => LineScan::Str(b'"'),
                (LineScan::Code, _) => LineScan::Code,
                (LineScan::Slash, b'/') => LineScan::LineComment,
                (LineScan::Slash, b'*') => LineScan::BlockComment,
                // the byte after a slash that does not start a comment is
                // scanned again
                (LineScan::Slash, _) => {
                    *self = LineScan::Code;
                    continue;
                }
                (LineScan::LineComment, b'\r') => LineScan::Code,
                (LineScan::LineComment, _) => LineScan::LineComment,
                (LineScan::BlockComment | LineScan::BlockCommentStar, b'*') => {
                    LineScan::BlockCommentStar
                }
                (LineScan::BlockCommentStar, b'/') => LineScan::Code,
                (LineScan::BlockComment | LineScan::BlockCommentStar, _) => LineScan::BlockComment,
                (LineScan::Str(quote), b'\\') => LineScan::StrEscape(quote),
                (LineScan::Str(quote), _) if byte == quote => LineScan::Code,
                (LineScan::Str(quote), _) => LineScan::Str(quote),
                (LineScan::StrEscape(quote), _) => LineScan::Str(quote),
            };
            pos += 1;
        }
        None
    }
}

pub(crate) const ONE_BYTES: u64 = u64::MAX / 255;

/// Flags the bytes in a word (in little endian order) which need escaping.
///
/// This is the classic "has zero byte" trick applied to control characters,
/// quotes and backslashes.  Only the lowest flagged byte is exact, bytes
/// above it might be flagged falsely.
#[inline(always)]
pub(crate) fn escape_mask(chars: u64) -> u64 {
    let contains_ctrl = chars.wrapping_sub(ONE_BYTES * 0x20) & !chars;
    let chars_quote = chars ^ (ONE_BYTES * u64::from(b'"'));
    let contains_quote = chars_quote.wrapping_sub(ONE_BYTES) & !chars_quote;
    let chars_backslash = chars ^ (ONE_BYTES * u64::from(b'\\'));
    let contains_backslash = chars_backslash.wrapping_sub(ONE_BYTES) & !chars_backslash;
    (contains_ctrl | contains_quote | contains_backslash) & (ONE_BYTES << 7)
}

#[inline(always)]
pub(crate) fn load_u64(input: &[u8], pos: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&input[pos..pos + 8]);
    u64::from_le_bytes(bytes)
}

#[inline(always)]
pub(crate) fn load_u32(input: &[u8], pos: usize) -> u64 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&input[pos..pos + 4]);
    u64::from(u32::from_le_bytes(bytes))
}

/// Checks if the bytes are ASCII.
///
/// Most strings are short, these are checked with (possibly overlapping)
/// word sized loads.
#[inline(always)]
pub(crate) fn is_ascii(bytes: &[u8]) -> bool {
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
pub(crate) fn validate_utf8_slice(bytes: &[u8]) -> bool {
    #[cfg(feature = "speedups")]
    {
        simdutf8::basic::from_utf8(bytes).is_ok()
    }
    #[cfg(not(feature = "speedups"))]
    {
        core::str::from_utf8(bytes).is_ok()
    }
}

const CT: bool = true; // control character \x00..=\x1F
const QU: bool = true; // quote \x22
const BS: bool = true; // backslash \x5C
const O: bool = false; // allow unescaped

// Lookup table of bytes that must be escaped. A value of true at index i means
// that byte i requires an escape sequence in the input.
#[rustfmt::skip]
pub(crate) static ESCAPE: [bool; 256] = [
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
fn test_escape_scanner() {
    let mut state = 0x2545f4914f6cdd1du64;
    let rounds = if cfg!(miri) { 5 } else { 2000 };
    for _ in 0..rounds {
        let len = (state % 300) as usize;
        let input: Vec<u8> = (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                match state % 16 {
                    0 => b'"',
                    1 => b'\\',
                    2 => b'\n',
                    3 => 0x80,
                    _ => b'x',
                }
            })
            .collect();
        let mut scanner = EscapeScanner::new();
        let mut pos = 0;
        while pos < len {
            let expected = skip_to_escape(&input, pos);
            assert_eq!(scanner.next(&input, pos), expected);
            // continue after the byte, sometimes further
            pos = expected + 1 + (state % 3) as usize;
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
    // short strings of every length and long ones around the blocks of 64
    // bytes
    for len in (0..80).chain([127, 128, 129, 200, 300]) {
        // bias towards plain bytes, long strings have blocks without
        // bytes that need escaping
        let bias = if len < 80 { 4 } else { 128 };
        for _ in 0..rounds {
            let input: Vec<u8> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    if state.is_multiple_of(bias) {
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
