//! The JSON parser.
//!
//! The parser is a state machine which can be suspended between tokens: if
//! the input ends within a token and more input can follow, the parser
//! returns how much of the input it consumed (up to the start of the token)
//! and continues with more input.  With the complete input (`eof`) it
//! parses a value in one go.  The state of the current container is held
//! in locals while parsing and only stored in the parser when it's
//! suspended.
use std::str;

use deser::de::DeserializeDriver;
use deser::ext::{ExtValue, Number as ExactNumber};
use deser::{Atom, Error, ErrorKind, Event, State};

use crate::scan::{is_ascii, skip_to_escape, validate_utf8_slice};

/// A parsed string.
pub(crate) enum Str<'a, 'b> {
    /// The string is a slice of the input.
    Borrowed(&'a str),
    /// The string was unescaped into the scratch buffer.
    Scratch(&'b str),
}

pub(crate) enum Number<'a> {
    I64(i64),
    /// An integer that does not fit into 64 bits but into 128 bits.  This
    /// holds the (validated) textual representation.
    BigInt(&'a str),
    U64(u64),
    /// A float whose text is the shortest representation of its value.
    F64(f64),
    /// A float (or an integer which does not fit into 128 bits) whose text
    /// cannot be recovered from the value.  This is passed on as number
    /// extension value if exact numbers are enabled.
    Literal(f64),
}

impl Number<'_> {
    /// Returns the value of a float.
    fn into_f64(self) -> f64 {
        match self {
            Number::F64(value) | Number::Literal(value) => value,
            _ => unreachable!("not a float"),
        }
    }
}

macro_rules! overflow {
    ($a:ident * 10 + $b:ident, $c:expr) => {
        $a >= $c / 10 && ($a > $c / 10 || $b > $c % 10)
    };
}

/// Receives the events of the parser.
///
/// Strings which are slices of the input are passed to
/// [`emit_input`](Self::emit_input), which can pass them on borrowed if the
/// input lives long enough.
pub(crate) trait Out<'i> {
    fn state_mut(&mut self) -> &mut State;
    fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error>;
    fn emit_input(&mut self, value: &'i str) -> Result<(), Error>;
}

/// Passes strings of the input on borrowed.
pub(crate) struct Borrowing<'a, 'd, 'i>(pub &'a mut DeserializeDriver<'d, 'i>);

impl<'i> Out<'i> for Borrowing<'_, '_, 'i> {
    #[inline(always)]
    fn state_mut(&mut self) -> &mut State {
        self.0.state_mut()
    }

    #[inline(always)]
    fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error> {
        self.0.emit(event)
    }

    #[inline(always)]
    fn emit_input(&mut self, value: &'i str) -> Result<(), Error> {
        self.0.emit_borrowed(value)
    }
}

/// Passes strings of the input on as data that is only valid for the call.
///
/// This is used when the input does not outlive the deserialization (for
/// instance a buffer that is refilled).
pub(crate) struct Copying<'a, 'd, 'de>(pub &'a mut DeserializeDriver<'d, 'de>);

impl<'i> Out<'i> for Copying<'_, '_, '_> {
    #[inline(always)]
    fn state_mut(&mut self) -> &mut State {
        self.0.state_mut()
    }

    #[inline(always)]
    fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error> {
        self.0.emit(event)
    }

    #[inline(always)]
    fn emit_input(&mut self, value: &'i str) -> Result<(), Error> {
        self.0.emit(value)
    }
}

/// Discards the events.
///
/// This is used to skip the rest of a value after an error.
pub(crate) struct Discard(pub State);

impl<'i> Out<'i> for Discard {
    #[inline(always)]
    fn state_mut(&mut self) -> &mut State {
        &mut self.0
    }

    #[inline(always)]
    fn emit<'e, E: Into<Event<'e>>>(&mut self, _event: E) -> Result<(), Error> {
        Ok(())
    }

    #[inline(always)]
    fn emit_input(&mut self, _value: &'i str) -> Result<(), Error> {
        Ok(())
    }
}

/// The options of the parser.
#[derive(Clone, Copy)]
pub(crate) struct Options {
    /// The input is a byte slice which needs to be validated as UTF-8.
    pub validate_utf8: bool,
    pub exact_numbers: bool,
}

/// The result of [`Parser::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Progress {
    /// The value is complete, it ends at the given position.
    Done(usize),
    /// More input is needed.  The input up to the given position was
    /// consumed, the parser continues with the input after it.
    NeedMore(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Container {
    Top,
    Seq,
    Map,
}

/// What the parser expects next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Expect {
    /// A value.
    Value,
    /// A value or the end of a sequence (after `[`).
    ValueOrEnd,
    /// A map key.
    Key,
    /// A map key or the end of a map (after `{`).
    KeyOrEnd,
    /// The colon after a key.
    Colon,
    /// A comma or the end of the container after a value.
    AfterValue,
}

/// An incomplete string at the start of the input.
///
/// The positions are relative to the opening quote, the scratch buffer holds
/// the string up to `copied`.
#[derive(Clone, Copy, Debug)]
struct PartialString {
    /// Where scanning continues.
    scanned: usize,
    /// The first byte which was not copied into the scratch buffer.
    copied: usize,
}

/// A JSON parser which can be suspended between tokens.
#[derive(Debug)]
pub(crate) struct Parser {
    // the outer containers, the current one is held in `container`
    stack: Vec<Container>,
    container: Container,
    expect: Expect,
    scratch: Vec<u8>,
    partial: Option<PartialString>,
    // the last error was an error of a sink, the rest of the value
    // continues at the position
    recoverable: Option<usize>,
}

