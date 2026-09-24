//! Shared scanning utilities for the parser and serializer.

/// Returns the index of the first byte at or after `pos` which needs special
/// handling within a string (a quote, a backslash or a control character).
///
/// This processes a word at a time and falls back to a byte-wise scan for the
/// tail of the input.
#[inline]
pub fn skip_to_escape(input: &[u8], mut pos: usize) -> usize {
    type Chunk = usize;
    const STEP: usize = std::mem::size_of::<Chunk>();
    const ONE_BYTES: Chunk = Chunk::MAX / 255;

    if pos >= input.len() || ESCAPE[usize::from(input[pos])] {
        return pos;
    }
    pos += 1;

    while pos + STEP <= input.len() {
        let mut bytes = [0u8; STEP];
        bytes.copy_from_slice(&input[pos..pos + STEP]);
        let chars = Chunk::from_le_bytes(bytes);
        // the classic "has zero byte" trick applied to control characters,
        // quotes and backslashes.  The lowest flagged byte is always exact.
        let contains_ctrl = chars.wrapping_sub(ONE_BYTES * 0x20) & !chars;
        let chars_quote = chars ^ (ONE_BYTES * Chunk::from(b'"'));
        let contains_quote = chars_quote.wrapping_sub(ONE_BYTES) & !chars_quote;
        let chars_backslash = chars ^ (ONE_BYTES * Chunk::from(b'\\'));
        let contains_backslash = chars_backslash.wrapping_sub(ONE_BYTES) & !chars_backslash;
        let masked = (contains_ctrl | contains_quote | contains_backslash) & (ONE_BYTES << 7);
        if masked != 0 {
            return pos + masked.trailing_zeros() as usize / 8;
        }
        pos += STEP;
    }

    while pos < input.len() && !ESCAPE[usize::from(input[pos])] {
        pos += 1;
    }
    pos
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
fn test_skip_to_escape() {
    fn naive(input: &[u8], mut pos: usize) -> usize {
        while pos < input.len() && !ESCAPE[usize::from(input[pos])] {
            pos += 1;
        }
        pos
    }

    let alphabet: &[u8] = b"a\"\\\x00\x1f\x20\x7f\x80\xff\xe3";
    let mut state = 0x2545f4914f6cdd1du64;
    for len in 0..40 {
        for _ in 0..200 {
            let input: Vec<u8> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    // bias towards plain bytes
                    if state % 4 == 0 {
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
