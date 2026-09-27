//! Scanning utilities for the parser.

const ONE_BYTES: u64 = u64::MAX / 255;

/// Returns the index of the first byte at or after `pos` which needs special
/// handling within a string: the quote, a backslash, a control character
/// (including tabs and newlines) or DEL.
///
/// This processes a word at a time and falls back to a byte-wise scan for the
/// tail of the input.
#[inline]
pub(crate) fn skip_plain(input: &[u8], mut pos: usize, quote: u8) -> usize {
    while pos + 8 <= input.len() {
        let masked = special_mask(load_u64(input, pos), quote);
        if masked != 0 {
            return pos + masked.trailing_zeros() as usize / 8;
        }
        pos += 8;
    }
    while pos < input.len() && !is_special(input[pos], quote) {
        pos += 1;
    }
    pos
}

#[inline(always)]
fn is_special(c: u8, quote: u8) -> bool {
    c < 0x20 || c == 0x7f || c == quote || c == b'\\'
}

/// Flags the special bytes in a word (in little endian order).
///
/// This is the classic "has zero byte" trick.  Only the lowest flagged byte
/// is exact, bytes above it might be flagged falsely.
#[inline(always)]
fn special_mask(chars: u64, quote: u8) -> u64 {
    let has_zero = |x: u64| x.wrapping_sub(ONE_BYTES) & !x;
    let contains_ctrl = chars.wrapping_sub(ONE_BYTES * 0x20) & !chars;
    let contains_del = has_zero(chars ^ (ONE_BYTES * 0x7f));
    let contains_quote = has_zero(chars ^ (ONE_BYTES * u64::from(quote)));
    let contains_backslash = has_zero(chars ^ (ONE_BYTES * u64::from(b'\\')));
    (contains_ctrl | contains_del | contains_quote | contains_backslash) & (ONE_BYTES << 7)
}

#[inline(always)]
fn load_u64(input: &[u8], pos: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&input[pos..pos + 8]);
    u64::from_le_bytes(bytes)
}

#[test]
fn test_skip_plain() {
    fn naive(input: &[u8], mut pos: usize, quote: u8) -> usize {
        while pos < input.len() && !is_special(input[pos], quote) {
            pos += 1;
        }
        pos
    }

    let alphabet: &[u8] = b"a\"'\\\x00\t\n\x1f\x20\x7e\x7f\x80\xff\xe3";
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
            for pos in [0, 1, 7, 9].into_iter().filter(|&pos| pos <= len) {
                for quote in *b"\"'" {
                    assert_eq!(
                        skip_plain(&input, pos, quote),
                        naive(&input, pos, quote),
                        "{:?} {} {}",
                        input,
                        pos,
                        quote
                    );
                }
            }
        }
    }
}