impl Default for Parser {
    fn default() -> Parser {
        Parser {
            stack: Vec::new(),
            container: Container::Top,
            expect: Expect::Value,
            scratch: Vec::new(),
            partial: None,
            recoverable: None,
        }
    }
}

/// Emits an event with its input range.
macro_rules! emit {
    ($out:expr, $base:expr, $start:expr, $end:expr, $event:expr) => {{
        $out.state_mut()
            .set_input_range($base + $start, $base + $end);
        $out.emit($event)
    }};
}

impl Parser {
    /// Returns `true` if the parser is between values.
    pub(crate) fn is_idle(&self) -> bool {
        self.expect == Expect::Value && self.container == Container::Top && self.partial.is_none()
    }

    /// Resets the parser to parse a new value.
    ///
    /// This is needed after an error.
    pub(crate) fn reset(&mut self) {
        self.stack.clear();
        self.container = Container::Top;
        self.expect = Expect::Value;
        self.partial = None;
        self.recoverable = None;
    }

    /// Returns where the rest of the value continues if the last error was
    /// an error of a sink.
    ///
    /// In that case the state is as if the event was accepted and the rest
    /// of the value can be parsed from the position (for instance with
    /// [`Discard`]).
    pub(crate) fn recoverable(&self) -> Option<usize> {
        self.recoverable
    }

    /// Parses a value (or continues it) from `input[pos..]`.
    ///
    /// `eof` is `true` if no input follows, `base` is the offset of the
    /// input in the stream: the input ranges of the events and the offsets
    /// of errors refer to the stream.  After an error the parser has to be
    /// [reset](Self::reset).
    #[inline(always)]
    pub(crate) fn parse<'i, O: Out<'i>>(
        &mut self,
        input: &'i [u8],
        pos: usize,
        eof: bool,
        base: usize,
        options: Options,
        out: &mut O,
    ) -> Result<Progress, Error> {
        self.recoverable = None;
        let mut cur = Cursor {
            input,
            pos,
            validate_utf8: options.validate_utf8,
            hit_end: false,
            partial: (0, 0),
        };
        match self.run(&mut cur, eof, base, options.exact_numbers, out) {
            Ok(progress) => Ok(progress),
            Err(err) if err.offset().is_none() => Err(err.with_offset(base + cur.pos)),
            Err(err) => Err(err),
        }
    }

