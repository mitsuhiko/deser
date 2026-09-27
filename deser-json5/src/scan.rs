// @generated from deser-private-jsontemplate/src/scan.rs by
// deser-private-jsontemplate/generate.py.  Do not edit.
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

/// Returns the index of the first byte at or after `pos` which needs special
/// handling within a string in single quotes (a single quote, a backslash or
/// a control character).
pub fn skip_to_escape_single(input: &[u8], mut pos: usize) -> usize {
    while pos < input.len() && !matches!(input[pos], b'\'' | b'\\' | 0x00..=0x1f) {
        pos += 1;
    }
    pos
}

/// Finds the ends of lines (for JSON Lines) in input with comments.
///
/// Only line breaks outside of comments and strings end a line, so a
/// comment can span lines and a string can contain what looks like a
/// comment.  The scan can be continued with more input.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LineScan {
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
    /// After an escaped carriage return (a line break continues).
    StrEscapeCr(u8),
}

impl LineScan {
    /// Returns the position of the line feed that ends the line.
    ///
    /// The input is scanned from `pos`, if it does not contain the end of
    /// the line, the scan continues where it stopped with more input.
    pub fn find_end(&mut self, input: &[u8], mut pos: usize) -> Option<usize> {
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
                (LineScan::Code, b'\'') => LineScan::Str(b'\''),
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
                (LineScan::StrEscape(quote), b'\r') => LineScan::StrEscapeCr(quote),
                (LineScan::StrEscape(quote), _) => LineScan::Str(quote),
                // `\r\n` in a string is a single line break
                (LineScan::StrEscapeCr(quote), b'\n') => LineScan::Str(quote),
                (LineScan::StrEscapeCr(quote), _) => {
                    *self = LineScan::Str(quote);
                    continue;
                }
            };
            pos += 1;
        }
        None
    }
}

pub const ONE_BYTES: u64 = u64::MAX / 255;

/// Flags the bytes in a word (in little endian order) which need escaping.
///
/// This is the classic "has zero byte" trick applied to control characters,
/// quotes and backslashes.  Only the lowest flagged byte is exact, bytes
/// above it might be flagged falsely.
#[inline(always)]
pub fn escape_mask(chars: u64) -> u64 {
    let contains_ctrl = chars.wrapping_sub(ONE_BYTES * 0x20) & !chars;
    let chars_quote = chars ^ (ONE_BYTES * u64::from(b'"'));
    let contains_quote = chars_quote.wrapping_sub(ONE_BYTES) & !chars_quote;
    let chars_backslash = chars ^ (ONE_BYTES * u64::from(b'\\'));
    let contains_backslash = chars_backslash.wrapping_sub(ONE_BYTES) & !chars_backslash;
    (contains_ctrl | contains_quote | contains_backslash) & (ONE_BYTES << 7)
}

#[inline(always)]
pub fn load_u64(input: &[u8], pos: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&input[pos..pos + 8]);
    u64::from_le_bytes(bytes)
}

#[inline(always)]
pub fn load_u32(input: &[u8], pos: usize) -> u64 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&input[pos..pos + 4]);
    u64::from(u32::from_le_bytes(bytes))
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
    #[cfg(feature = "speedups")]
    {
        simdutf8::basic::from_utf8(bytes).is_ok()
    }
    #[cfg(not(feature = "speedups"))]
    {
        std::str::from_utf8(bytes).is_ok()
    }
}

const CT: bool = true; // control character \x00..=\x1F
const QU: bool = true; // quote \x22
const BS: bool = true; // backslash \x5C
const O: bool = false; // allow unescaped

// Lookup table of bytes that must be escaped. A value of true at index i means
// that byte i requires an escape sequence in the input.
#[rustfmt::skip]
pub static ESCAPE: [bool; 256] = [
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
