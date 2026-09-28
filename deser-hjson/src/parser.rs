// @generated from deser-template-json/src/parser.rs by
// deser-template-json/generate.py.  Do not edit.
use alloc::vec::Vec;
use core::str;

use deser_core::Text;
use deser_core::de::DeserializeDriver;
use deser_core::ext::{ExtValue, Number as ExactNumber};
use deser_core::{Atom, Error, ErrorKind, Event, State};
use deser_core::{Implicit, ImplicitValue};

use crate::scan::skip_to_escape_single;
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
/// Atoms of strings which are slices of the input (strings and map keys) are
/// passed to [`emit_input`](Self::emit_input), which can pass them on
/// borrowed if the input lives long enough.
pub(crate) trait Out<'i> {
    fn state_mut(&mut self) -> &mut State;
    fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error>;
    fn emit_input(&mut self, atom: Atom<'i>) -> Result<(), Error>;
}

/// Creates the atom of a map key.
///
/// JSON only has strings as keys, which are lexical: they can stand for
/// values of other types (like integers).
#[inline(always)]
fn key_atom(key: &str) -> Atom<'_> {
    Atom::Lexical(Text::borrowed(key))
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
    fn emit_input(&mut self, atom: Atom<'i>) -> Result<(), Error> {
        self.0.emit_borrowed(atom)
    }
}

#[cfg(any(test, feature = "io"))]
/// Passes strings of the input on as data that is only valid for the call.
///
/// This is used when the input does not outlive the deserialization (for
/// instance a buffer that is refilled).
pub(crate) struct Copying<'a, 'd, 'de>(pub &'a mut DeserializeDriver<'d, 'de>);

#[cfg(any(test, feature = "io"))]
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
    fn emit_input(&mut self, atom: Atom<'i>) -> Result<(), Error> {
        self.0.emit(atom)
    }
}

#[cfg(feature = "io")]
/// Discards the events.
///
/// This is used to skip the rest of a value after an error.
pub(crate) struct Discard(pub State);