    #[inline(always)]
    fn run<'i, O: Out<'i>>(
        &mut self,
        cur: &mut Cursor<'i>,
        eof: bool,
        base: usize,
        exact_numbers: bool,
        out: &mut O,
    ) -> Result<Progress, Error> {
        let input = cur.input;
        let stack = &mut self.stack;
        let scratch = &mut self.scratch;
        let mut container = self.container;
        let mut partial = self.partial.take();

        // stores the state and returns that more input is needed
        macro_rules! suspend {
            ($consumed:expr, $expect:expr) => {{
                self.container = container;
                self.expect = $expect;
                return Ok(Progress::NeedMore($consumed));
            }};
        }

        // fails with an error of a sink.  The state is stored as if the
        // event was accepted so that the rest of the value can be skipped.
        macro_rules! sink {
            ($rv:expr, $expect:expr) => {
                if let Err(err) = $rv {
                    self.container = container;
                    self.expect = $expect;
                    self.recoverable = Some(cur.pos);
                    return Err(err);
                }
            };
        }

        // skips whitespace and returns the next byte, suspends at the end
        // of the input.
        macro_rules! next_byte {
            ($expect:expr) => {
                match cur.parse_whitespace() {
                    Some(byte) => byte,
                    None if eof => return Err(eof_error()),
                    None => suspend!(cur.pos, $expect),
                }
            };
        }

        // parses a string after its opening quote, suspends if it's
        // incomplete.
        macro_rules! string {
            ($start:expr, $expect:expr) => {{
                cur.hit_end = false;
                let rv = cur.parse_str(scratch, $start, partial.take());
                if cur.hit_end && !eof {
                    self.partial = Some(PartialString {
                        scanned: cur.partial.0 - $start,
                        copied: cur.partial.1 - $start,
                    });
                    suspend!($start, $expect)
                }
                rv?
            }};
        }

        // closes the current container
        macro_rules! close {
            ($start:expr, $event:expr) => {{
                container = stack.pop().unwrap_or(Container::Top);
                sink!(
                    emit!(out, base, $start, cur.pos, $event),
                    Expect::AfterValue
                );
            }};
        }

        // parses a map key and the colon after it
        macro_rules! key {
            ($byte:expr) => {{
                let start = cur.pos;
                if $byte != b'"' {
                    return Err(token_error(base + start, "expected map key"));
                }
                cur.bump();
                match string!(start, Expect::Key) {
                    Str::Borrowed(key) => {
                        out.state_mut()
                            .set_input_range(base + start, base + cur.pos);
                        sink!(out.emit_input(key), Expect::Colon)
                    }
                    Str::Scratch(key) => sink!(
                        emit!(out, base, start, cur.pos, Event::from(key)),
                        Expect::Colon
                    ),
                }
                colon!();
            }};
        }

        macro_rules! colon {
            () => {
                match next_byte!(Expect::Colon) {
                    b':' => cur.bump(),
                    _ => return Err(Error::new(ErrorKind::Unexpected, "expected colon")),
                }
            };
        }

        // after `{`: the map ends or the first key follows.  Evaluates to
        // `true` if a value follows.
        macro_rules! open_map {
            () => {{
                let byte = if partial.is_some() {
                    b'"'
                } else {
                    next_byte!(Expect::KeyOrEnd)
                };
                if byte == b'}' {
                    let start = cur.pos;
                    cur.bump();
                    close!(start, Event::MapEnd);
                    false
                } else {
                    key!(byte);
                    true
                }
            }};
        }

        // after `[`: the sequence ends or the first value follows.
        // Evaluates to `true` if a value follows.
        macro_rules! open_seq {
            () => {{
                if next_byte!(Expect::ValueOrEnd) == b']' {
                    let start = cur.pos;
                    cur.bump();
                    close!(start, Event::SeqEnd);
                    false
                } else {
                    true
                }
            }};
        }

        // continue where the parser was suspended
        let mut skip_value = match self.expect {
            Expect::Value => false,
            Expect::ValueOrEnd => !open_seq!(),
            Expect::KeyOrEnd => !open_map!(),
            Expect::Key => {
                let byte = if partial.is_some() {
                    b'"'
                } else {
                    next_byte!(Expect::Key)
                };
                key!(byte);
                false
            }
            Expect::Colon => {
                colon!();
                false
            }
            Expect::AfterValue => true,
        };

        'value: loop {
            if !skip_value {
                let byte = if partial.is_some() {
                    b'"'
                } else {
                    next_byte!(Expect::Value)
                };
                let start = cur.pos;
                cur.bump();
                cur.hit_end = false;
                match byte {
                    b'"' => match string!(start, Expect::Value) {
                        Str::Borrowed(val) => {
                            out.state_mut()
                                .set_input_range(base + start, base + cur.pos);
                            sink!(out.emit_input(val), Expect::AfterValue)
                        }
                        Str::Scratch(val) => sink!(
                            emit!(out, base, start, cur.pos, Event::from(val)),
                            Expect::AfterValue
                        ),
                    },
                    b'0'..=b'9' | b'-' => {
                        let rv = if byte == b'-' {
                            let first_digit = cur.next_or_nul();
                            cur.parse_integer(false, first_digit)
                        } else {
                            cur.parse_integer(true, byte)
                        };
                        // the number might continue
                        if cur.hit_end && !eof {
                            suspend!(start, Expect::Value)
                        }
                        let number = rv?;
                        out.state_mut()
                            .set_input_range(base + start, base + cur.pos);
                        sink!(
                            emit_number(out, number, input, exact_numbers, start, cur.pos),
                            Expect::AfterValue
                        )
                    }
                    b'n' | b't' | b'f' => {
                        let (rest, event): (&[u8], _) = match byte {
                            b'n' => (b"ull", Event::Atom(Atom::Null)),
                            b't' => (b"rue", Event::from(true)),
                            _ => (b"alse", Event::from(false)),
                        };
                        let rv = cur.parse_ident(rest);
                        if cur.hit_end && !eof {
                            suspend!(start, Expect::Value)
                        }
                        rv?;
                        sink!(emit!(out, base, start, cur.pos, event), Expect::AfterValue)
                    }
                    b'{' => {
                        stack.push(container);
                        container = Container::Map;
                        sink!(
                            emit!(out, base, start, cur.pos, Event::map_start()),
                            Expect::KeyOrEnd
                        );
                        if open_map!() {
                            continue 'value;
                        }
                    }
                    b'[' => {
                        stack.push(container);
                        container = Container::Seq;
                        sink!(
                            emit!(out, base, start, cur.pos, Event::seq_start()),
                            Expect::ValueOrEnd
                        );
                        if open_seq!() {
                            continue 'value;
                        }
                    }
                    b',' => return Err(token_error(base + start, "unexpected comma")),
                    b':' => return Err(token_error(base + start, "unexpected colon")),
                    b']' | b'}' => return Err(token_error(base + start, "expected a value")),
                    _ => return Err(token_error(base + start, "unexpected character")),
                }
            }
            skip_value = false;

            // a value was completed, either the container ends or the next
            // value follows.
            loop {
                let close = match container {
                    Container::Top => {
                        self.container = Container::Top;
                        self.expect = Expect::Value;
                        return Ok(Progress::Done(cur.pos));
                    }
                    Container::Map => b'}',
                    Container::Seq => b']',
                };
                match next_byte!(Expect::AfterValue) {
                    b',' => {
                        cur.bump();
                        if container == Container::Map {
                            let byte = next_byte!(Expect::Key);
                            key!(byte);
                        }
                        continue 'value;
                    }
                    byte if byte == close => {
                        let start = cur.pos;
                        cur.bump();
                        close!(
                            start,
                            if close == b'}' {
                                Event::MapEnd
                            } else {
                                Event::SeqEnd
                            }
                        );
                    }
                    b']' | b'}' => {
                        return Err(Error::new(
                            ErrorKind::Unexpected,
                            if container == Container::Map {
                                "unexpected end of seq"
                            } else {
                                "unexpected end of map"
                            },
                        ));
                    }
                    _ => {
                        return Err(Error::new(ErrorKind::Unexpected, "expected a comma"));
                    }
                }
            }
        }
    }
}

#[cold]
fn eof_error() -> Error {
    Error::new(ErrorKind::EndOfFile, "unexpected end of file")
}

