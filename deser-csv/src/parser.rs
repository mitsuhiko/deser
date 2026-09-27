//! Splits delimited text into records and fields.
//!
//! The scanner finds the fields of a record in a single pass and remembers
//! their positions, the deserializer emits them from there.  It can be
//! suspended at any byte (when more input is needed) and continues where it
//! stopped once the input is longer.
use alloc::format;
use alloc::vec::Vec;
use deser_core::{Error, ErrorKind};

use crate::{Escape, Terminator};

// the classes of bytes
const OTHER: u8 = 0;
const DELIMITER: u8 = 1;
const QUOTE: u8 = 2;
const ESCAPE: u8 = 3;
const TERMINATOR: u8 = 4;
const CR: u8 = 5;

const LO: u64 = 0x0101_0101_0101_0101;
const HI: u64 = 0x8080_8080_8080_8080;

/// The field was quoted.
pub(crate) const QUOTED: u8 = 1;
/// The field contains escapes (or doubled quotes) that need to be decoded.
pub(crate) const UNESCAPE: u8 = 2;

/// The special characters of a dialect.
#[derive(Clone)]
pub(crate) struct Dialect {
    pub(crate) delimiter: u8,
    pub(crate) quote: Option<u8>,
    pub(crate) double_quote: bool,
    pub(crate) escape: Escape,
    pub(crate) comment: Option<u8>,
    classes: [u8; 256],
    // the special characters (repeated to fill the array) in every byte
    // of a word
    specials: [u64; 5],
}

impl core::fmt::Debug for Dialect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Dialect")
            .field("delimiter", &self.delimiter)
            .field("quote", &self.quote)
            .finish_non_exhaustive()
    }
}

impl Dialect {
    /// Creates a dialect and checks that the special characters are ASCII
    /// and distinct.
    pub(crate) fn new(
        delimiter: u8,
        quote: Option<u8>,
        double_quote: bool,
        escape: Escape,
        terminator: Terminator,
        comment: Option<u8>,
    ) -> Result<Dialect, Error> {
        let mut classes = [OTHER; 256];
        let mut assign = |byte: u8, class: u8, what: &str| -> Result<(), Error> {
            if !byte.is_ascii() {
                return Err(config_error(format!(
                    "the {} must be an ASCII character",
                    what
                )));
            }
            if classes[byte as usize] != OTHER {
                return Err(config_error(format!(
                    "the {} conflicts with another special character",
                    what
                )));
            }
            classes[byte as usize] = class;
            Ok(())
        };
        match terminator {
            Terminator::Newline | Terminator::CrLf => {
                assign(b'\n', TERMINATOR, "terminator")?;
                assign(b'\r', CR, "terminator")?;
            }
            Terminator::Byte(byte) => assign(byte, TERMINATOR, "terminator")?,
        }
        assign(delimiter, DELIMITER, "delimiter")?;
        if let Some(quote) = quote {
            assign(quote, QUOTE, "quote character")?;
        }
        if let Some(escape) = escape.byte() {
            assign(escape, ESCAPE, "escape character")?;
        }
        if let Some(comment) = comment
            && (!comment.is_ascii() || classes[comment as usize] != OTHER)
        {
            return Err(config_error(
                "the comment character must be an ASCII character that is not special",
            ));
        }
        let (first, second) = match terminator {
            Terminator::Newline | Terminator::CrLf => (b'\n', b'\r'),
            Terminator::Byte(byte) => (byte, byte),
        };
        let specials = [
            delimiter,
            quote.unwrap_or(delimiter),
            escape.byte().unwrap_or(delimiter),
            first,
            second,
        ]
        .map(|byte| u64::from(byte) * LO);
        Ok(Dialect {
            delimiter,
            quote,
            double_quote,
            escape,
            comment,
            classes,
            specials,
        })
    }

    #[inline(always)]
    fn class(&self, byte: u8) -> u8 {
        self.classes[byte as usize]
    }

    /// Returns `true` if the byte is a special character.
    #[inline(always)]
    pub(crate) fn is_special(&self, byte: u8) -> bool {
        self.class(byte) != OTHER
    }

