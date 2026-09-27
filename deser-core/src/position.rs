use std::fmt;

/// A position in the input.
///
/// Lines and columns are 1-based.  Columns are counted in characters (bytes
/// that are not UTF-8 continuation bytes).  This is how positions of errors
/// (see [`Error::line`](crate::Error::line)) are counted as well.
///
/// ```
/// use deser::Position;
///
/// let pos = Position::of("[1,\n  x]".as_bytes(), 6);
/// assert_eq!((pos.offset, pos.line, pos.column), (6, 2, 3));
/// ```
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    /// The byte offset from the start of the input.
    pub offset: usize,
    /// The line number (1-based).
    pub line: usize,
    /// The column number in characters (1-based).
    pub column: usize,
}

impl Position {
    /// Returns the start of the input.
    pub const fn start() -> Position {
        Position {
            offset: 0,
            line: 1,
            column: 1,
        }
    }

    /// Resolves a byte offset in the source into a position.
    ///
    /// Offsets beyond the end of the source are clamped.  This scans the
    /// source up to the offset, to resolve many offsets in the same source
    /// use `deser_location::SourceMap`.
    pub fn of(source: &[u8], offset: usize) -> Position {
        let mut rv = Position::start();
        rv.advance(&source[..offset.min(source.len())]);
        rv
    }

    /// Advances the position over the given bytes.
    ///
    /// ```
    /// use deser::Position;
    ///
    /// let mut pos = Position::start();
    /// pos.advance("ab\nc".as_bytes());
    /// assert_eq!(pos.to_string(), "2:2");
    /// pos.advance("äd".as_bytes());
    /// assert_eq!(pos.to_string(), "2:4");
    /// ```
    pub fn advance(&mut self, bytes: &[u8]) {
        let mut line_start = None;
        find_newlines(bytes, |idx| {
            self.line += 1;
            line_start = Some(idx + 1);
        });
        let rest = match line_start {
            Some(line_start) => {
                self.column = 1;
                &bytes[line_start..]
            }
            None => bytes,
        };
        self.column += count_chars(rest);
        self.offset += bytes.len();
    }
}

impl Default for Position {
    fn default() -> Position {
        Position::start()
    }
}

impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

// The helpers below process the input a word at a time as positions are
// resolved for errors and (with deser-location) for every event.

const LO7: u64 = 0x7f7f_7f7f_7f7f_7f7f;
const HI: u64 = 0x8080_8080_8080_8080;
const NEWLINES: u64 = 0x0a0a_0a0a_0a0a_0a0a;

/// Sets the high bit of every byte that is zero (exact, no false positives).
fn zero_bytes(x: u64) -> u64 {
    !(((x & LO7).wrapping_add(LO7)) | x | LO7)
}

/// Invokes the callback with the index of every newline.
fn find_newlines<F: FnMut(usize)>(bytes: &[u8], mut f: F) {
    let (chunks, rest) = bytes.as_chunks::<8>();
    for (idx, &chunk) in chunks.iter().enumerate() {
        let mut mask = zero_bytes(u64::from_le_bytes(chunk) ^ NEWLINES);
        while mask != 0 {
            f(idx * 8 + mask.trailing_zeros() as usize / 8);
            mask &= mask - 1;
        }
    }
    let offset = bytes.len() - rest.len();
    for (idx, &byte) in rest.iter().enumerate() {
        if byte == b'\n' {
            f(offset + idx);
        }
    }
}

/// Counts the characters (bytes that are not utf-8 continuation bytes).
fn count_chars(bytes: &[u8]) -> usize {
    let (chunks, rest) = bytes.as_chunks::<8>();
    let mut continuation = 0;
    for &chunk in chunks {
        let w = u64::from_le_bytes(chunk);
        // high bit set and the bit below it cleared
        continuation += (w & !(w << 1) & HI).count_ones() as usize;
    }
    continuation += rest.iter().filter(|&&b| b & 0xc0 == 0x80).count();
    bytes.len() - continuation
}

#[test]
fn test_helpers() {
    let mut state = 0x2545f4914f6cdd1du64;
    let alphabet = "ab\n\u{e4}\u{1f600}x\n".as_bytes();
    let rounds = if cfg!(miri) { 1 } else { 50 };
    for len in 0..64 {
        for _ in 0..rounds {
            let input: Vec<u8> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    alphabet[(state % alphabet.len() as u64) as usize]
                })
                .collect();
            let mut newlines = Vec::new();
            find_newlines(&input, |idx| newlines.push(idx));
            let expected: Vec<usize> = input
                .iter()
                .enumerate()
                .filter(|&(_, &b)| b == b'\n')
                .map(|(idx, _)| idx)
                .collect();
            assert_eq!(newlines, expected);
            assert_eq!(
                count_chars(&input),
                input.iter().filter(|&&b| b & 0xc0 != 0x80).count()
            );

            // advancing in pieces is the same as in one go
            let split = input.len() / 3;
            let mut pos = Position::start();
            pos.advance(&input[..split]);
            pos.advance(&input[split..]);
            let line_start = input.iter().rposition(|&b| b == b'\n').map_or(0, |x| x + 1);
            let expected = Position {
                offset: input.len(),
                line: newlines.len() + 1,
                column: input[line_start..]
                    .iter()
                    .filter(|&&b| b & 0xc0 != 0x80)
                    .count()
                    + 1,
            };
            assert_eq!(pos, expected);
            assert_eq!(Position::of(&input, input.len()), expected);
        }
    }
}

#[test]
fn test_of() {
    let source = "ab\ncäd\n\nx".as_bytes();
    let pos = |offset| Position::of(source, offset).to_string();
    assert_eq!(pos(0), "1:1");
    assert_eq!(pos(2), "1:3");
    assert_eq!(pos(3), "2:1");
    // ä is two bytes
    assert_eq!(pos(6), "2:3");
    assert_eq!(pos(7), "2:4");
    assert_eq!(pos(8), "3:1");
    assert_eq!(pos(9), "4:1");
    // clamped
    assert_eq!(pos(100), "4:2");
}