/// Reads tokens from the input.
pub(crate) struct Cursor<'a> {
    pub(crate) input: &'a [u8],
    pub(crate) pos: usize,
    // `true` if the input is a byte slice which needs to be validated
    validate_utf8: bool,
    // `true` if a token ran into the end of the input
    hit_end: bool,
    // where an incomplete string continues (scan position and copy position)
    partial: (usize, usize),
}

impl<'a> Cursor<'a> {
    /// Creates a cursor to skip whitespace.
    pub(crate) fn new(input: &'a [u8], pos: usize) -> Cursor<'a> {
        Cursor {
            input,
            pos,
            validate_utf8: false,
            hit_end: false,
            partial: (0, 0),
        }
    }

    /// Parses a string, the cursor is after the opening quote at `start`.
    ///
    /// An incomplete string continues where it stopped.  If the string is
    /// incomplete, `partial` holds where it continues.
    #[inline(never)]
    fn parse_str<'b>(
        &mut self,
        buffer: &'b mut Vec<u8>,
        start: usize,
        resume: Option<PartialString>,
    ) -> Result<Str<'a, 'b>, Error> {
        let validate_utf8 = self.validate_utf8;
        fn result(validate_utf8: bool, bytes: &[u8]) -> Result<&str, Error> {
            // Strings in byte slices are validated here.  Bytes outside of
            // strings are only accepted if they are ASCII so this validates
            // the entire input.  The decoded escapes are valid UTF-8 and
            // cannot complete an invalid sequence before them as they never
            // start with a continuation byte, so validating the unescaped
            // string is equivalent to validating the raw one.
            if validate_utf8 && !is_ascii(bytes) && !validate_utf8_slice(bytes) {
                return Err(Error::new(ErrorKind::Unexpected, "invalid utf-8 in string"));
            }
            // SAFETY: the input is valid UTF-8 as it comes from a `&str` or
            // was validated above.  The borrowed slices start and end at
            // ASCII characters (quotes and backslashes) so they are valid
            // UTF-8 too.  The \u-escapes are validated when they are decoded
            // into the buffer.
            Ok(unsafe { str::from_utf8_unchecked(bytes) })
        }

        // Index of the first byte not yet copied into the scratch space.
        let mut copied = match resume {
            Some(resume) => {
                self.pos = start + resume.scanned;
                start + resume.copied
            }
            None => {
                buffer.clear();
                self.pos
            }
        };

        loop {
            self.pos = skip_to_escape(self.input, self.pos);
            if self.pos == self.input.len() {
                self.hit_end = true;
                self.partial = (self.pos, copied);
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    "unexpected end of string",
                ));
            }
            match self.input[self.pos] {
                b'"' => {
                    if buffer.is_empty() {
                        // Fast path: return a slice of the raw JSON without any
                        // copying.
                        let input = self.input;
                        let borrowed = &input[copied..self.pos];
                        self.pos += 1;
                        return result(validate_utf8, borrowed).map(Str::Borrowed);
                    } else {
                        buffer.extend_from_slice(&self.input[copied..self.pos]);
                        self.pos += 1;
                        return result(validate_utf8, buffer).map(Str::Scratch);
                    }
                }
                b'\\' => {
                    buffer.extend_from_slice(&self.input[copied..self.pos]);
                    let escape = self.pos;
                    self.pos += 1;
                    if let Err(err) = self.parse_escape(buffer) {
                        // an incomplete escape is parsed again
                        self.partial = (escape, escape);
                        return Err(err);
                    }
                    copied = self.pos;
                }
                _ => {
                    return Err(Error::new(
                        ErrorKind::Unexpected,
                        "unexpected character in string",
                    ));
                }
            }
        }
    }

    #[inline]
    fn next(&mut self) -> Option<u8> {
        if self.pos < self.input.len() {
            let ch = self.input[self.pos];
            self.pos += 1;
            Some(ch)
        } else {
            self.hit_end = true;
            None
        }
    }

    fn next_or_nul(&mut self) -> u8 {
        self.next().unwrap_or(b'\0')
    }

    #[inline]
    fn peek(&mut self) -> Option<u8> {
        if self.pos < self.input.len() {
            Some(self.input[self.pos])
        } else {
            self.hit_end = true;
            None
        }
    }

    fn peek_or_nul(&mut self) -> u8 {
        self.peek().unwrap_or(b'\0')
    }

    #[inline]
    fn bump(&mut self) {
        self.pos += 1;
    }

    fn next_or_eof(&mut self) -> Result<u8, Error> {
        self.next()
            .ok_or_else(|| Error::new(ErrorKind::EndOfFile, "unexpected end of file"))
    }

    /// Parses a JSON escape sequence and appends it into the scratch space. Assumes
    /// the previous byte read was a backslash.
    fn parse_escape(&mut self, buffer: &mut Vec<u8>) -> Result<(), Error> {
        let ch = self.next_or_eof()?;

        match ch {
            b'"' => buffer.push(b'"'),
            b'\\' => buffer.push(b'\\'),
            b'/' => buffer.push(b'/'),
            b'b' => buffer.push(b'\x08'),
            b'f' => buffer.push(b'\x0c'),
            b'n' => buffer.push(b'\n'),
            b'r' => buffer.push(b'\r'),
            b't' => buffer.push(b'\t'),
            b'u' => {
                let c = match self.decode_hex_escape()? {
                    0xDC00..=0xDFFF => {
                        return Err(Error::new(ErrorKind::Unexpected, "invalid string"));
                    }

                    // Non-BMP characters are encoded as a sequence of
                    // two hex escapes, representing UTF-16 surrogates.
                    n1 @ 0xD800..=0xDBFF => {
                        if self.next_or_eof()? != b'\\' {
                            return Err(Error::new(ErrorKind::Unexpected, "invalid string"));
                        }
                        if self.next_or_eof()? != b'u' {
                            return Err(Error::new(ErrorKind::Unexpected, "invalid string"));
                        }

                        let n2 = self.decode_hex_escape()?;

                        if !(0xDC00..=0xDFFF).contains(&n2) {
                            return Err(Error::new(ErrorKind::Unexpected, "invalid string"));
                        }

                        let n = (u32::from(n1 - 0xD800) << 10 | u32::from(n2 - 0xDC00)) + 0x1_0000;

                        match char::from_u32(n) {
                            Some(c) => c,
                            None => {
                                return Err(Error::new(ErrorKind::Unexpected, "invalid string"));
                            }
                        }
                    }

                    n => match char::from_u32(u32::from(n)) {
                        Some(c) => c,
                        None => {
                            return Err(Error::new(ErrorKind::Unexpected, "invalid string"));
                        }
                    },
                };

                buffer.extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
            }
            _ => {
                return Err(Error::new(ErrorKind::Unexpected, "invalid string"));
            }
        }

        Ok(())
    }

    fn decode_hex_escape(&mut self) -> Result<u16, Error> {
        let mut n = 0;
        for _ in 0..4 {
            n = match self.next_or_eof()? {
                c @ b'0'..=b'9' => n * 16_u16 + u16::from(c - b'0'),
                b'a' | b'A' => n * 16_u16 + 10_u16,
                b'b' | b'B' => n * 16_u16 + 11_u16,
                b'c' | b'C' => n * 16_u16 + 12_u16,
                b'd' | b'D' => n * 16_u16 + 13_u16,
                b'e' | b'E' => n * 16_u16 + 14_u16,
                b'f' | b'F' => n * 16_u16 + 15_u16,
                _ => {
                    return Err(Error::new(ErrorKind::Unexpected, "invalid hex escape"));
                }
            };
        }
        Ok(n)
    }

    #[inline(always)]
    pub(crate) fn parse_whitespace(&mut self) -> Option<u8> {
        const SPACES: u64 = u64::from_ne_bytes([b' '; 8]);
        let input = self.input;
        let mut pos = self.pos;
        loop {
            // indented JSON contains long runs of spaces, skip them a word
            // at a time.
            if pos + 8 <= input.len() {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&input[pos..pos + 8]);
                if u64::from_ne_bytes(bytes) == SPACES {
                    pos += 8;
                    continue;
                }
            }
            match input.get(pos) {
                Some(b' ' | b'\n' | b'\t' | b'\r') => pos += 1,
                Some(&byte) => {
                    self.pos = pos;
                    return Some(byte);
                }
                None => {
                    self.pos = pos;
                    self.hit_end = true;
                    return None;
                }
            }
        }
    }

    fn parse_ident(&mut self, ident: &[u8]) -> Result<(), Error> {
        for expected in ident {
            match self.next() {
                None => {
                    return Err(Error::new(ErrorKind::EndOfFile, "unexpected end of file"));
                }
                Some(next) => {
                    if next != *expected {
                        return Err(Error::new(ErrorKind::Unexpected, "unexpected character"));
                    }
                }
            }
        }
        Ok(())
    }

    fn parse_integer(&mut self, nonnegative: bool, first_digit: u8) -> Result<Number<'a>, Error> {
        match first_digit {
            b'0' => match self.peek_or_nul() {
                b'0'..=b'9' => Err(Error::new(
                    ErrorKind::Unexpected,
                    "only a single leading 0 is allowed",
                )),
                _ => self.parse_number(nonnegative, 0),
            },
            c @ b'1'..=b'9' => {
                let mut res = u64::from(c - b'0');

                loop {
                    match self.peek_or_nul() {
                        c @ b'0'..=b'9' => {
                            self.bump();
                            let digit = u64::from(c - b'0');

                            // We need to be careful with overflow. If we can, try to keep the
                            // number as a `u64` until we grow too large. At that point, switch to
                            // parsing the value as a `f64`.
                            if overflow!(res * 10 + digit, u64::MAX) {
                                return self.parse_overflowing_integer(nonnegative, res);
                            }

                            res = res * 10 + digit;
                        }
                        _ => {
                            return self.parse_number(nonnegative, res);
                        }
                    }
                }
            }
            _ => Err(Error::new(ErrorKind::Unexpected, "invalid integer")),
        }
    }

    /// Returns the text of the number that was just parsed.
    ///
    /// This only works for integers as it scans backwards for digits.
    fn number_text(&self, nonnegative: bool) -> &'a str {
        let input = self.input;
        let mut start = self.pos;
        while start > 0 && input[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if !nonnegative {
            start -= 1;
        }
        // the input is valid utf-8 as it was created from a string
        str::from_utf8(&input[start..self.pos]).unwrap()
    }

    /// Continues parsing an integer which no longer fits into 64 bits.
    ///
    /// If the number turns out to be an integer that fits into 128 bits it's
    /// passed on as big integer.  Otherwise it's parsed as float.
    #[cold]
    fn parse_overflowing_integer(
        &mut self,
        nonnegative: bool,
        significand: u64,
    ) -> Result<Number<'a>, Error> {
        let digits_start = self.pos - 1;
        let float = self.parse_long_integer(
            nonnegative,
            significand,
            1, // significand * 10^1
        )?;
        let is_integer = self.input[digits_start..self.pos]
            .iter()
            .all(|c| c.is_ascii_digit());
        if is_integer {
            let text = self.number_text(nonnegative);
            let fits = if nonnegative {
                text.parse::<u128>().is_ok()
            } else {
                text.parse::<i128>().is_ok()
            };
            if fits {
                return Ok(Number::BigInt(text));
            }
        }
        Ok(Number::Literal(float))
    }

    fn parse_long_integer(
        &mut self,
        nonnegative: bool,
        significand: u64,
        mut exponent: i32,
    ) -> Result<f64, Error> {
        loop {
            match self.peek_or_nul() {
                b'0'..=b'9' => {
                    self.bump();
                    // This could overflow... if your integer is gigabytes long.
                    // Ignore that possibility.
                    exponent += 1;
                }
                b'.' => {
                    return self
                        .parse_decimal(nonnegative, significand, exponent)
                        .map(Number::into_f64);
                }
                b'e' | b'E' => {
                    return self.parse_exponent(nonnegative, significand, exponent);
                }
                _ => {
                    return f64_from_parts(nonnegative, significand, exponent);
                }
            }
        }
    }

    fn parse_number(&mut self, nonnegative: bool, significand: u64) -> Result<Number<'a>, Error> {
        match self.peek_or_nul() {
            b'.' => self.parse_decimal(nonnegative, significand, 0),
            b'e' | b'E' => self
                .parse_exponent(nonnegative, significand, 0)
                .map(Number::Literal),
            _ => {
                Ok(if nonnegative {
                    Number::U64(significand)
                } else {
                    let neg = (significand as i64).wrapping_neg();

                    // Values below i64::MIN are passed on as 128 bit integers.
                    if neg > 0 {
                        Number::BigInt(self.number_text(false))
                    } else {
                        Number::I64(neg)
                    }
                })
            }
        }
    }

    /// Parses the fraction of a number.
    ///
    /// This returns a [`Number::Literal`] unless the text of the number is
    /// the shortest representation of its value.
    fn parse_decimal(
        &mut self,
        nonnegative: bool,
        mut significand: u64,
        starting_exp: i32,
    ) -> Result<Number<'a>, Error> {
        self.bump();

        let mut exponent = starting_exp;
        let mut at_least_one_digit = false;
        let mut overflowed = false;
        while let c @ b'0'..=b'9' = self.peek_or_nul() {
            self.bump();
            let digit = u64::from(c - b'0');
            at_least_one_digit = true;

            if overflow!(significand * 10 + digit, u64::MAX) {
                // The next multiply/add would overflow, so just ignore all
                // further digits.
                while let b'0'..=b'9' = self.peek_or_nul() {
                    self.bump();
                }
                overflowed = true;
                break;
            }

            significand = significand * 10 + digit;
            exponent -= 1;
        }

        if !at_least_one_digit {
            return Err(Error::new(ErrorKind::Unexpected, "expected a digit"));
        }

        match self.peek_or_nul() {
            b'e' | b'E' => self
                .parse_exponent(nonnegative, significand, exponent)
                .map(Number::Literal),
            _ => {
                let value = f64_from_parts(nonnegative, significand, exponent)?;
                Ok(
                    if !overflowed
                        && starting_exp == 0
                        && is_shortest_repr(significand, exponent.unsigned_abs())
                    {
                        Number::F64(value)
                    } else {
                        Number::Literal(value)
                    },
                )
            }
        }
    }

    fn parse_exponent(
        &mut self,
        nonnegative: bool,
        significand: u64,
        starting_exp: i32,
    ) -> Result<f64, Error> {
        self.bump();

        let positive_exp = match self.peek_or_nul() {
            b'+' => {
                self.bump();
                true
            }
            b'-' => {
                self.bump();
                false
            }
            _ => true,
        };

        let mut exp = match self.next_or_nul() {
            c @ b'0'..=b'9' => i32::from(c - b'0'),
            _ => {
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    "expected digit after exponent",
                ));
            }
        };

        while let c @ b'0'..=b'9' = self.peek_or_nul() {
            self.bump();
            let digit = i32::from(c - b'0');

            if overflow!(exp * 10 + digit, i32::MAX) {
                return self.parse_exponent_overflow(nonnegative, significand, positive_exp);
            }

            exp = exp * 10 + digit;
        }

        let final_exp = if positive_exp {
            starting_exp.saturating_add(exp)
        } else {
            starting_exp.saturating_sub(exp)
        };

        f64_from_parts(nonnegative, significand, final_exp)
    }

    // This cold code should not be inlined into the middle of the hot
    // exponent-parsing loop above.
    #[cold]
    #[inline(never)]
    fn parse_exponent_overflow(
        &mut self,
        nonnegative: bool,
        significand: u64,
        positive_exp: bool,
    ) -> Result<f64, Error> {
        // Error instead of +/- infinity.
        if significand != 0 && positive_exp {
            return Err(Error::new(ErrorKind::Unexpected, "infinity takes no sign"));
        }

        while let b'0'..=b'9' = self.peek_or_nul() {
            self.bump();
        }
        Ok(if nonnegative { 0.0 } else { -0.0 })
    }
}