    /// Returns `true` if the text contains a special character.
    #[inline]
    pub(crate) fn has_special(&self, text: &[u8]) -> bool {
        // eight bytes at a time: a byte of `x` is zero where the text has
        // the special character
        let (chunks, rest) = text.as_chunks::<8>();
        for chunk in chunks {
            let word = u64::from_ne_bytes(*chunk);
            let mut found = 0;
            for special in self.specials {
                let x = word ^ special;
                found |= x.wrapping_sub(LO) & !x & HI;
            }
            if found != 0 {
                return true;
            }
        }
        rest.iter().any(|&b| self.is_special(b))
    }

    /// Returns `true` if the byte ends a record.
    pub(crate) fn is_terminator(&self, byte: u8) -> bool {
        matches!(self.class(byte), TERMINATOR | CR)
    }
}

#[cold]
fn config_error<M: Into<alloc::borrow::Cow<'static, str>>>(msg: M) -> Error {
    Error::new(ErrorKind::Unexpected, msg)
}

/// A field of a record.  The positions are relative to the record.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Field {
    /// The start of the field (including quotes and whitespace).
    pub(crate) span_start: usize,
    /// The end of the field (including quotes and whitespace).
    pub(crate) span_end: usize,
    /// The start of the text (without quotes).
    pub(crate) start: usize,
    /// The end of the text (without quotes).
    pub(crate) end: usize,
    pub(crate) flags: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Before the first byte of a record.
    RecordStart,
    /// In a comment line.
    Comment,
    /// Before the first byte of a field.
    FieldStart,
    Unquoted,
    UnquotedEscape,
    Quoted,
    QuotedEscape,
    /// After a quote in a quoted field (it ends the field or is doubled).
    QuotedQuote,
    /// After the closing quote.
    AfterQuoted,
}

/// The result of a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scan {
    /// A record ends at `end`, the first `consumed` bytes of the input are
    /// used (including the terminator).  The fields are in the scanner.
    Record { end: usize, consumed: usize },
    /// A blank line or comment was skipped.
    Skip { consumed: usize },
    /// More input is needed.
    Incomplete,
    /// There are no more records.
    End,
}

/// How a record is scanned.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Options {
    /// Whitespace around fields (and quotes) is not part of the fields.
    pub(crate) trim: bool,
    pub(crate) skip_blank_lines: bool,
    /// Quotes in unquoted fields and characters after quoted fields are
    /// accepted.
    pub(crate) lenient_quotes: bool,
    pub(crate) max_record_len: usize,
}