#[cfg(feature = "io")]
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
    fn emit_input(&mut self, _atom: Atom<'i>) -> Result<(), Error> {
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
    /// A map without braces at the root, it ends at the end of the input.
    Braceless,
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
    // the number of characters of the line before the input that follows
    // (the indentation of multiline strings is relative to their column)
    column: usize,
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
            column: 0,
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
    #[cfg(feature = "io")]
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

    #[cfg(feature = "io")]
    /// Returns where the rest of the value continues if the last error was
    /// an error of a sink.
    ///
    /// In that case the state is as if the event was accepted and the rest
    /// of the value can be parsed from the position (for instance with
    /// [`Discard`]).
    pub(crate) fn recoverable(&self) -> Option<usize> {
        self.recoverable
    }

    /// Tells the parser that the input was consumed without it
    /// suspending, for instance a complete value or the whitespace between
    /// values in a stream.
    ///
    /// The parser tracks the column where the next input starts.
    #[cfg(feature = "io")]
    pub(crate) fn advance(&mut self, input: &[u8]) {
        self.column = advance_column(self.column, input);
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
            number_start: 0,
            truncated: false,
            eof,
            column: self.column,
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
                self.column = advance_column(self.column, &input[..$consumed]);
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
                let key = match $byte {
                    b'"' => {
                        cur.bump();
                        string!(start, Expect::Key)
                    }
                    b'\'' => {
                        cur.bump();
                        string!(start, Expect::Key)
                    }
                    b'{' | b'}' | b'[' | b']' | b',' | b':' => {
                        return Err(token_error(base + start, "expected map key"));
                    }
                    // keys without quotes end at whitespace and punctuators
                    _ => {
                        cur.hit_end = false;
                        let rv = cur.parse_quoteless_key();
                        if cur.hit_end && !eof {
                            suspend!(start, Expect::Key)
                        }
                        rv?
                    }
                };
                match key {
                    Str::Borrowed(key) => {
                        out.state_mut()
                            .set_input_range(base + start, base + cur.pos);
                        sink!(out.emit_input(key_atom(key)), Expect::Colon)
                    }
                    Str::Scratch(key) => sink!(
                        emit!(out, base, start, cur.pos, key_atom(key)),
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

        // skips whitespace and returns the next byte or `None` at the end
        // of the input
        macro_rules! next_byte_or_end {
            ($expect:expr) => {
                match cur.parse_whitespace() {
                    Some(byte) => Some(byte),
                    None if eof => None,
                    None => suspend!(cur.pos, $expect),
                }
            };
        }

        // in a map without braces the map ends at the end of the input or
        // the next key follows.  Evaluates to `true` if a value follows.
        macro_rules! open_braceless {
            () => {{
                match next_byte_or_end!(Expect::KeyOrEnd) {
                    Some(byte) => {
                        key!(byte);
                        true
                    }
                    None => {
                        close!(cur.pos, Event::MapEnd);
                        false
                    }
                }
            }};
        }

        // emits a string value, the cursor is after the opening quote
        macro_rules! string_value {
            ($start:expr) => {
                match string!($start, Expect::Value) {
                    Str::Borrowed(val) => {
                        out.state_mut()
                            .set_input_range(base + $start, base + cur.pos);
                        sink!(
                            out.emit_input(Atom::Str(Text::borrowed(val))),
                            Expect::AfterValue
                        )
                    }
                    Str::Scratch(val) => sink!(
                        emit!(out, base, $start, cur.pos, Event::from(val)),
                        Expect::AfterValue
                    ),
                }
            };
        }

        // emits a value without quotes: a number, `true`, `false` or `null`
        // if only whitespace, a comma, the end of a container or a comment
        // follows it on the line, otherwise the rest of the line is a
        // string.
        macro_rules! quoteless {
            ($start:expr) => {{
                cur.pos = $start;
                let rv = cur.parse_quoteless();
                if cur.hit_end && !eof {
                    suspend!($start, Expect::Value)
                }
                let value = rv?;
                out.state_mut()
                    .set_input_range(base + $start, base + cur.pos);
                sink!(
                    emit_quoteless(out, value, input, exact_numbers, $start, cur.pos),
                    Expect::AfterValue
                )
            }};
        }

        // emits a multiline string, the cursor is after the first quote
        macro_rules! multiline_string {
            ($start:expr) => {{
                cur.hit_end = false;
                let rv = cur.parse_multiline_str(scratch, $start);
                if cur.hit_end && !eof {
                    suspend!($start, Expect::Value)
                }
                let val = rv?;
                sink!(
                    emit!(out, base, $start, cur.pos, Event::from(val)),
                    Expect::AfterValue
                )
            }};
        }

        // continue where the parser was suspended
        let mut skip_value = match self.expect {
            Expect::Value => false,
            Expect::ValueOrEnd => !open_seq!(),
            Expect::KeyOrEnd if container == Container::Braceless => !open_braceless!(),
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
                // a map key at the root starts a map without braces
                if container == Container::Top && partial.is_none() && byte != b'{' && byte != b'['
                {
                    match cur.is_map_key() {
                        Some(true) => {
                            stack.push(container);
                            container = Container::Braceless;
                            sink!(
                                emit!(out, base, cur.pos, cur.pos, Event::map_start()),
                                Expect::KeyOrEnd
                            );
                            key!(byte);
                            continue 'value;
                        }
                        Some(false) => {}
                        None => suspend!(cur.pos, Expect::Value),
                    }
                }
                let start = cur.pos;
                cur.bump();
                cur.hit_end = false;
                match byte {
                    b'"' => string_value!(start),
                    // `'''` starts a multiline string
                    b'\'' => match input.get(start + 1..start + 3) {
                        Some(b"''") => multiline_string!(start),
                        None if !eof => suspend!(start, Expect::Value),
                        _ => string_value!(start),
                    },
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
                    _ => quoteless!(start),
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
                    // the comma is optional
                    Container::Braceless => {
                        if next_byte_or_end!(Expect::AfterValue) == Some(b',') {
                            cur.bump();
                        }
                        if open_braceless!() {
                            continue 'value;
                        }
                        continue;
                    }
                };
                match next_byte!(Expect::AfterValue) {
                    b',' => {
                        cur.bump();
                        // the container can end after the comma
                        let more = if container == Container::Map {
                            open_map!()
                        } else {
                            open_seq!()
                        };
                        if more {
                            continue 'value;
                        }
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
                    // the comma between values is optional
                    byte => {
                        if container == Container::Map {
                            key!(byte);
                        }
                        continue 'value;
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
    // where the number being parsed starts
    number_start: usize,
    // digits of the number were dropped from its significand
    truncated: bool,
    // `true` if no input follows, a comment at the end is complete
    eof: bool,
    // the number of characters of the line before the input
    column: usize,
}

/// A comment in the input (see [`Cursor::skip_comment`]).
enum Comment {
    /// The comment ends at the position.
    End(usize),
    /// The input ends within the comment.
    Incomplete,
    /// The byte at the position is invalid (it's not a comment or the
    /// comment is not valid UTF-8).
    Invalid(usize),
}

impl<'a> Cursor<'a> {
    /// Creates a cursor to skip whitespace in a complete input.
    pub(crate) fn new(input: &'a [u8], pos: usize) -> Cursor<'a> {
        Cursor {
            input,
            pos,
            validate_utf8: true,
            hit_end: false,
            partial: (0, 0),
            number_start: 0,
            truncated: false,
            eof: true,
            column: 0,
        }
    }

    /// Creates a cursor to skip whitespace in an input that more input
    /// might follow.
    #[cfg(feature = "io")]
    pub(crate) fn new_partial(input: &'a [u8], pos: usize, eof: bool) -> Cursor<'a> {
        Cursor {
            eof,
            ..Cursor::new(input, pos)
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

        // strings in single quotes end at a single quote
        let single = self.input[start] == b'\'';

        loop {
            self.pos = if single {
                skip_to_escape_single(self.input, self.pos)
            } else {
                skip_to_escape(self.input, self.pos)
            };
            if self.pos == self.input.len() {
                self.hit_end = true;
                self.partial = (self.pos, copied);
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    "unexpected end of string",
                ));
            }
            let byte = self.input[self.pos];
            // the closing single quote is handled like a double quote
            let byte = if single && byte == b'\'' { b'"' } else { byte };
            match byte {
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
                // control characters other than line breaks are allowed
                byte if byte != b'\n' && byte != b'\r' => self.pos += 1,
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

    /// Consumes the next eight bytes if they are digits and returns their
    /// value.
    ///
    /// This does not look at the end of the input, the digits after these
    /// are parsed one by one.
    #[inline(always)]
    fn eight_digits(&mut self) -> Option<u64> {
        let bytes = self.input.get(self.pos..self.pos + 8)?;
        let value = u64::from_le_bytes(bytes.try_into().unwrap());
        // all bytes are between b'0' and b'9'
        let digits = value.wrapping_sub(0x3030_3030_3030_3030);
        let above = value.wrapping_add(0x4646_4646_4646_4646);
        if (digits | above) & 0x8080_8080_8080_8080 != 0 {
            return None;
        }
        self.pos += 8;
        // combine pairs of digits, then pairs of those and so on
        let pairs = digits.wrapping_mul(10).wrapping_add(digits >> 8);
        let low = (pairs & 0x0000_00ff_0000_00ff).wrapping_mul(0x000f_4240_0000_0064);
        let high = ((pairs >> 16) & 0x0000_00ff_0000_00ff).wrapping_mul(0x0000_2710_0000_0001);
        Some(u64::from((low.wrapping_add(high) >> 32) as u32))
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
                    0xDC00..=0xDFFF => return Err(lone_surrogate()),

                    // Non-BMP characters are encoded as a sequence of
                    // two hex escapes, representing UTF-16 surrogates.
                    n1 @ 0xD800..=0xDBFF => {
                        if self.next_or_eof()? != b'\\' || self.next_or_eof()? != b'u' {
                            return Err(lone_surrogate());
                        }

                        let n2 = self.decode_hex_escape()?;

                        if !(0xDC00..=0xDFFF).contains(&n2) {
                            return Err(lone_surrogate());
                        }

                        let n = (u32::from(n1 - 0xD800) << 10 | u32::from(n2 - 0xDC00)) + 0x1_0000;

                        match char::from_u32(n) {
                            Some(c) => c,
                            None => return Err(lone_surrogate()),
                        }
                    }

                    n => match char::from_u32(u32::from(n)) {
                        Some(c) => c,
                        None => return Err(lone_surrogate()),
                    },
                };

                buffer.extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
            }
            b'\'' => buffer.push(b'\''),
            _ => return Err(invalid_escape()),
        }

        Ok(())
    }

    /// Parses a map key without quotes.
    ///
    /// The key ends at whitespace or a punctuator (`{}[],:`), it's
    /// borrowed from the input.
    fn parse_quoteless_key<'b>(&mut self) -> Result<Str<'a, 'b>, Error> {
        let input = self.input;
        let start = self.pos;
        let len = input[start..]
            .iter()
            .position(|&byte| !is_key_byte(byte))
            .unwrap_or_else(|| {
                // the key might continue
                self.hit_end = true;
                input.len() - start
            });
        self.pos = start + len;
        let key = &input[start..self.pos];
        if self.validate_utf8 && !is_ascii(key) && !validate_utf8_slice(key) {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "invalid utf-8 in map key",
            ));
        }
        // SAFETY: the key is valid UTF-8 as it comes from a `&str` or was
        // validated above, it ends at an ASCII character
        Ok(Str::Borrowed(unsafe { str::from_utf8_unchecked(key) }))
    }

    /// Returns `true` if a map key followed by a colon is at the cursor.
    ///
    /// This tells if the value at the root is a map without braces.
    /// Returns `None` if more input is needed to tell.
    fn is_map_key(&self) -> Option<bool> {
        let input = self.input;
        let incomplete = || if self.eof { Some(false) } else { None };
        let mut pos = self.pos;
        match input[pos] {
            quote @ (b'"' | b'\'') => loop {
                pos += 1;
                match input.get(pos) {
                    Some(b'\\') => pos += 1,
                    Some(&byte) if byte == quote => {
                        pos += 1;
                        break;
                    }
                    // an invalid string, the parser reports the error
                    Some(b'\n' | b'\r') => return Some(false),
                    Some(_) => {}
                    None => return incomplete(),
                }
            },
            _ => match input[pos..].iter().position(|&byte| !is_key_byte(byte)) {
                Some(0) => return Some(false),
                Some(len) => pos += len,
                None => return incomplete(),
            },
        }
        let mut cur = Cursor::new(input, pos);
        cur.eof = self.eof;
        match cur.parse_whitespace() {
            Some(byte) => Some(byte == b':'),
            None => incomplete(),
        }
    }

    /// Parses a value without quotes, the cursor is at its start.
    ///
    /// Numbers and literals are complete once the byte after them (and
    /// after the whitespace that follows) is known, strings once the line
    /// is complete.  If more input is needed, `hit_end` is set.
    fn parse_quoteless(&mut self) -> Result<Quoteless<'a>, Error> {
        let input = self.input;
        let start = self.pos;
        let value = match input[start] {
            byte @ (b'-' | b'0'..=b'9') => {
                self.number_start = start;
                self.truncated = false;
                self.bump();
                let rv = if byte == b'-' {
                    let first_digit = self.next_or_nul();
                    self.parse_integer(false, first_digit)
                } else {
                    self.parse_integer(true, byte)
                };
                rv.ok().map(Quoteless::Number)
            }
            byte @ (b't' | b'f' | b'n') => {
                let (word, value): (&[u8], _) = match byte {
                    b't' => (b"true", ImplicitValue::Bool(true)),
                    b'f' => (b"false", ImplicitValue::Bool(false)),
                    _ => (b"null", ImplicitValue::Null),
                };
                let rest = &input[start..];
                if rest.starts_with(word) {
                    self.pos = start + word.len();
                    Some(Quoteless::Literal(value))
                } else {
                    // the word might continue
                    self.hit_end |= word.starts_with(rest);
                    None
                }
            }
            _ => None,
        };
        // `5 times` is a string, `5 # times` is a number
        if let Some(value) = value {
            let pos = input[self.pos..]
                .iter()
                .position(|&byte| byte != b' ' && byte != b'\t')
                .map_or(input.len(), |index| self.pos + index);
            match input[pos..] {
                [b',' | b']' | b'}' | b'#' | b'\n' | b'\r', ..] | [b'/', b'/' | b'*', ..] => {
                    return Ok(value);
                }
                [] if self.eof => return Ok(value),
                // what follows is not known yet
                [] | [b'/'] => self.hit_end = true,
                _ => {}
            }
        }

        // the string ends at the end of the line
        let end = match line_end(input, start) {
            Some(end) => end,
            None => {
                self.hit_end = true;
                input.len()
            }
        };
        // the whitespace at the end is not part of the string
        let mut stop = end;
        while matches!(input[stop - 1], b' ' | b'\t') {
            stop -= 1;
        }
        let bytes = &input[start..stop];
        if self.validate_utf8 && !is_ascii(bytes) && !validate_utf8_slice(bytes) {
            return Err(Error::new(ErrorKind::Unexpected, "invalid utf-8 in string"));
        }
        self.pos = stop;
        // SAFETY: the input is valid UTF-8 as it comes from a `&str` or was
        // validated above, the string ends at an ASCII character
        Ok(Quoteless::Str(unsafe { str::from_utf8_unchecked(bytes) }))
    }

    /// Parses a multiline string, the cursor is after the first of the
    /// three quotes at `start`.
    ///
    /// Whitespace after the opening quotes and the indentation up to the
    /// column of the opening quotes are removed, as are carriage returns
    /// and the last line break.
    fn parse_multiline_str<'b>(
        &mut self,
        buffer: &'b mut Vec<u8>,
        start: usize,
    ) -> Result<&'b str, Error> {
        let input = self.input;
        let content = start + 3;
        let Some(len) = input[content..].windows(3).position(|w| w == b"'''") else {
            self.hit_end = true;
            self.pos = input.len();
            return Err(eof_error());
        };
        let close = content + len;
        let text = &input[content..close];
        if self.validate_utf8 && !is_ascii(text) && !validate_utf8_slice(text) {
            return Err(Error::new(ErrorKind::Unexpected, "invalid utf-8 in string"));
        }
        let indent = match input[..start].iter().rposition(|&byte| byte == b'\n') {
            Some(index) => count_chars(&input[index + 1..start]),
            None => self.column + count_chars(&input[..start]),
        };
        // skips whitespace up to the column of the quotes
        let skip_indent = |mut pos: usize| {
            let limit = (pos + indent).min(close);
            while pos < limit && input[pos] <= b' ' && input[pos] != b'\n' {
                pos += 1;
            }
            pos
        };

        buffer.clear();
        let mut pos = content;
        while pos < close && input[pos] <= b' ' && input[pos] != b'\n' {
            pos += 1;
        }
        if pos < close && input[pos] == b'\n' {
            pos = skip_indent(pos + 1);
        }
        while pos < close {
            match input[pos] {
                b'\n' => {
                    buffer.push(b'\n');
                    pos = skip_indent(pos + 1);
                }
                b'\r' => pos += 1,
                byte => {
                    buffer.push(byte);
                    pos += 1;
                }
            }
        }
        if buffer.last() == Some(&b'\n') {
            buffer.pop();
        }
        self.pos = close + 3;
        // SAFETY: the text was validated, only ASCII characters were removed
        Ok(unsafe { str::from_utf8_unchecked(buffer) })
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
                // `#` starts a line comment too
                Some(b'/' | b'#') => match self.skip_comment(pos) {
                    Comment::End(end) => pos = end,
                    // the comment is scanned again with more input
                    Comment::Incomplete => {
                        self.pos = pos;
                        self.hit_end = true;
                        return None;
                    }
                    Comment::Invalid(pos) => {
                        self.pos = pos;
                        return Some(input[pos]);
                    }
                },
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

    /// Skips the comment at `pos` (which is at a slash).
    #[cold]
    fn skip_comment(&self, pos: usize) -> Comment {
        let input = self.input;
        let hash = input[pos] == b'#';
        let end = match input.get(pos + 1) {
            _ if hash => match line_comment_len(&input[pos + 1..]) {
                Some(len) => pos + 1 + len,
                None if self.eof => input.len(),
                None => return Comment::Incomplete,
            },
            Some(b'/') => match line_comment_len(&input[pos + 2..]) {
                Some(len) => pos + 2 + len,
                None if self.eof => input.len(),
                None => return Comment::Incomplete,
            },
            // an unterminated comment at the end is reported as the end
            // of the input
            Some(b'*') => match input[pos + 2..].windows(2).position(|w| w == b"*/") {
                Some(index) => pos + 2 + index + 2,
                None => return Comment::Incomplete,
            },
            Some(_) => return Comment::Invalid(pos),
            None if self.eof => return Comment::Invalid(pos),
            None => return Comment::Incomplete,
        };
        // bytes outside of strings are validated here
        let comment = &input[pos..end];
        if self.validate_utf8
            && !is_ascii(comment)
            && let Err(err) = str::from_utf8(comment)
        {
            return Comment::Invalid(pos + err.valid_up_to());
        }
        Comment::End(end)
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
                // eight digits at a time while they cannot overflow
                while res < EIGHT_DIGITS_LIMIT
                    && let Some(digits) = self.eight_digits()
                {
                    res = res * 100_000_000 + digits;
                }

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
        // the digits after the first 19 are dropped from the significand
        self.truncated = true;
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
                    return self.float(nonnegative, significand, exponent);
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
        // eight digits at a time while they cannot overflow
        while significand < EIGHT_DIGITS_LIMIT
            && let Some(digits) = self.eight_digits()
        {
            significand = significand * 100_000_000 + digits;
            exponent -= 8;
            at_least_one_digit = true;
        }
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
                self.truncated = true;
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
                let value = self.float(nonnegative, significand, exponent)?;
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

        self.float(nonnegative, significand, final_exp)
    }

    /// Returns the value of the float that was just parsed.
    ///
    /// The number is given as its significand (the digits) and decimal
    /// exponent.  For exponents up to 22 (which covers numbers with up to
    /// 22 fraction digits) the value is computed from them: if the
    /// significand has at most 53 bits, both it and the power of ten are
    /// exact as `f64` and their product (or quotient) is rounded correctly
    /// as only the result is rounded.  Otherwise the algorithm of Eisel and
    /// Lemire is used (see [`eisel_lemire`]).  For other exponents (and if
    /// digits were dropped from the significand) the text of the number is
    /// parsed, which rounds correctly too.
    #[inline]
    fn float(&self, nonnegative: bool, significand: u64, exponent: i32) -> Result<f64, Error> {
        if !self.truncated {
            let value = match POW10.get(exponent.unsigned_abs() as usize) {
                Some(&pow) if significand <= 1 << 53 => Some(if exponent >= 0 {
                    significand as f64 * pow
                } else {
                    significand as f64 / pow
                }),
                Some(_) => eisel_lemire(significand, exponent),
                None if significand == 0 => Some(0.0),
                None => None,
            };
            if let Some(value) = value {
                return Ok(if nonnegative { value } else { -value });
            }
        }
        self.parse_float_text()
    }

    /// Parses the text of the number that was just parsed as float.
    #[cold]
    #[inline(never)]
    fn parse_float_text(&self) -> Result<f64, Error> {
        // SAFETY: the number was validated and only consists of ASCII
        // characters
        let text = unsafe { str::from_utf8_unchecked(&self.input[self.number_start..self.pos]) };
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => Ok(value),
            _ => Err(number_out_of_range()),
        }
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

/// Eight more digits can be added to significands below this.
const EIGHT_DIGITS_LIMIT: u64 = (u64::MAX - 99_999_999) / 100_000_000;

/// Powers of ten which are exact as `f64`.
static POW10: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/// The powers of five from 5^-22 to 5^22 with 128 bits (as low and high
/// half) for [`eisel_lemire`].
///
/// The positive powers are exact (they have at most 52 bits) and shifted
/// so that the highest bit is set.  The negative ones are `2^(z + 127) /
/// 5^-q + 1` (rounded down) where `2^z` is the smallest power of two above
/// `5^-q`, which also has the highest bit set.
static POW5: [(u64, u64); 45] = {
    let mut table = [(0, 0); 45];
    let mut q = -22i32;
    while q <= 22 {
        let pow = 5u128.pow(q.unsigned_abs());
        let value = if q >= 0 {
            pow << pow.leading_zeros()
        } else {
            // 2^(z + 127) / pow by long division, the quotient has 128 bits
            let z = 128 - pow.leading_zeros();
            let mut quotient = 0u128;
            let mut rem = 1u128;
            let mut bit = 0;
            while bit < z + 127 {
                rem <<= 1;
                quotient <<= 1;
                if rem >= pow {
                    rem -= pow;
                    quotient |= 1;
                }
                bit += 1;
            }
            quotient + 1
        };
        table[(q + 22) as usize] = (value as u64, (value >> 64) as u64);
        q += 1;
    }
    table
};

/// Returns the correctly rounded value of `significand * 10^exponent`.
///
/// This is the algorithm of Eisel and Lemire ("Number Parsing at a
/// Gigabyte per Second", <https://arxiv.org/abs/2101.11408>) as used by the
/// standard library, for exponents from -22 to 22: the significand is
/// multiplied with a 128 bit approximation of the power of five, the
/// power of two is added to the exponent.  In this range the product is
/// precise enough to round correctly (ties to even only happen for
/// exponents from -4 to 23 where the powers of five are exact).  Returns
/// `None` for values out of the range of normal floats (which do not occur
/// in this range).
fn eisel_lemire(significand: u64, exponent: i32) -> Option<f64> {
    // the bits of the mantissa (without the implicit one) and the extra
    // bits of the product needed to round
    const MANTISSA_BITS: i32 = 52;
    const PRECISION_MASK: u64 = u64::MAX >> (MANTISSA_BITS + 3);

    let leading_zeros = significand.leading_zeros();
    let significand = significand << leading_zeros;
    let (pow_lo, pow_hi) = POW5[(exponent + 22) as usize];
    let product = u128::from(significand) * u128::from(pow_hi);
    let (mut lo, mut hi) = (product as u64, (product >> 64) as u64);
    if hi & PRECISION_MASK == PRECISION_MASK {
        // the bits below the mantissa might carry, the lower half of the
        // power is needed
        let carry = ((u128::from(significand) * u128::from(pow_lo)) >> 64) as u64;
        lo = lo.wrapping_add(carry);
        if carry > lo {
            hi += 1;
        }
    }

    let upper_bit = (hi >> 63) as i32;
    let shift = upper_bit + 64 - MANTISSA_BITS - 3;
    let mut mantissa = hi >> shift;
    // the biased exponent: floor(log2(10^exponent)) + 63 for the product,
    // the normalization and the bias of 1023
    let mut power2 = ((exponent.wrapping_mul(152_170 + 65536) >> 16) + 63) + upper_bit
        - leading_zeros as i32
        + 1023;
    if power2 <= 0 {
        return None;
    }
    // exactly half way between two floats rounds to the even one
    if lo <= 1 && (-4..=23).contains(&exponent) && mantissa & 3 == 1 && mantissa << shift == hi {
        mantissa &= !1;
    }
    mantissa += mantissa & 1;
    mantissa >>= 1;
    if mantissa >= 2 << MANTISSA_BITS {
        mantissa = 1 << MANTISSA_BITS;
        power2 += 1;
    }
    mantissa &= !(1 << MANTISSA_BITS);
    if power2 >= 0x7ff {
        return None;
    }
    Some(f64::from_bits(mantissa | (power2 as u64) << MANTISSA_BITS))
}

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
/// through `f64`.
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

/// A value without quotes.
enum Quoteless<'a> {
    Str(&'a str),
    /// `true`, `false` or `null`.
    Literal(ImplicitValue),
    Number(Number<'a>),
}

/// Emits a value without quotes.
///
/// Hjson infers the type of numbers, `true`, `false` and `null` from their
/// text, they are passed on as implicit values (a string receives the
/// text).  Integers that do not fit into 64 bits and, with exact numbers,
/// floats whose text cannot be recovered from their value are passed on as
/// in JSON.
#[inline(never)]
fn emit_quoteless<'i, O: Out<'i>>(
    out: &mut O,
    value: Quoteless<'i>,
    input: &'i [u8],
    exact_numbers: bool,
    start: usize,
    end: usize,
) -> Result<(), Error> {
    let value = match value {
        Quoteless::Str(value) => return out.emit_input(Atom::Str(Text::borrowed(value))),
        Quoteless::Literal(value) => value,
        Quoteless::Number(Number::U64(value)) => ImplicitValue::U64(value),
        Quoteless::Number(Number::I64(value)) => ImplicitValue::I64(value),
        Quoteless::Number(Number::F64(value)) => ImplicitValue::F64(value),
        Quoteless::Number(Number::Literal(value)) if !exact_numbers => ImplicitValue::F64(value),
        Quoteless::Number(number) => {
            return emit_number(out, number, input, exact_numbers, start, end);
        }
    };
    // SAFETY: numbers and literals only consist of ASCII characters
    let text = unsafe { str::from_utf8_unchecked(&input[start..end]) };
    out.emit_input(Atom::Implicit(Implicit::new(Text::borrowed(text), value)))
}

/// Returns the position of the line break that ends the line at `pos`.
fn line_end(input: &[u8], pos: usize) -> Option<usize> {
    input[pos..]
        .iter()
        .position(|&byte| byte == b'\n' || byte == b'\r')
        .map(|index| pos + index)
}

/// Returns `true` if the byte can be part of a map key without quotes.
fn is_key_byte(byte: u8) -> bool {
    byte > b' ' && !matches!(byte, b'{' | b'}' | b'[' | b']' | b',' | b':')
}

/// Returns the number of characters in valid UTF-8.
fn count_chars(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&byte| byte & 0xc0 != 0x80).count()
}

/// Returns the column after the input starting at `column`.
fn advance_column(column: usize, input: &[u8]) -> usize {
    match input.iter().rposition(|&byte| byte == b'\n') {
        Some(index) => count_chars(&input[index + 1..]),
        None => column + count_chars(input),
    }
}

/// Returns the length of the rest of a line comment (after the slashes)
/// including the line break that ends it.
fn line_comment_len(bytes: &[u8]) -> Option<usize> {
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' | b'\r' => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}

#[cold]
fn invalid_escape() -> Error {
    Error::new(ErrorKind::Unexpected, "invalid escape in string")
}

#[cold]
fn lone_surrogate() -> Error {
    Error::new(
        ErrorKind::Unexpected,
        "lone surrogate in unicode escape in string",
    )
}

#[cold]
fn number_out_of_range() -> Error {
    Error::new(ErrorKind::OutOfRange, "number out of range")
}

#[cfg(test)]
mod tests {
    use deser_core::de::Recording;

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

    #[test]
    fn test_integers() {
        fn parse(text: &str) -> Result<u64, String> {
            let mut out = None::<u64>;
            let mut driver = DeserializeDriver::new(&mut out);
            Parser::default()
                .parse(
                    text.as_bytes(),
                    0,
                    true,
                    0,
                    OPTIONS,
                    &mut Borrowing(&mut driver),
                )
                .map_err(|err| err.message().to_string())?;
            drop(driver);
            Ok(out.unwrap())
        }

        // every length, around the groups of eight digits and the limit
        let mut value = 0u64;
        for digit in (1..=20).map(|x| x % 10) {
            value = value.wrapping_mul(10).wrapping_add(digit);
            let text = value.to_string();
            assert_eq!(parse(&text), Ok(value), "{text}");
        }
        for value in [
            u64::MAX,
            u64::MAX - 1,
            u64::MAX / 10,
            99_999_999,
            100_000_000,
            EIGHT_DIGITS_LIMIT,
            EIGHT_DIGITS_LIMIT * 100_000_000 + 99_999_999,
            (EIGHT_DIGITS_LIMIT + 1) * 100_000_000,
        ] {
            assert_eq!(parse(&value.to_string()), Ok(value), "{value}");
        }
    }

    #[test]
    fn test_floats_are_rounded_correctly() {
        /// Parses a number as `f64`.
        fn parse(text: &str) -> Result<f64, String> {
            let mut out = None::<f64>;
            let mut driver = DeserializeDriver::new(&mut out);
            let options = Options {
                validate_utf8: false,
                exact_numbers: false,
            };
            Parser::default()
                .parse(
                    text.as_bytes(),
                    0,
                    true,
                    0,
                    options,
                    &mut Borrowing(&mut driver),
                )
                .map_err(|err| err.message().to_string())?;
            drop(driver);
            Ok(out.unwrap())
        }

        let check = |text: &str| match text.parse::<f64>() {
            Ok(value) if value.is_finite() => {
                assert_eq!(parse(text).map(f64::to_bits), Ok(value.to_bits()), "{text}");
            }
            // numbers out of range are strings
            _ => assert_eq!(
                parse(text),
                Err("unexpected string, expected f64".into()),
                "{text}"
            ),
        };
        for text in [
            "2e-23",
            "0.1",
            "9007199254740993",
            "9007199254740993.0",
            "1e23",
            "8.98846567431158e307",
            "1.7976931348623157e308",
            "1.7976931348623159e308",
            "2.2250738585072011e-308",
            "4.9e-324",
            "2.4703282292062328e-324",
            "1e-400",
            "0e999",
            "-0.0e-999",
            "123456789012345678901234567890e-10",
            "0.000000000000000000000000000001234567890123456789",
        ] {
            check(text);
        }

        let mut rng: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = |n: u64| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng % n
        };
        // miri is too slow for many iterations
        for _ in 0..if cfg!(miri) { 200 } else { 100_000 } {
            let mut text = String::new();
            if next(2) == 0 {
                text.push('-');
            }
            text.push(char::from(b'1' + next(9) as u8));
            for _ in 0..next(25) {
                text.push(char::from(b'0' + next(10) as u8));
            }
            if next(2) == 0 {
                text.push('.');
                for _ in 0..1 + next(25) {
                    text.push(char::from(b'0' + next(10) as u8));
                }
            }
            if next(2) == 0 {
                text.push_str(&format!("e{}", next(700) as i32 - 350));
            }
            check(&text);
        }
        // significands beyond 53 bits with exponents up to 22 (like
        // coordinates) are corrected with integer arithmetic
        for _ in 0..if cfg!(miri) { 200 } else { 100_000 } {
            let digits = 16 + next(4) as usize;
            let mut text = String::new();
            text.push(char::from(b'1' + next(9) as u8));
            for _ in 1..digits {
                text.push(char::from(b'0' + next(10) as u8));
            }
            if next(2) == 0 {
                text.insert(1 + next(digits as u64 - 1) as usize, '.');
            }
            if next(2) == 0 {
                text.push_str(&format!("e{}", next(30) as i32 - 15));
            }
            check(&text);
        }
        // the halfway points between floats above 2^53 (which are rounded
        // to the even float) and the numbers next to them
        for _ in 0..if cfg!(miri) { 20 } else { 10_000 } {
            let m = (1u64 << 52) + next(1 << 52);
            check(&format!("{}.5", m));
            check(&format!("{}.4999999999", m));
            let half = (2 * m + 1) << 10;
            for value in [half - 1, half, half + 1] {
                check(&format!("{value}e0"));
                check(&format!("{}e-1", u128::from(value) * 10));
            }
        }
    }

    /// Checks that the inputs parse like the equivalent JSON, in one go
    /// and in chunks.
    fn assert_like_json(inputs: &[(&str, &str)]) {
        for (input, json) in inputs {
            let expected = parse_complete(json).unwrap();
            assert_eq!(parse_complete(input), Ok(expected.clone()), "{input}");
            for size in 1..=input.len() {
                assert_eq!(
                    parse_chunked(input, size),
                    Ok(expected.clone()),
                    "{input} size {size}"
                );
            }
        }
    }

    /// Checks that the inputs fail with the given errors, in one go and
    /// in chunks.
    fn assert_errors(inputs: &[(&str, &str)]) {
        for (input, msg) in inputs {
            assert_eq!(parse_complete(input).unwrap_err(), *msg, "{input}");
            for size in 1..=input.len() {
                assert_eq!(
                    parse_chunked(input, size).unwrap_err(),
                    *msg,
                    "{input} size {size}"
                );
            }
        }
    }

    #[test]
    fn test_comments() {
        assert_like_json(&[
            ("// c\n1", "1"),
            ("/* c */ 1 /* c */", "1"),
            ("1 // c", "1"),
            ("[1, /* a\n * b */ 2 // c\n, 3]", "[1, 2, 3]"),
            ("{/**/\"a\"/**/:/**/1/**/}", "{\"a\": 1}"),
            (
                "[\"// no comment\", \"/* no comment */\"]",
                "[\"// no comment\", \"/* no comment */\"]",
            ),
            ("/* ä */ [/* 😀 */]", "[]"),
        ]);
    }

    #[test]
    fn test_trailing_commas() {
        assert_like_json(&[
            ("[1,]", "[1]"),
            ("[1, 2 , ]", "[1, 2]"),
            ("{\"a\": 1,}", "{\"a\": 1}"),
            ("[[1,],{\"a\":[],},]", "[[1],{\"a\":[]}]"),
        ]);
        assert_errors(&[
            ("[,]", "unexpected comma"),
            ("[1,,]", "unexpected comma"),
            ("{,}", "expected map key"),
            ("{\"a\": 1,,}", "expected map key"),
        ]);
    }

    #[test]
    fn test_hjson() {
        assert_like_json(&[
            // comments
            ("# c\n1 # c", "1"),
            ("[1 # c\n, 2 // c\n, 3 /* c */]", "[1, 2, 3]"),
            // strings without quotes run to the end of the line
            (
                "[\n  a b \t\n  c, d # e ] // f\n  3 # c\n  true\n  4 5\n  nulls\n]",
                r#"["a b", "c, d # e ] // f", 3, true, "4 5", "nulls"]"#,
            ),
            (
                "[\n  -0\n  01\n  1e999\n  .5\n  1.\n  -\n  0x1\n]",
                r#"[-0, "01", "1e999", ".5", "1.", "-", "0x1"]"#,
            ),
            ("[\n  ä ö\r\n  😀\r]", r#"["ä ö", "😀"]"#),
            // optional commas
            (
                r#"{"a": "b" "c": ["d" 'e']}"#,
                r#"{"a": "b", "c": ["d", "e"]}"#,
            ),
            ("{\na: 1\nb: [\n2\n3\n]\n}", r#"{"a": 1, "b": [2, 3]}"#),
            // keys
            (
                "{a-b: 1, 'c d': 2, \"e\": 3, é/#*.: 4\n  f\n  :\n  5}",
                r#"{"a-b": 1, "c d": 2, "e": 3, "é/#*.": 4, "f": 5}"#,
            ),
            // multiline strings
            ("x:\n  '''\n  a\n   b\n\n  '''", r#"{"x": "a\n b\n"}"#),
            (
                "x: '''  \r\n     a\r\n    b'c''d'''",
                r#"{"x": "  a\n b'c''d"}"#,
            ),
            ("'''a'''", r#""a""#),
            ("ä: '''\n    a\n   '''", r#"{"ä": " a"}"#),
            ("[''' a ''', '''''']", r#"["a ", ""]"#),
            // maps without braces
            ("a: 1\nb: 2", r#"{"a": 1, "b": 2}"#),
            ("a: 1, b: x,\n", r#"{"a": 1, "b": "x,"}"#),
            ("\"a\" : [\n]\n'b': {}", r#"{"a": [], "b": {}}"#),
            ("a b: c", r#""a b: c""#),
        ]);
        assert_errors(&[
            ("{a b: 1}", "expected colon"),
            ("{:1}", "expected map key"),
            ("{a,: 1}", "expected colon"),
            ("a: 1\n}", "expected map key"),
            ("[a", "unexpected end of file"),
            ("{a: 1", "unexpected end of file"),
            ("a:", "unexpected end of file"),
            ("'''abc''", "unexpected end of file"),
            ("[1,,2]", "unexpected comma"),
            ("x: ]", "expected a value"),
            ("x: \"a\nb\"", "unexpected character in string"),
            (r#"x: "\x""#, "invalid escape in string"),
        ]);
    }
}