fn f64_from_parts(nonnegative: bool, significand: u64, mut exponent: i32) -> Result<f64, Error> {
    let mut f = significand as f64;
    loop {
        match POW10.get(exponent.unsigned_abs() as usize) {
            Some(&pow) => {
                if exponent >= 0 {
                    f *= pow;
                    if f.is_infinite() {
                        return Err(Error::new(ErrorKind::OutOfRange, "infinite float"));
                    }
                } else {
                    f /= pow;
                }
                break;
            }
            None => {
                if f == 0.0 {
                    break;
                }
                if exponent >= 0 {
                    return Err(Error::new(ErrorKind::Unexpected, "unexpected float"));
                }
                f /= 1e308;
                exponent += 308;
            }
        }
    }
    Ok(if nonnegative { f } else { -f })
}

// Clippy bug: https://github.com/rust-lang/rust-clippy/issues/5201
#[allow(clippy::excessive_precision)]
static POW10: [f64; 309] = [
    1e000, 1e001, 1e002, 1e003, 1e004, 1e005, 1e006, 1e007, 1e008, 1e009, //
    1e010, 1e011, 1e012, 1e013, 1e014, 1e015, 1e016, 1e017, 1e018, 1e019, //
    1e020, 1e021, 1e022, 1e023, 1e024, 1e025, 1e026, 1e027, 1e028, 1e029, //
    1e030, 1e031, 1e032, 1e033, 1e034, 1e035, 1e036, 1e037, 1e038, 1e039, //
    1e040, 1e041, 1e042, 1e043, 1e044, 1e045, 1e046, 1e047, 1e048, 1e049, //
    1e050, 1e051, 1e052, 1e053, 1e054, 1e055, 1e056, 1e057, 1e058, 1e059, //
    1e060, 1e061, 1e062, 1e063, 1e064, 1e065, 1e066, 1e067, 1e068, 1e069, //
    1e070, 1e071, 1e072, 1e073, 1e074, 1e075, 1e076, 1e077, 1e078, 1e079, //
    1e080, 1e081, 1e082, 1e083, 1e084, 1e085, 1e086, 1e087, 1e088, 1e089, //
    1e090, 1e091, 1e092, 1e093, 1e094, 1e095, 1e096, 1e097, 1e098, 1e099, //
    1e100, 1e101, 1e102, 1e103, 1e104, 1e105, 1e106, 1e107, 1e108, 1e109, //
    1e110, 1e111, 1e112, 1e113, 1e114, 1e115, 1e116, 1e117, 1e118, 1e119, //
    1e120, 1e121, 1e122, 1e123, 1e124, 1e125, 1e126, 1e127, 1e128, 1e129, //
    1e130, 1e131, 1e132, 1e133, 1e134, 1e135, 1e136, 1e137, 1e138, 1e139, //
    1e140, 1e141, 1e142, 1e143, 1e144, 1e145, 1e146, 1e147, 1e148, 1e149, //
    1e150, 1e151, 1e152, 1e153, 1e154, 1e155, 1e156, 1e157, 1e158, 1e159, //
    1e160, 1e161, 1e162, 1e163, 1e164, 1e165, 1e166, 1e167, 1e168, 1e169, //
    1e170, 1e171, 1e172, 1e173, 1e174, 1e175, 1e176, 1e177, 1e178, 1e179, //
    1e180, 1e181, 1e182, 1e183, 1e184, 1e185, 1e186, 1e187, 1e188, 1e189, //
    1e190, 1e191, 1e192, 1e193, 1e194, 1e195, 1e196, 1e197, 1e198, 1e199, //
    1e200, 1e201, 1e202, 1e203, 1e204, 1e205, 1e206, 1e207, 1e208, 1e209, //
    1e210, 1e211, 1e212, 1e213, 1e214, 1e215, 1e216, 1e217, 1e218, 1e219, //
    1e220, 1e221, 1e222, 1e223, 1e224, 1e225, 1e226, 1e227, 1e228, 1e229, //
    1e230, 1e231, 1e232, 1e233, 1e234, 1e235, 1e236, 1e237, 1e238, 1e239, //
    1e240, 1e241, 1e242, 1e243, 1e244, 1e245, 1e246, 1e247, 1e248, 1e249, //
    1e250, 1e251, 1e252, 1e253, 1e254, 1e255, 1e256, 1e257, 1e258, 1e259, //
    1e260, 1e261, 1e262, 1e263, 1e264, 1e265, 1e266, 1e267, 1e268, 1e269, //
    1e270, 1e271, 1e272, 1e273, 1e274, 1e275, 1e276, 1e277, 1e278, 1e279, //
    1e280, 1e281, 1e282, 1e283, 1e284, 1e285, 1e286, 1e287, 1e288, 1e289, //
    1e290, 1e291, 1e292, 1e293, 1e294, 1e295, 1e296, 1e297, 1e298, 1e299, //
    1e300, 1e301, 1e302, 1e303, 1e304, 1e305, 1e306, 1e307, 1e308,
];