/// Finds the fields of records.
#[derive(Debug)]
pub(crate) struct Scanner {
    mode: Mode,
    pos: usize,
    span_start: usize,
    start: usize,
    end: usize,
    flags: u8,
    /// The fields of the last record.
    pub(crate) fields: Vec<Field>,
    /// The first error of the last record (offset and message).  The
    /// record can still be split, which is why this does not end the
    /// stream.
    pub(crate) error: Option<(usize, &'static str)>,
    /// A record ended with a CR at the end of the input, a LF that follows
    /// belongs to it.
    pending_lf: bool,
}

impl Default for Scanner {
    fn default() -> Scanner {
        Scanner {
            mode: Mode::RecordStart,
            pos: 0,
            span_start: 0,
            start: 0,
            end: 0,
            flags: 0,
            fields: Vec::new(),
            error: None,
            pending_lf: false,
        }
    }
}

impl Scanner {
    /// Scans the next record.
    ///
    /// The input starts at the start of the record (or of what is skipped
    /// before it).  After [`Scan::Incomplete`] the scanner has to be invoked
    /// again with the same input plus more data.
    pub(crate) fn scan(
        &mut self,
        dialect: &Dialect,
        input: &[u8],
        eof: bool,
        options: Options,
    ) -> Result<Scan, Error> {
        if self.mode == Mode::RecordStart {
            if self.pending_lf {
                match input.first() {
                    None if eof => return Ok(Scan::End),
                    None => return Ok(Scan::Incomplete),
                    Some(b'\n') => {
                        self.pending_lf = false;
                        return Ok(Scan::Skip { consumed: 1 });
                    }
                    Some(_) => self.pending_lf = false,
                }
            }
            let Some(&first) = input.first() else {
                return Ok(if eof { Scan::End } else { Scan::Incomplete });
            };
            self.fields.clear();
            self.error = None;
            self.pos = 0;
            if Some(first) == dialect.comment {
                self.mode = Mode::Comment;
            } else if dialect.is_terminator(first) {
                if options.skip_blank_lines {
                    let consumed = self.terminator(dialect, input, 0, eof);
                    return Ok(Scan::Skip { consumed });
                }
                // a record with one empty field
                self.fields.push(Field {
                    span_start: 0,
                    span_end: 0,
                    start: 0,
                    end: 0,
                    flags: 0,
                });
                return Ok(self.end_record(dialect, input, 0, eof));
            } else {
                self.start_field(0);
            }
        }

        let len = input.len();
        let mut pos = self.pos;
        while pos < len {
            match self.mode {
                Mode::Comment => {
                    match input[pos..].iter().position(|&b| dialect.is_terminator(b)) {
                        Some(index) => {
                            let consumed = self.terminator(dialect, input, pos + index, eof);
                            self.mode = Mode::RecordStart;
                            return Ok(Scan::Skip { consumed });
                        }
                        None => pos = len,
                    }
                }
                Mode::FieldStart => {
                    let byte = input[pos];
                    match dialect.class(byte) {
                        OTHER if options.trim && is_space(byte) => pos += 1,
                        OTHER => {
                            self.start = pos;
                            self.mode = Mode::Unquoted;
                            pos += 1;
                        }
                        QUOTE => {
                            self.flags = QUOTED;
                            self.start = pos + 1;
                            self.mode = Mode::Quoted;
                            pos += 1;
                        }
                        ESCAPE => {
                            self.start = pos;
                            self.flags = UNESCAPE;
                            self.mode = Mode::UnquotedEscape;
                            pos += 1;
                        }
                        DELIMITER => {
                            self.start = pos;
                            self.push_unquoted(input, pos, options);
                            pos += 1;
                            self.start_field(pos);
                        }
                        _ => {
                            self.start = pos;
                            self.push_unquoted(input, pos, options);
                            return Ok(self.end_record(dialect, input, pos, eof));
                        }
                    }
                }
                Mode::Unquoted => {
                    while pos < len && dialect.class(input[pos]) == OTHER {
                        pos += 1;
                    }
                    let Some(&byte) = input.get(pos) else {
                        break;
                    };
                    match dialect.class(byte) {
                        DELIMITER => {
                            self.push_unquoted(input, pos, options);
                            pos += 1;
                            self.start_field(pos);
                        }
                        QUOTE => {
                            if !options.lenient_quotes {
                                self.set_error(pos, "unexpected quote in an unquoted field");
                            }
                            pos += 1;
                        }
                        ESCAPE => {
                            self.flags |= UNESCAPE;
                            self.mode = Mode::UnquotedEscape;
                            pos += 1;
                        }
                        _ => {
                            self.push_unquoted(input, pos, options);
                            return Ok(self.end_record(dialect, input, pos, eof));
                        }
                    }
                }
                Mode::UnquotedEscape => {
                    pos += 1;
                    self.mode = Mode::Unquoted;
                }
                Mode::Quoted => {
                    while pos < len && !matches!(dialect.class(input[pos]), QUOTE | ESCAPE) {
                        pos += 1;
                    }
                    let Some(&byte) = input.get(pos) else {
                        break;
                    };
                    if dialect.class(byte) == QUOTE {
                        self.mode = Mode::QuotedQuote;
                    } else {
                        self.flags |= UNESCAPE;
                        self.mode = Mode::QuotedEscape;
                    }
                    pos += 1;
                }
                Mode::QuotedEscape => {
                    pos += 1;
                    self.mode = Mode::Quoted;
                }
                Mode::QuotedQuote => {
                    if dialect.double_quote && dialect.class(input[pos]) == QUOTE {
                        self.flags |= UNESCAPE;
                        self.mode = Mode::Quoted;
                        pos += 1;
                    } else {
                        self.end = pos - 1;
                        self.mode = Mode::AfterQuoted;
                    }
                }
                Mode::AfterQuoted => {
                    let byte = input[pos];
                    match dialect.class(byte) {
                        DELIMITER => {
                            self.push_quoted(pos);
                            pos += 1;
                            self.start_field(pos);
                        }
                        TERMINATOR | CR => {
                            self.push_quoted(pos);
                            return Ok(self.end_record(dialect, input, pos, eof));
                        }
                        OTHER if options.trim && is_space(byte) => pos += 1,
                        _ => {
                            // the rest of the field is taken as it is, the
                            // quotes are removed when the field is decoded
                            if !options.lenient_quotes {
                                self.set_error(pos, "unexpected character after a quoted field");
                            }
                            self.flags |= UNESCAPE;
                            self.mode = Mode::Unquoted;
                        }
                    }
                }
                Mode::RecordStart => unreachable!("records are started above"),
            }
        }

        self.pos = pos;
        if !eof {
            if pos > options.max_record_len {
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    format!(
                        "record is longer than the maximum of {} bytes",
                        options.max_record_len
                    ),
                )
                .with_offset(0));
            }
            return Ok(Scan::Incomplete);
        }