/// Creates an error for the token at the offset.
#[cold]
fn token_error(offset: usize, msg: &'static str) -> Error {
    Error::new(ErrorKind::Unexpected, msg).with_offset(offset)
}

/// Returns `true` if a decimal number without exponent is the shortest
/// representation of its value as `f64` (as formatted by `Debug`).
///
/// The number is given as its digits (without the dot) and the number of
/// fraction digits.  In that case the text can be recovered from the value,
/// so the value is emitted as float.  This is the case if the fraction has
/// no trailing zeros (other than `.0`), there are at most 15 significant
/// digits and the value is zero or at least 1e-4 (below that the shortest
/// representation uses an exponent).  15 digits are guaranteed to roundtrip
/// through `f64` and in that range `f64_from_parts` rounds correctly.
#[inline]
fn is_shortest_repr(digits: u64, frac_len: u32) -> bool {
    const MAX: u64 = 1_000_000_000_000_000;
    digits < MAX
        && (frac_len == 1 || !digits.is_multiple_of(10))
        && (frac_len <= 4 || digits == 0 || (frac_len <= 19 && digits >= 10u64.pow(frac_len - 4)))
}

/// Emits a number.
#[inline]
fn emit_number<'i, O: Out<'i>>(
    out: &mut O,
    number: Number,
    input: &[u8],
    exact_numbers: bool,
    start: usize,
    end: usize,
) -> Result<(), Error> {
    match number {
        Number::U64(val) => out.emit(Event::from(val)),
        Number::I64(val) => out.emit(Event::from(val)),
        Number::F64(val) => out.emit(Event::from(val)),
        Number::Literal(val) if exact_numbers => emit_literal(out, input, val, start, end),
        Number::Literal(val) => out.emit(Event::from(val)),
        Number::BigInt(val) => emit_big_int(out, val),
    }
}