        // the end of the input ends the record
        match self.mode {
            Mode::Comment => {
                self.mode = Mode::RecordStart;
                return Ok(Scan::Skip { consumed: len });
            }
            Mode::FieldStart => {
                self.start = len;
                self.push_unquoted(input, len, options);
            }
            Mode::Unquoted => self.push_unquoted(input, len, options),
            Mode::UnquotedEscape => {
                self.set_error(len - 1, "escape character at the end of the input");
                self.push_unquoted(input, len, options);
            }
            Mode::Quoted | Mode::QuotedEscape => {
                self.set_error(self.start - 1, "unterminated quoted field");
                self.end = len;
                self.push_quoted(len);
            }
            Mode::QuotedQuote => {
                self.end = len - 1;
                self.push_quoted(len);
            }
            Mode::AfterQuoted => self.push_quoted(len),
            Mode::RecordStart => unreachable!("records are started above"),
        }
        self.mode = Mode::RecordStart;
        Ok(Scan::Record {
            end: len,
            consumed: len,
        })
    }

    fn start_field(&mut self, pos: usize) {
        self.span_start = pos;
        self.flags = 0;
        self.mode = Mode::FieldStart;
    }

    fn set_error(&mut self, offset: usize, msg: &'static str) {
        if self.error.is_none() {
            self.error = Some((offset, msg));
        }
    }

    /// Adds an unquoted field that ends at `pos`.
    fn push_unquoted(&mut self, input: &[u8], pos: usize, options: Options) {
        let mut end = pos;
        if options.trim && self.flags & UNESCAPE == 0 {
            while end > self.start && is_space(input[end - 1]) {
                end -= 1;
            }
        }
        // for the rest of a field after the closing quote (with lenient
        // quotes) the start is after the opening quote
        self.fields.push(Field {
            span_start: self.span_start,
            span_end: pos,
            start: self.start,
            end,
            flags: self.flags,
        });
    }

    /// Adds a quoted field whose closing quote is at `self.end`.
    fn push_quoted(&mut self, pos: usize) {
        self.fields.push(Field {
            span_start: self.span_start,
            span_end: pos,
            start: self.start,
            end: self.end,
            flags: self.flags,
        });
    }

    /// Ends the record at the terminator at `pos` (or the end of the input).
    fn end_record(&mut self, dialect: &Dialect, input: &[u8], pos: usize, eof: bool) -> Scan {
        let consumed = self.terminator(dialect, input, pos, eof);
        self.mode = Mode::RecordStart;
        Scan::Record { end: pos, consumed }
    }

    /// Returns the end of the terminator at `pos`.
    fn terminator(&mut self, dialect: &Dialect, input: &[u8], pos: usize, eof: bool) -> usize {
        if pos >= input.len() {
            return input.len();
        }
        if dialect.class(input[pos]) == CR {
            match input.get(pos + 1) {
                Some(b'\n') => return pos + 2,
                None if !eof => self.pending_lf = true,
                _ => {}
            }
        }
        pos + 1
    }
}