/// Emits a number as number extension value with its text.
///
/// This is not inlined to keep the code of the parser loop small.
#[inline(never)]
fn emit_literal<'i, O: Out<'i>>(
    out: &mut O,
    input: &[u8],
    value: f64,
    start: usize,
    end: usize,
) -> Result<(), Error> {
    // SAFETY: numbers only consist of ASCII characters
    let text = unsafe { str::from_utf8_unchecked(&input[start..end]) };
    let number = ExactNumber::new(text, value);
    out.emit(Atom::Ext(ExtValue::borrowed_value::<ExactNumber>(&number)))
}

/// Emits an integer that does not fit into 64 bits as extension value.
#[cold]
fn emit_big_int<'i, O: Out<'i>>(out: &mut O, text: &str) -> Result<(), Error> {
    // the tokenizer already validated that the value fits
    if text.starts_with('-') {
        let value: i128 = text.parse().unwrap();
        out.emit(Atom::Ext(ExtValue::borrowed(&value)))
    } else {
        let value: u128 = text.parse().unwrap();
        out.emit(Atom::Ext(ExtValue::borrowed(&value)))
    }
}

#[cfg(test)]
mod tests {
    use deser::de::Recording;

    use super::*;

    const OPTIONS: Options = Options {
        validate_utf8: true,
        exact_numbers: true,
    };