#[inline(always)]
fn is_space(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

/// Decodes the escapes (and doubled quotes) of a field into `out`.
pub(crate) fn unescape(dialect: &Dialect, text: &[u8], quoted: bool, out: &mut Vec<u8>) {
    out.clear();
    let escape = dialect.escape.byte();
    let mut pos = 0;
    while pos < text.len() {
        let byte = text[pos];
        pos += 1;
        if Some(byte) == escape {
            match text.get(pos) {
                Some(&next) => {
                    pos += 1;
                    out.push(match (dialect.escape, next) {
                        (Escape::Backslash, b't') => b'\t',
                        (Escape::Backslash, b'n') => b'\n',
                        (Escape::Backslash, b'r') => b'\r',
                        (Escape::Backslash, b'0') => b'\0',
                        (_, next) => next,
                    });
                }
                // an escape at the end of the input is taken as it is
                None => out.push(byte),
            }
        } else if quoted && Some(byte) == dialect.quote {
            // doubled quotes are a quote, single quotes (which only exist
            // with lenient quotes) are dropped
            if dialect.double_quote && text.get(pos) == Some(&byte) {
                pos += 1;
                out.push(byte);
            }
        } else {
            out.push(byte);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialect() -> Dialect {
        Dialect::new(
            b',',
            Some(b'"'),
            true,
            Escape::None,
            Terminator::Newline,
            None,
        )
        .unwrap()
    }

    const OPTIONS: Options = Options {
        trim: false,
        skip_blank_lines: true,
        lenient_quotes: false,
        max_record_len: usize::MAX,
    };

    /// Scans all records of an input given in chunks of `size` bytes.
    fn scan_all(input: &[u8], size: usize) -> Vec<Vec<String>> {
        let dialect = dialect();
        let mut scanner = Scanner::default();
        let mut rv = Vec::new();
        let mut start = 0;
        let mut available = size.min(input.len());
        loop {
            let eof = available == input.len();
            let chunk = &input[start..available];
            match scanner.scan(&dialect, chunk, eof, OPTIONS).unwrap() {
                Scan::Record { end: _, consumed } => {
                    let mut out = Vec::new();
                    rv.push(
                        scanner
                            .fields
                            .iter()
                            .map(|field| {
                                let text = &chunk[field.start..field.end];
                                if field.flags & UNESCAPE != 0 {
                                    unescape(&dialect, text, field.flags & QUOTED != 0, &mut out);
                                    String::from_utf8(out.clone()).unwrap()
                                } else {
                                    String::from_utf8(text.to_vec()).unwrap()
                                }
                            })
                            .collect(),
                    );
                    start += consumed;
                }
                Scan::Skip { consumed } => start += consumed,
                Scan::Incomplete => available = (available + size).min(input.len()),
                Scan::End => return rv,
            }
        }
    }

    #[test]
    fn test_scan_in_chunks() {
        let input = b"a,\"b\"\"c\",d\r\n\r\n\"x\ny\",,z\ry";
        let expected = vec![
            vec!["a".to_string(), "b\"c".into(), "d".into()],
            vec!["x\ny".into(), "".into(), "z".into()],
            vec!["y".into()],
        ];
        for size in 1..=input.len() {
            assert_eq!(scan_all(input, size), expected, "size {}", size);
        }
    }

    #[test]
    fn test_has_special() {
        let dialect = dialect();
        assert!(!dialect.has_special(b""));
        assert!(!dialect.has_special(b"abcdefghijklmnop\x80\xff"));
        for special in *b",\"\n\r" {
            for len in 1..20 {
                for pos in 0..len {
                    let mut text = vec![b'x'; len];
                    text[pos] = special;
                    assert!(dialect.has_special(&text), "{:?}", text);
                }
            }
        }
    }

    #[test]
    fn test_invalid_dialects() {
        let err = Dialect::new(
            b',',
            Some(b','),
            true,
            Escape::None,
            Terminator::Newline,
            None,
        )
        .unwrap_err();
        assert_eq!(
            err.message(),
            "the quote character conflicts with another special character"
        );
        let err =
            Dialect::new(0xa7, None, true, Escape::None, Terminator::Newline, None).unwrap_err();
        assert_eq!(err.message(), "the delimiter must be an ASCII character");
    }
}