    fn events(value: Recording) -> Vec<Event<'static>> {
        value.events().cloned().collect()
    }

    /// Parses the input in one go.
    fn parse_complete(input: &str) -> Result<Vec<Event<'static>>, String> {
        let mut out = None::<Recording>;
        let mut parser = Parser::default();
        {
            let mut driver = DeserializeDriver::new(&mut out);
            parser
                .parse(
                    input.as_bytes(),
                    0,
                    true,
                    0,
                    OPTIONS,
                    &mut Borrowing(&mut driver),
                )
                .map_err(|err| err.message().to_string())?;
        }
        Ok(events(out.unwrap()))
    }

    /// Feeds the input to the parser in chunks like a stream would.
    fn parse_chunked(input: &str, size: usize) -> Result<Vec<Event<'static>>, String> {
        let mut out = None::<Recording>;
        let mut parser = Parser::default();
        {
            let mut driver = DeserializeDriver::new(&mut out);
            let mut buffer = Vec::new();
            let mut base = 0;
            let mut rest = input.as_bytes();
            loop {
                let len = size.min(rest.len());
                buffer.extend_from_slice(&rest[..len]);
                rest = &rest[len..];
                let eof = rest.is_empty();
                match parser
                    .parse(&buffer, 0, eof, base, OPTIONS, &mut Copying(&mut driver))
                    .map_err(|err| err.message().to_string())?
                {
                    Progress::Done(_) => break,
                    Progress::NeedMore(consumed) => {
                        assert!(!eof, "more input needed at the end");
                        buffer.drain(..consumed);
                        base += consumed;
                    }
                }
            }
        }
        Ok(events(out.unwrap()))
    }

    #[test]
    fn test_chunks() {
        let long = "x".repeat(300);
        let inputs = [
            r#"{"a": [1, -2, 3.5, 1e10, -0.25e-3, 12345678901234567890123], "b": {"c": null}}"#.to_string(),
            r#"["plain", "esc\"aped\\", "\u00e4\ud83d\ude00", "ä", true, false, null, [], {}, [[]]]"#.to_string(),
            format!(r#"{{"{long}": "{long}\n{long}", "k\u0041": [{{}}, {{"x": [1]}}]}}"#),
            "  42  ".to_string(),
            "\"top\"".to_string(),
            "123".to_string(),
            " [ 1 , 2 ] ".to_string(),
        ];
        for input in inputs {
            let expected = parse_complete(&input).unwrap();
            for size in 1..=input.len() {
                assert_eq!(
                    parse_chunked(&input, size).unwrap(),
                    expected,
                    "{input} size {size}"
                );
            }
        }
    }

    #[test]
    fn test_errors_in_chunks() {
        for input in [
            "[1, 2",
            "[1 2]",
            "{\"a\" 1}",
            "[tru]",
            "\"abc",
            "[1,]",
            "{\"a\": \"\\x\"}",
        ] {
            let expected = parse_complete(input).unwrap_err();
            for size in 1..=input.len() {
                assert_eq!(
                    parse_chunked(input, size).unwrap_err(),
                    expected,
                    "{input} size {size}"
                );
            }
        }
    }
}
