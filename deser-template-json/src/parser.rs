//! The JSON parser.
//!
//! The parser is a state machine which can be suspended between tokens: if
//! the input ends within a token and more input can follow, the parser
//! returns how much of the input it consumed (up to the start of the token)
//! and continues with more input.  With the complete input (`eof`) it
//! parses a value in one go.  The state of the current container is held
//! in locals while parsing and only stored in the parser when it's
//! suspended.
#[cfg(json5)]
use alloc::format;
#[cfg(json5)]
use alloc::string::String;
use alloc::vec::Vec;
use core::str;

use deser_core::Text;
use deser_core::de::DeserializeDriver;
use deser_core::ext::{ExtValue, Number as ExactNumber};
#[cfg(not(hjson))]
use deser_core::ext::{RawFormatInfo, RawInput};
use deser_core::{Atom, Error, ErrorKind, Event, State};
#[cfg(hjson)]
use deser_core::{Implicit, ImplicitValue};

use crate::copy::extend;
#[cfg(single_quotes)]
use crate::scan::skip_to_escape_single;
use crate::scan::{EscapeScanner, is_ascii, skip_to_escape, validate_utf8_slice};

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
    /// A hexadecimal integer that does not fit into 64 bits.
    #[cfg(json5)]
    U128(u128),
    /// A negative hexadecimal integer that does not fit into 64 bits.
    #[cfg(json5)]
    I128(i128),
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
    fn emit_input(&mut self, atom: Atom<'i>) -> Result<(), Error> {
        self.0.emit(atom)
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
    #[cfg(hjson)]
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

/// What the parser continues with.
#[derive(Clone, Copy, Debug)]
enum Pending {
    /// Nothing, the next token.
    None,
    /// An incomplete string at the start of the input.
    String(PartialString),
    /// The next value is requested as raw value (see `raw_value`).
    Raw,
}

impl Pending {
    #[cfg(hjson)]
    #[inline(always)]
    fn is_none(&self) -> bool {
        matches!(self, Pending::None)
    }

    #[inline(always)]
    fn is_string(&self) -> bool {
        matches!(self, Pending::String(_))
    }

    /// Takes the incomplete string.
    #[inline(always)]
    fn take_string(&mut self) -> Option<PartialString> {
        match *self {
            Pending::String(partial) => {
                *self = Pending::None;
                Some(partial)
            }
            _ => None,
        }
    }
}

/// A JSON parser which can be suspended between tokens.
#[derive(Debug)]
pub(crate) struct Parser {
    // the outer containers, the current one is held in `container`
    stack: Vec<Container>,
    container: Container,
    expect: Expect,
    scratch: Vec<u8>,
    partial: Pending,
    // the last error was an error of a sink, the rest of the value
    // continues at the position
    recoverable: Option<usize>,
    // the format of the raw value that is requested by `Pending::Raw` if
    // it's no longer with the state (the top-level value or a value that
    // continues with more input), see `raw_value`
    #[cfg(not(hjson))]
    raw_format: Option<&'static RawFormatInfo>,
    // the number of characters of the line before the input that follows
    // (the indentation of multiline strings is relative to their column)
    #[cfg(hjson)]
    column: usize,
}

impl Default for Parser {
    fn default() -> Parser {
        Parser {
            stack: Vec::new(),
            container: Container::Top,
            expect: Expect::Value,
            scratch: Vec::new(),
            partial: Pending::None,
            recoverable: None,
            #[cfg(not(hjson))]
            raw_format: None,
            #[cfg(hjson)]
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
    /// Returns `true` if the parser is between values.
    pub(crate) fn is_idle(&self) -> bool {
        self.expect == Expect::Value
            && self.container == Container::Top
            && !self.partial.is_string()
    }

    /// Resets the parser to parse a new value.
    ///
    /// This is needed after an error.
    pub(crate) fn reset(&mut self) {
        self.stack.clear();
        self.container = Container::Top;
        self.expect = Expect::Value;
        self.partial = Pending::None;
        self.recoverable = None;
        #[cfg(not(hjson))]
        {
            self.raw_format = None;
        }
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

    /// Tells the parser that the input was consumed without it
    /// suspending, for instance a complete value or the whitespace between
    /// values in a stream.
    ///
    /// The parser tracks the column where the next input starts.
    #[cfg(hjson)]
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
            #[cfg(comments)]
            eof,
            #[cfg(hjson)]
            column: self.column,
        };
        // the scratch space is kept with the state between values (and
        // deserializations), unless it holds a string which continues with
        // more input
        if self.scratch.capacity() == 0 {
            take_scratch(&mut self.scratch, out.state_mut());
        }
        // the raw values that are passed on, and the top-level value might
        // be one of them
        #[cfg(not(hjson))]
        {
            let state = out.state_mut();
            // the request of the top-level value, or of a value whose
            // request came with the last input (the input ended before it)
            if let Some(format) = state.set_raw_format(&crate::raw::ID) {
                if self.is_idle() {
                    self.partial = Pending::Raw;
                }
                self.raw_format = Some(format);
            }
        }
        let rv = match self.run(&mut cur, eof, base, options.exact_numbers, out) {
            Ok(progress) => Ok(progress),
            Err(err) if err.offset().is_none() => Err(err.with_offset(base + cur.pos)),
            Err(err) => Err(err),
        };
        if !self.partial.is_string() && self.scratch.capacity() != 0 {
            put_scratch(&mut self.scratch, out.state_mut());
        }
        rv
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
        let mut partial = core::mem::replace(&mut self.partial, Pending::None);

        // stores the state and returns that more input is needed
        macro_rules! suspend {
            ($consumed:expr, $expect:expr) => {{
                self.container = container;
                self.expect = $expect;
                self.partial = partial;
                #[cfg(hjson)]
                {
                    self.column = advance_column(self.column, &input[..$consumed]);
                }
                return Ok(Progress::NeedMore($consumed));
            }};
        }

        // fails with an error of a sink.  The state is stored as if the
        // event was accepted so that the rest of the value can be skipped.
        macro_rules! sink {
            ($rv:expr, $expect:expr) => {
                if let Err(err) = $rv {
                    // the next value is requested as raw value, this is
                    // not an error
                    match sink_error(err) {
                        None => partial = Pending::Raw,
                        Some(err) => {
                            self.container = container;
                            self.expect = $expect;
                            self.recoverable = Some(cur.pos);
                            return Err(err);
                        }
                    }
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
                let rv = cur.parse_str(scratch, $start, partial.take_string());
                if cur.hit_end && !eof {
                    partial = Pending::String(PartialString {
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
                // a sequence of raw values requests the value after its
                // last one
                #[cfg(not(hjson))]
                {
                    partial = Pending::None;
                }
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
                    #[cfg(single_quotes)]
                    b'\'' => {
                        cur.bump();
                        string!(start, Expect::Key)
                    }
                    #[cfg(json5)]
                    b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' | b'\\' | 0x80..=0xff => {
                        cur.hit_end = false;
                        let rv = cur.detached(|cur| cur.parse_identifier(scratch));
                        if cur.hit_end && !eof {
                            suspend!(start, Expect::Key)
                        }
                        rv?
                    }
                    #[cfg(hjson)]
                    b'{' | b'}' | b'[' | b']' | b',' | b':' => {
                        return Err(token_error(base + start, "expected map key"));
                    }
                    // keys without quotes end at whitespace and punctuators
                    #[cfg(hjson)]
                    _ => {
                        cur.hit_end = false;
                        let rv = cur.detached(|cur| cur.parse_quoteless_key());
                        if cur.hit_end && !eof {
                            suspend!(start, Expect::Key)
                        }
                        rv?
                    }
                    #[cfg(not(hjson))]
                    _ => return Err(token_error(base + start, "expected map key")),
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
                    _ => return Err(Error::new(ErrorKind::Syntax, "expected colon")),
                }
            };
        }

        // after `{`: the map ends or the first key follows.  Evaluates to
        // `true` if a value follows.
        macro_rules! open_map {
            () => {{
                let byte = if partial.is_string() {
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
        #[cfg(hjson)]
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
        #[cfg(hjson)]
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

        // emits a number, the cursor is after its first byte
        #[cfg(not(hjson))]
        macro_rules! number {
            ($byte:expr, $start:expr) => {{
                let lead = cur.number_lead($byte);
                if !matches!(lead, Lead::Integer(_)) {
                    cur.number_start = $start;
                    cur.truncated = false;
                }
                let rv = match lead {
                    Lead::Integer(number) => Ok(number),
                    Lead::Fraction(nonnegative, significand) => {
                        cur.detached(|cur| cur.parse_decimal(nonnegative, significand, 0))
                    }
                    Lead::Exponent(nonnegative, significand) => cur.detached(|cur| {
                        cur.parse_exponent(nonnegative, significand, 0)
                            .map(Number::Literal)
                    }),
                    Lead::Other => {
                        cur.pos = $start + 1;
                        match $byte {
                            b'-' => {
                                let first_digit = cur.next_or_nul();
                                cur.detached(|cur| cur.parse_integer(false, first_digit))
                            }
                            #[cfg(json5)]
                            b'+' => {
                                let first_digit = cur.next_or_nul();
                                cur.detached(|cur| cur.parse_integer(true, first_digit))
                            }
                            byte => cur.detached(|cur| cur.parse_integer(true, byte)),
                        }
                    }
                };
                // the number might continue
                if cur.hit_end && !eof {
                    suspend!($start, Expect::Value)
                }
                let number = rv?;
                out.state_mut()
                    .set_input_range(base + $start, base + cur.pos);
                sink!(
                    emit_number(out, number, input, exact_numbers, $start, cur.pos),
                    Expect::AfterValue
                )
            }};
        }

        // emits a value without quotes: a number, `true`, `false` or `null`
        // if only whitespace, a comma, the end of a container or a comment
        // follows it on the line, otherwise the rest of the line is a
        // string.
        #[cfg(hjson)]
        macro_rules! quoteless {
            ($start:expr) => {{
                cur.pos = $start;
                let rv = cur.detached(|cur| cur.parse_quoteless());
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
        #[cfg(hjson)]
        macro_rules! multiline_string {
            ($start:expr) => {{
                cur.hit_end = false;
                let rv = cur.detached(|cur| cur.parse_multiline_str(scratch, $start));
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
            #[cfg(hjson)]
            Expect::KeyOrEnd if container == Container::Braceless => !open_braceless!(),
            Expect::KeyOrEnd => !open_map!(),
            Expect::Key => {
                let byte = if partial.is_string() {
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
                let byte = match partial {
                    Pending::None => next_byte!(Expect::Value),
                    Pending::String(_) => b'"',
                    // the value is requested as raw value: it's validated
                    // and passed on as it is.  This is handled on its own
                    // so that it does not affect the code of other values.
                    #[cfg(not(hjson))]
                    Pending::Raw => {
                        // the whitespace before the value
                        let _ = next_byte!(Expect::Value);
                        let start = cur.pos;
                        partial = Pending::None;
                        // the format the raw value requested, from the
                        // request if it was not kept (see `raw_format`)
                        let format = match self.raw_format.take() {
                            Some(format) => format,
                            None => match out.state_mut().take_raw_request() {
                                Some(format) => format,
                                None => return Err(raw_without_format()),
                            },
                        };
                        match cur.detached(|cur| raw_value(cur, scratch, eof, base, format, out)) {
                            RawValue::Emitted(rv) => sink!(rv, Expect::AfterValue),
                            RawValue::Incomplete => {
                                partial = Pending::Raw;
                                self.raw_format = Some(format);
                                suspend!(start, Expect::Value)
                            }
                            RawValue::Failed(err) => return Err(err),
                        }
                        skip_value = true;
                        continue 'value;
                    }
                    #[cfg(hjson)]
                    Pending::Raw => unreachable!("Hjson has no raw values"),
                };
                // a map key at the root starts a map without braces
                #[cfg(hjson)]
                if container == Container::Top && partial.is_none() && byte != b'{' && byte != b'['
                {
                    match cur.detached(|cur| cur.is_map_key()) {
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
                    #[cfg(json5)]
                    b'\'' => string_value!(start),
                    // `'''` starts a multiline string
                    #[cfg(hjson)]
                    b'\'' => match input.get(start + 1..start + 3) {
                        Some(b"''") => multiline_string!(start),
                        None if !eof => suspend!(start, Expect::Value),
                        _ => string_value!(start),
                    },
                    #[cfg(not(hjson))]
                    b'0'..=b'9' | b'-' => number!(byte, start),
                    // `+1`, `.5`, `Infinity` and `NaN`
                    #[cfg(json5)]
                    b'+' | b'.' | b'I' | b'N' => number!(byte, start),
                    #[cfg(not(hjson))]
                    b'n' | b't' | b'f' => {
                        let (rest, event): (&[u8], _) = match byte {
                            b'n' => (b"ull", Event::Atom(Atom::Null)),
                            b't' => (b"rue", Event::from(true)),
                            _ => (b"alse", Event::from(false)),
                        };
                        let rv = cur.detached(|cur| cur.parse_ident(rest));
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
                    #[cfg(not(hjson))]
                    _ => return Err(token_error(base + start, "unexpected character")),
                    #[cfg(hjson)]
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
                    #[cfg(hjson)]
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
                        #[cfg(not(trailing_commas))]
                        {
                            if container == Container::Map {
                                let byte = next_byte!(Expect::Key);
                                key!(byte);
                            }
                            continue 'value;
                        }
                        // the container can end after the comma
                        #[cfg(trailing_commas)]
                        {
                            let more = if container == Container::Map {
                                open_map!()
                            } else {
                                open_seq!()
                            };
                            if more {
                                continue 'value;
                            }
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
                            ErrorKind::Syntax,
                            if container == Container::Map {
                                "unexpected end of seq"
                            } else {
                                "unexpected end of map"
                            },
                        ));
                    }
                    #[cfg(not(hjson))]
                    _ => {
                        return Err(Error::new(ErrorKind::Syntax, "expected a comma"));
                    }
                    // the comma between values is optional
                    #[cfg(hjson)]
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

/// What happened to a value that is requested as raw value.
#[cfg(not(hjson))]
enum RawValue {
    /// It was emitted, the result is the one of the sink.
    Emitted(Result<(), Error>),
    /// It's incomplete, it's parsed again with more input.
    Incomplete,
    /// It's invalid.
    Failed(Error),
}

/// Skips a value that is requested as raw value and emits its input, the
/// cursor is at its first byte.
#[cfg(not(hjson))]
#[cold]
#[inline(never)]
fn raw_value<'i, O: Out<'i>>(
    cur: &mut Cursor<'i>,
    scratch: &mut Vec<u8>,
    eof: bool,
    base: usize,
    format: &'static RawFormatInfo,
    out: &mut O,
) -> RawValue {
    let start = cur.pos;
    // the scanner comes with the description of the format of the raw value
    // (see `raw::Scanner`): programs without raw values do not contain it
    let scanned = match format
        .data()
        .and_then(|data| data.downcast_ref::<crate::raw::Scanner>())
    {
        Some(scanner) => (scanner.0)(cur, scratch, eof, base, out.state_mut()),
        None => Err(Error::new(
            ErrorKind::InvalidState,
            "raw value of an unknown format requested",
        )),
    };
    match scanned {
        Ok(true) => {}
        Ok(false) => return RawValue::Incomplete,
        Err(err) => return RawValue::Failed(err),
    }
    let input = &cur.input[start..cur.pos];
    // SAFETY: the value was validated (see `skip_raw`).  The input is valid
    // UTF-8: strings were validated if the input is a byte slice,
    // everything else is ASCII (or validated like comments).
    // The format is the one of the raw value that requested it, which is
    // this dialect: referring to its description here would bring its
    // functions (like the serializer) into every program.
    let value = unsafe { RawInput::new(input, format) };
    out.state_mut()
        .set_input_range(base + start, base + cur.pos);
    RawValue::Emitted(out.emit_input(Atom::Ext(ExtValue::owned_value::<RawInput>(value))))
}

/// Returns `true` if the result of an event requests the next value as raw
/// value (see `Error::is_raw_request`).
#[cfg(not(hjson))]
#[inline(always)]
fn is_raw_request(err: &Error) -> bool {
    err.is_raw_request()
}

/// Returns the error of an event or `None` if it requests the next value as
/// raw value.  This is out of line so that the error handling after every
/// event stays small.
#[cold]
#[inline(never)]
fn sink_error(err: Error) -> Option<Error> {
    if is_raw_request(&err) {
        None
    } else {
        Some(err)
    }
}

/// The error if a raw value is requested without the description of its
/// format (see `State::take_raw_request`), which sinks never do.
#[cfg(not(hjson))]
#[cold]
fn raw_without_format() -> Error {
    Error::new(
        ErrorKind::InvalidState,
        "raw value requested without a format",
    )
}

//# Hjson has no raw values, it never declares that it passes them on so
//# sinks never request them.
#[cfg(hjson)]
#[inline(always)]
fn is_raw_request(_err: &Error) -> bool {
    false
}

/// Skips a value that is wanted as raw value while validating it, the
/// cursor is at its first byte.
///
/// Returns `false` if the input ends within the value (or after a number
/// which could continue) and more input can follow.  Then the value is
/// skipped again once more input is there.
#[cfg(not(any(comments, trailing_commas, single_quotes, json5, hjson)))]
pub(crate) fn skip_raw(
    cur: &mut Cursor<'_>,
    scratch: &mut Vec<u8>,
    eof: bool,
    _base: usize,
    _state: &mut State,
) -> Result<bool, Error> {
    cur.skip_value(scratch, eof)
}

//# The dialects parse the value without passing on its events, which
//# validates their extensions to JSON like the rest of the input.
#[cfg(all(not(hjson), any(comments, trailing_commas, single_quotes, json5)))]
pub(crate) fn skip_raw(
    cur: &mut Cursor<'_>,
    _scratch: &mut Vec<u8>,
    eof: bool,
    base: usize,
    state: &mut State,
) -> Result<bool, Error> {
    let options = Options {
        validate_utf8: cur.validate_utf8,
        exact_numbers: false,
    };
    match Parser::default().parse(cur.input, cur.pos, eof, base, options, &mut Skip(state))? {
        Progress::Done(end) => {
            cur.pos = end;
            Ok(true)
        }
        Progress::NeedMore(_) => Ok(false),
    }
}

/// Discards the events of a value that is skipped (see `skip_raw`).
#[cfg(all(not(hjson), any(comments, trailing_commas, single_quotes, json5)))]
struct Skip<'a>(&'a mut State);

#[cfg(all(not(hjson), any(comments, trailing_commas, single_quotes, json5)))]
impl<'i> Out<'i> for Skip<'_> {
    #[inline(always)]
    fn state_mut(&mut self) -> &mut State {
        self.0
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

/// Takes the scratch space that was kept with the state.
#[inline(never)]
fn take_scratch(scratch: &mut Vec<u8>, state: &mut State) {
    *scratch = state.__private_take_scratch();
}

/// Keeps the scratch space with the state.
#[inline(never)]
fn put_scratch(scratch: &mut Vec<u8>, state: &mut State) {
    state.__private_put_scratch(core::mem::take(scratch));
}

#[cold]
fn eof_error() -> Error {
    Error::new(ErrorKind::EndOfFile, "unexpected end of file")
}

/// The start of a number (see [`Cursor::number_lead`]).
#[cfg(not(hjson))]
enum Lead<'a> {
    /// An integer, the number is complete.
    Integer(Number<'a>),
    /// The integer part (with the sign) of a number with a fraction, the
    /// cursor is at the decimal point.
    Fraction(bool, u64),
    /// The integer part (with the sign) of a number with an exponent, the
    /// cursor is at the `e`.
    Exponent(bool, u64),
    /// Any other number, it's parsed from the start.
    Other,
}

/// Reads tokens from the input.
///
/// The parser keeps its cursor in registers: functions that are not
/// inlined get a copy of it (see [`detached`](Self::detached)).
#[derive(Clone, Copy)]
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
    #[cfg(comments)]
    eof: bool,
    // the number of characters of the line before the input
    #[cfg(hjson)]
    column: usize,
}

/// A comment in the input (see [`Cursor::skip_comment`]).
#[cfg(comments)]
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
            //# comments are always validated, for strings this is a no-op
            validate_utf8: cfg!(comments),
            hit_end: false,
            partial: (0, 0),
            number_start: 0,
            truncated: false,
            #[cfg(comments)]
            eof: true,
            #[cfg(hjson)]
            column: 0,
        }
    }

    /// Creates a cursor to skip whitespace in an input that more input
    /// might follow.
    #[cfg(comments)]
    pub(crate) fn new_partial(input: &'a [u8], pos: usize, eof: bool) -> Cursor<'a> {
        Cursor {
            eof,
            ..Cursor::new(input, pos)
        }
    }

    /// Calls a function which is not inlined with a copy of the cursor and
    /// takes the copy back.
    ///
    /// If the address of the cursor of the parser was passed to a function,
    /// the cursor would have to be kept in memory and every token would
    /// load and store the position.
    #[inline(always)]
    fn detached<R>(&mut self, f: impl FnOnce(&mut Cursor<'a>) -> R) -> R {
        let mut copy = *self;
        let rv = f(&mut copy);
        // the input and the options do not change, they stay in registers
        self.pos = copy.pos;
        self.hit_end = copy.hit_end;
        self.partial = copy.partial;
        self.number_start = copy.number_start;
        self.truncated = copy.truncated;
        rv
    }

    /// Parses a string, the cursor is after the opening quote at `start`.
    ///
    /// An incomplete string continues where it stopped.  If the string is
    /// incomplete, `partial` holds where it continues.
    ///
    /// Most strings have no escapes, they are slices of the input.  These
    /// are handled here, inlined into the parser, the others by
    /// [`parse_str_slow`](Self::parse_str_slow).
    #[inline(always)]
    fn parse_str<'b>(
        &mut self,
        buffer: &'b mut Vec<u8>,
        start: usize,
        resume: Option<PartialString>,
    ) -> Result<Str<'a, 'b>, Error> {
        //# strings in single quotes end at a single quote
        #[cfg(single_quotes)]
        let plain = resume.is_none() && self.input[start] != b'\'';
        #[cfg(not(single_quotes))]
        let plain = resume.is_none();
        if plain {
            let end = skip_to_escape(self.input, self.pos);
            if self.input.get(end) == Some(&b'"') {
                let bytes = &self.input[self.pos..end];
                if !self.validate_utf8 || is_ascii(bytes) || validate_utf8_slice(bytes) {
                    self.pos = end + 1;
                    // SAFETY: the input is valid UTF-8 as it comes from a
                    // `&str` or was validated above.  The slice starts and
                    // ends at ASCII characters (quotes).
                    return Ok(Str::Borrowed(unsafe { str::from_utf8_unchecked(bytes) }));
                }
            }
        }
        self.detached(|cur| cur.parse_str_slow(buffer, start, resume))
    }

    /// Parses a string that is incomplete or has escapes (see
    /// [`parse_str`](Self::parse_str)).
    #[inline(never)]
    fn parse_str_slow<'b>(
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
                return Err(Error::new(ErrorKind::Syntax, "invalid utf-8 in string"));
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
        #[cfg(single_quotes)]
        let single = self.input[start] == b'\'';

        let mut escapes = EscapeScanner::new();
        loop {
            #[cfg(not(single_quotes))]
            {
                self.pos = escapes.next(self.input, self.pos);
            }
            #[cfg(single_quotes)]
            {
                self.pos = if single {
                    skip_to_escape_single(self.input, self.pos)
                } else {
                    escapes.next(self.input, self.pos)
                };
            }
            if self.pos == self.input.len() {
                self.hit_end = true;
                self.partial = (self.pos, copied);
                return Err(Error::new(ErrorKind::Syntax, "unexpected end of string"));
            }
            let byte = self.input[self.pos];
            // the closing single quote is handled like a double quote
            #[cfg(single_quotes)]
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
                        extend(buffer, &self.input[copied..self.pos]);
                        self.pos += 1;
                        return result(validate_utf8, buffer).map(Str::Scratch);
                    }
                }
                b'\\' => {
                    extend(buffer, &self.input[copied..self.pos]);
                    // the common escapes stand for a single byte
                    if let Some(&byte) = self.input.get(self.pos + 1)
                        && let unescaped @ 1.. = UNESCAPE[usize::from(byte)]
                    {
                        buffer.push(unescaped);
                        self.pos += 2;
                        copied = self.pos;
                        continue;
                    }
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
                #[cfg(single_quotes)]
                byte if byte != b'\n' && byte != b'\r' => self.pos += 1,
                _ => {
                    return Err(Error::new(
                        ErrorKind::Syntax,
                        "unexpected character in string",
                    ));
                }
            }
        }
    }

    /// Skips a value while validating it, the cursor is at its first byte.
    ///
    /// Returns `false` if the input ends within the value (or after a
    /// number which could continue) and more input can follow.  Then the
    /// value is skipped again once more input is there.
    #[cfg(not(any(comments, trailing_commas, single_quotes, json5, hjson)))]
    #[inline(never)]
    fn skip_value(&mut self, scratch: &mut Vec<u8>, eof: bool) -> Result<bool, Error> {
        self.hit_end = false;
        match self.skip_value_inner(scratch) {
            _ if self.hit_end && !eof => Ok(false),
            rv => rv.map(|()| true),
        }
    }

    #[cfg(not(any(comments, trailing_commas, single_quotes, json5, hjson)))]
    fn skip_value_inner(&mut self, scratch: &mut Vec<u8>) -> Result<(), Error> {
        // the open containers, `true` for maps.  The innermost 64 are bits,
        // the ones below them go to `outer`.
        let mut depth = 0usize;
        let mut maps = 0u64;
        let mut outer = Vec::new();
        macro_rules! push {
            ($is_map:expr) => {{
                if depth >= 64 {
                    outer.push(maps >> 63 != 0);
                }
                maps = (maps << 1) | u64::from($is_map);
                depth += 1;
            }};
        }
        macro_rules! pop {
            () => {{
                depth -= 1;
                maps >>= 1;
                if depth >= 64 {
                    maps |= u64::from(outer.pop().unwrap_or(false)) << 63;
                }
            }};
        }
        loop {
            let Some(byte) = self.parse_whitespace() else {
                return Err(eof_error());
            };
            let start = self.pos;
            self.pos += 1;
            match byte {
                b'"' => {
                    self.parse_str(scratch, start, None)?;
                }
                b'-' | b'0'..=b'9' => self.skip_number(byte)?,
                b'n' => self.parse_ident(b"ull")?,
                b't' => self.parse_ident(b"rue")?,
                b'f' => self.parse_ident(b"alse")?,
                b'[' => {
                    if self.parse_whitespace() == Some(b']') {
                        self.pos += 1;
                    } else {
                        push!(false);
                        continue;
                    }
                }
                b'{' => {
                    if self.parse_whitespace() == Some(b'}') {
                        self.pos += 1;
                    } else {
                        push!(true);
                        self.skip_key(scratch)?;
                        continue;
                    }
                }
                // errors without offset are located at the cursor
                _ => {
                    self.pos = start;
                    return Err(Error::new(
                        ErrorKind::Syntax,
                        match byte {
                            b',' => "unexpected comma",
                            b':' => "unexpected colon",
                            b']' | b'}' => "expected a value",
                            _ => "unexpected character",
                        },
                    ));
                }
            }
            // a value was completed, either the container ends or the next
            // value follows
            loop {
                if depth == 0 {
                    return Ok(());
                }
                let is_map = maps & 1 != 0;
                match self.parse_whitespace() {
                    Some(b',') => {
                        self.pos += 1;
                        if is_map {
                            self.skip_key(scratch)?;
                        }
                        break;
                    }
                    Some(b']') if !is_map => {
                        self.pos += 1;
                        pop!();
                    }
                    Some(b'}') if is_map => {
                        self.pos += 1;
                        pop!();
                    }
                    Some(b']' | b'}') => {
                        return Err(Error::new(
                            ErrorKind::Syntax,
                            if is_map {
                                "unexpected end of seq"
                            } else {
                                "unexpected end of map"
                            },
                        ));
                    }
                    Some(_) => {
                        return Err(Error::new(ErrorKind::Syntax, "expected a comma"));
                    }
                    None => return Err(eof_error()),
                }
            }
        }
    }

    /// Skips a map key and the colon after it.
    #[cfg(not(any(comments, trailing_commas, single_quotes, json5, hjson)))]
    fn skip_key(&mut self, scratch: &mut Vec<u8>) -> Result<(), Error> {
        match self.parse_whitespace() {
            Some(b'"') => {
                let start = self.pos;
                self.pos += 1;
                self.parse_str(scratch, start, None)?;
            }
            Some(_) => return Err(Error::new(ErrorKind::Syntax, "expected map key")),
            None => return Err(eof_error()),
        }
        match self.parse_whitespace() {
            Some(b':') => {
                self.pos += 1;
                Ok(())
            }
            Some(_) => Err(Error::new(ErrorKind::Syntax, "expected colon")),
            None => Err(eof_error()),
        }
    }

    /// Skips a number while validating it, the cursor is after its first
    /// byte.
    #[cfg(not(any(comments, trailing_commas, single_quotes, json5, hjson)))]
    fn skip_number(&mut self, first: u8) -> Result<(), Error> {
        let input = self.input;
        let mut pos = self.pos;
        let digits = |pos: &mut usize| {
            let start = *pos;
            while input.get(*pos).is_some_and(u8::is_ascii_digit) {
                *pos += 1;
            }
            *pos - start
        };
        let first_digit = if first == b'-' {
            match input.get(pos) {
                Some(&digit @ b'0'..=b'9') => {
                    pos += 1;
                    digit
                }
                _ => return self.invalid_number(pos),
            }
        } else {
            first
        };
        if first_digit != b'0' {
            digits(&mut pos);
        }
        if input.get(pos) == Some(&b'.') {
            pos += 1;
            if digits(&mut pos) == 0 {
                return self.invalid_number(pos);
            }
        }
        if let Some(b'e' | b'E') = input.get(pos) {
            pos += 1;
            if let Some(b'+' | b'-') = input.get(pos) {
                pos += 1;
            }
            if digits(&mut pos) == 0 {
                return self.invalid_number(pos);
            }
        }
        // the number could continue
        if pos == input.len() {
            self.hit_end = true;
        }
        self.pos = pos;
        Ok(())
    }

    #[cfg(not(any(comments, trailing_commas, single_quotes, json5, hjson)))]
    #[cold]
    fn invalid_number(&mut self, pos: usize) -> Result<(), Error> {
        self.pos = pos;
        if pos == self.input.len() {
            self.hit_end = true;
            return Err(eof_error());
        }
        Err(Error::new(ErrorKind::Syntax, "invalid number"))
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

    #[inline]
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

    /// Consumes up to eight digits and returns their value and how many
    /// there were.
    ///
    /// The next eight bytes are loaded as a word, the digits at its start
    /// are combined at once.  If there are fewer than eight digits, the byte
    /// after them is not a digit.  Returns `None` if fewer than eight bytes
    /// are left, the digits at the end of the input are parsed one by one.
    #[inline(always)]
    fn digits(&mut self) -> Option<(u64, usize)> {
        let bytes = self.input.get(self.pos..self.pos + 8)?;
        let value = u64::from_le_bytes(bytes.try_into().unwrap());
        let digits = value.wrapping_sub(0x3030_3030_3030_3030);
        let above = value.wrapping_add(0x4646_4646_4646_4646);
        // the high bit of bytes below b'0' or above b'9' is set.  Only the
        // first of them is exact: the bytes before it are digits which do
        // not borrow or carry into it.
        let other = (digits | above) & 0x8080_8080_8080_8080;
        // Eight digits and no digits are separate branches, so that the
        // position after them does not depend on the data (a branch is
        // predicted, the next load can start right away).
        if other == 0 {
            self.pos += 8;
            return Some((combine_digits(digits), 8));
        }
        if other & 0x80 != 0 {
            return Some((0, 0));
        }
        let count = (other.trailing_zeros() / 8) as usize;
        self.pos += count;
        // move the digits to the top of the word, the bytes below them are
        // zeros (leading zeros of the number)
        Some((combine_digits(digits << (64 - 8 * count)), count))
    }

    /// Parses the integer part of a number of up to nine digits, the
    /// cursor is after its first byte.
    ///
    /// This is inlined into the parser: integers are complete, the
    /// fraction or exponent of a float continues out of line.  Everything
    /// else (and numbers near the end of the input) is
    /// [`Lead::Other`], the cursor is anywhere then.
    #[cfg(not(hjson))]
    #[inline(always)]
    fn number_lead(&mut self, first: u8) -> Lead<'a> {
        let nonnegative = first != b'-';
        let first = if nonnegative {
            first
        } else {
            match self.input.get(self.pos) {
                Some(&first) => {
                    self.pos += 1;
                    first
                }
                None => return Lead::Other,
            }
        };
        //# a leading zero might be a hexadecimal number in JSON5
        if !matches!(first, b'1'..=b'9') {
            return Lead::Other;
        }
        let Some((digits, count)) = self.digits() else {
            return Lead::Other;
        };
        if count == 8 {
            return Lead::Other;
        }
        let value = u64::from(first - b'0') * POW10_U64[count] + digits;
        // the digits ended before the end of the input (there were eight
        // bytes)
        match self.input[self.pos] {
            b'.' => Lead::Fraction(nonnegative, value),
            b'e' | b'E' => Lead::Exponent(nonnegative, value),
            _ if nonnegative => Lead::Integer(Number::U64(value)),
            _ => Lead::Integer(Number::I64(-(value as i64))),
        }
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
            #[cfg(single_quotes)]
            b'\'' => buffer.push(b'\''),
            #[cfg(json5)]
            b'v' => buffer.push(b'\x0b'),
            // `\0` must not be followed by a digit
            #[cfg(json5)]
            b'0' => match self.peek() {
                Some(b'0'..=b'9') => return Err(invalid_escape()),
                Some(_) => buffer.push(b'\0'),
                None => return Err(eof_error()),
            },
            #[cfg(json5)]
            b'x' => {
                let mut n = 0;
                for _ in 0..2 {
                    match char::from(self.next_or_eof()?).to_digit(16) {
                        Some(digit) => n = n * 16 + digit,
                        None => {
                            return Err(Error::new(ErrorKind::Syntax, "invalid hex escape"));
                        }
                    }
                }
                let c = char::from_u32(n).unwrap();
                buffer.extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
            }
            // escaped line breaks are removed
            #[cfg(json5)]
            b'\n' => {}
            #[cfg(json5)]
            b'\r' => match self.peek() {
                Some(b'\n') => self.bump(),
                Some(_) => {}
                None => return Err(eof_error()),
            },
            #[cfg(json5)]
            0xe2 => match self.input.get(self.pos..self.pos + 2) {
                // the line and paragraph separators
                Some([0x80, 0xa8 | 0xa9]) => self.pos += 2,
                Some(_) => buffer.push(0xe2),
                None => {
                    self.hit_end = true;
                    return Err(eof_error());
                }
            },
            #[cfg(json5)]
            b'1'..=b'9' => return Err(invalid_escape()),
            // other characters stand for themselves.  The rest of a
            // character that is not ASCII is copied with the string.
            #[cfg(json5)]
            byte => buffer.push(byte),
            #[cfg(not(json5))]
            _ => return Err(invalid_escape()),
        }

        Ok(())
    }

    /// Parses an identifier (a map key without quotes).
    ///
    /// Identifiers are borrowed from the input unless they contain
    /// escapes, then they are copied into the buffer.
    #[cfg(json5)]
    fn parse_identifier<'b>(&mut self, buffer: &'b mut Vec<u8>) -> Result<Str<'a, 'b>, Error> {
        let start = self.pos;
        // the first byte not yet copied into the buffer after an escape
        let mut copied = None;
        loop {
            let first = self.pos == start;
            match self.peek() {
                Some(b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$') => self.bump(),
                Some(b'0'..=b'9') if !first => self.bump(),
                Some(b'\\') => {
                    let from = match copied {
                        Some(from) => from,
                        None => {
                            buffer.clear();
                            start
                        }
                    };
                    buffer.extend_from_slice(&self.input[from..self.pos]);
                    let escape = self.pos;
                    self.bump();
                    let c = match self.next_or_eof()? {
                        b'u' => char::from_u32(u32::from(self.decode_hex_escape()?)),
                        _ => None,
                    };
                    // the error is reported at the escape
                    let Some(c) = c.filter(|&c| is_identifier_char(c, first)) else {
                        self.pos = escape;
                        return Err(invalid_identifier());
                    };
                    buffer.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
                    copied = Some(self.pos);
                }
                Some(byte @ 0x80..=0xff) => {
                    let len = match byte {
                        0xc0..=0xdf => 2,
                        0xe0..=0xef => 3,
                        _ => 4,
                    };
                    let Some(bytes) = self.input.get(self.pos..self.pos + len) else {
                        self.hit_end = true;
                        return Err(eof_error());
                    };
                    match str::from_utf8(bytes).ok().and_then(|s| s.chars().next()) {
                        Some(c) if is_identifier_char(c, first) => self.pos += len,
                        Some(_) => break,
                        None => return Err(Error::new(ErrorKind::Syntax, "invalid utf-8")),
                    }
                }
                _ => break,
            }
        }
        if self.pos == start {
            return Err(Error::new(ErrorKind::Syntax, "expected map key"));
        }
        match copied {
            Some(from) => {
                buffer.extend_from_slice(&self.input[from..self.pos]);
                // SAFETY: the buffer holds validated characters and the
                // decoded escapes
                Ok(Str::Scratch(unsafe { str::from_utf8_unchecked(buffer) }))
            }
            // SAFETY: the identifier is ASCII or was validated above
            None => Ok(Str::Borrowed(unsafe {
                str::from_utf8_unchecked(&self.input[start..self.pos])
            })),
        }
    }

    /// Parses a map key without quotes.
    ///
    /// The key ends at whitespace or a punctuator (`{}[],:`), it's
    /// borrowed from the input.
    #[cfg(hjson)]
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
            return Err(Error::new(ErrorKind::Syntax, "invalid utf-8 in map key"));
        }
        // SAFETY: the key is valid UTF-8 as it comes from a `&str` or was
        // validated above, it ends at an ASCII character
        Ok(Str::Borrowed(unsafe { str::from_utf8_unchecked(key) }))
    }

    /// Returns `true` if a map key followed by a colon is at the cursor.
    ///
    /// This tells if the value at the root is a map without braces.
    /// Returns `None` if more input is needed to tell.
    #[cfg(hjson)]
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
    #[cfg(hjson)]
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
            return Err(Error::new(ErrorKind::Syntax, "invalid utf-8 in string"));
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
    #[cfg(hjson)]
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
            return Err(Error::new(ErrorKind::Syntax, "invalid utf-8 in string"));
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
                    return Err(Error::new(ErrorKind::Syntax, "invalid hex escape"));
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
                // vertical tab and form feed
                #[cfg(json5)]
                Some(0x0b | 0x0c) => pos += 1,
                #[cfg(all(comments, not(hjson)))]
                Some(b'/') => match self.skip_comment(pos) {
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
                // `#` starts a line comment too
                #[cfg(hjson)]
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
                #[cfg(json5)]
                Some(&byte) if byte >= 0x80 => match unicode_whitespace(&input[pos..]) {
                    Some(len) => pos += len,
                    // the character is completed with more input
                    None if input.len() - pos < 3 && !self.eof => {
                        self.pos = pos;
                        self.hit_end = true;
                        return None;
                    }
                    None => {
                        self.pos = pos;
                        return Some(byte);
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
    #[cfg(comments)]
    #[cold]
    fn skip_comment(&self, pos: usize) -> Comment {
        let input = self.input;
        #[cfg(hjson)]
        let hash = input[pos] == b'#';
        let end = match input.get(pos + 1) {
            #[cfg(hjson)]
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

    #[cfg(not(hjson))]
    fn parse_ident(&mut self, ident: &[u8]) -> Result<(), Error> {
        for expected in ident {
            match self.next() {
                None => {
                    return Err(Error::new(ErrorKind::EndOfFile, "unexpected end of file"));
                }
                Some(next) => {
                    if next != *expected {
                        return Err(Error::new(ErrorKind::Syntax, "unexpected character"));
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
                    ErrorKind::Syntax,
                    "only a single leading 0 is allowed",
                )),
                #[cfg(json5)]
                b'x' | b'X' => self.parse_hex(nonnegative),
                _ => self.parse_number(nonnegative, 0),
            },
            // a leading decimal point
            #[cfg(json5)]
            b'.' => match self.peek_or_nul() {
                b'0'..=b'9' => {
                    self.pos -= 1;
                    self.parse_decimal(nonnegative, 0, 0)
                }
                _ => Err(Error::new(ErrorKind::Syntax, "expected a digit")),
            },
            #[cfg(json5)]
            b'I' => {
                self.parse_ident(b"nfinity")?;
                Ok(Number::F64(if nonnegative {
                    f64::INFINITY
                } else {
                    f64::NEG_INFINITY
                }))
            }
            #[cfg(json5)]
            b'N' => {
                self.parse_ident(b"aN")?;
                Ok(Number::F64(f64::NAN))
            }
            c @ b'1'..=b'9' => {
                let mut res = u64::from(c - b'0');
                // up to eight digits at a time while they cannot overflow
                while res < EIGHT_DIGITS_LIMIT
                    && let Some((digits, count)) = self.digits()
                {
                    res = res * POW10_U64[count] + digits;
                    if count < 8 {
                        return self.parse_number(nonnegative, res);
                    }
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
            _ => Err(Error::new(ErrorKind::Syntax, "invalid integer")),
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
        #[cfg(not(json5))]
        let mut at_least_one_digit = false;
        let mut overflowed = false;
        // up to eight digits at a time while they cannot overflow
        while significand < EIGHT_DIGITS_LIMIT
            && let Some((digits, count)) = self.digits()
        {
            significand = significand * POW10_U64[count] + digits;
            exponent -= count as i32;
            #[cfg(not(json5))]
            {
                at_least_one_digit |= count > 0;
            }
            if count < 8 {
                break;
            }
        }
        while let c @ b'0'..=b'9' = self.peek_or_nul() {
            self.bump();
            let digit = u64::from(c - b'0');
            #[cfg(not(json5))]
            {
                at_least_one_digit = true;
            }

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

        //# JSON5 allows a trailing decimal point
        #[cfg(not(json5))]
        if !at_least_one_digit {
            return Err(Error::new(ErrorKind::Syntax, "expected a digit"));
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

    /// Parses a hexadecimal integer, the cursor is at the `x`.
    ///
    /// Integers that do not fit into 128 bits are approximated as floats
    /// (like ECMAScript does for all integers beyond 53 bits).
    #[cfg(json5)]
    fn parse_hex(&mut self, nonnegative: bool) -> Result<Number<'a>, Error> {
        self.bump();
        let mut value = 0u128;
        // the digits that do not fit into 128 bits, and if one is not zero
        let mut dropped = 0u64;
        let mut sticky = false;
        let mut digits = 0;
        while let Some(digit) = char::from(self.peek_or_nul()).to_digit(16) {
            self.bump();
            digits += 1;
            match value.checked_mul(16) {
                Some(shifted) if dropped == 0 => value = shifted | u128::from(digit),
                _ => {
                    dropped += 1;
                    sticky |= digit != 0;
                }
            }
        }
        if digits == 0 {
            return Err(Error::new(ErrorKind::Syntax, "expected a hex digit"));
        }
        if dropped > 0 {
            // the value has more than 64 significant bits, the lowest bit
            // stands for the dropped digits so that the value is rounded
            // correctly.  The scaling by a power of two is exact.
            // 2^(4 * dropped), infinite beyond the range of floats
            let scale = f64::from_bits((1023 + 4 * dropped.min(256)) << 52);
            let float = (value | u128::from(sticky)) as f64 * scale;
            if float.is_infinite() {
                return Err(number_out_of_range());
            }
            return Ok(Number::F64(if nonnegative { float } else { -float }));
        }
        Ok(match value {
            value if nonnegative => match u64::try_from(value) {
                Ok(value) => Number::U64(value),
                Err(_) => Number::U128(value),
            },
            value if value <= 1 << 63 => Number::I64((value as i64).wrapping_neg()),
            value if value <= 1 << 127 => Number::I128((value as i128).wrapping_neg()),
            value => Number::F64(-(value as f64)),
        })
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
                    ErrorKind::Syntax,
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
            return Err(Error::new(ErrorKind::Syntax, "infinity takes no sign"));
        }

        while let b'0'..=b'9' = self.peek_or_nul() {
            self.bump();
        }
        Ok(if nonnegative { 0.0 } else { -0.0 })
    }
}

/// Eight more digits can be added to significands below this.
const EIGHT_DIGITS_LIMIT: u64 = (u64::MAX - 99_999_999) / 100_000_000;

/// Returns the value of eight digits (the values of the characters, the
/// first one in the lowest byte).
#[inline(always)]
fn combine_digits(digits: u64) -> u64 {
    // combine pairs of digits, then pairs of those and so on
    let pairs = digits.wrapping_mul(10).wrapping_add(digits >> 8);
    let low = (pairs & 0x0000_00ff_0000_00ff).wrapping_mul(0x000f_4240_0000_0064);
    let high = ((pairs >> 16) & 0x0000_00ff_0000_00ff).wrapping_mul(0x0000_2710_0000_0001);
    u64::from((low.wrapping_add(high) >> 32) as u32)
}

/// The bytes of the escapes that stand for a single byte (`\\n` and so
/// on), zero for the others.
static UNESCAPE: [u8; 256] = {
    let mut table = [0; 256];
    table[b'"' as usize] = b'"';
    table[b'\\' as usize] = b'\\';
    table[b'/' as usize] = b'/';
    table[b'b' as usize] = b'\x08';
    table[b'f' as usize] = b'\x0c';
    table[b'n' as usize] = b'\n';
    table[b'r' as usize] = b'\r';
    table[b't' as usize] = b'\t';
    table
};

/// The powers of ten that fit into 64 bits.
static POW10_U64: [u64; 20] = {
    let mut table = [1; 20];
    let mut idx = 1;
    while idx < table.len() {
        table[idx] = table[idx - 1] * 10;
        idx += 1;
    }
    table
};

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
    Error::new(ErrorKind::Syntax, msg).with_offset(offset)
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
        && (frac_len <= 4
            || digits == 0
            || (frac_len <= 19 && digits >= POW10_U64[frac_len as usize - 4]))
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
        #[cfg(json5)]
        Number::U128(val) => out.emit(Atom::Ext(ExtValue::borrowed(&val))),
        #[cfg(json5)]
        Number::I128(val) => out.emit(Atom::Ext(ExtValue::borrowed(&val))),
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
    #[cfg(json5)]
    let normalized = json_number(text);
    #[cfg(json5)]
    let text = normalized.as_deref().unwrap_or(text);
    let number = ExactNumber::new(text, value);
    out.emit(Atom::Ext(ExtValue::borrowed_value::<ExactNumber>(&number)))
}

/// Returns the text of a number in the syntax of JSON if it differs.
///
/// Numbers can have a leading `+` and leading or trailing decimal points
/// (hexadecimal numbers are integers and never passed on as text).
#[cfg(json5)]
fn json_number(text: &str) -> Option<String> {
    let (sign, rest) = match text.as_bytes()[0] {
        b'+' => ("", &text[1..]),
        b'-' => ("-", &text[1..]),
        _ => ("", text),
    };
    let (int, frac) = rest.split_once('.').unwrap_or((rest, ""));
    let exp_start = frac.find(['e', 'E']).unwrap_or(frac.len());
    let (frac, exp) = frac.split_at(exp_start);
    if !text.starts_with('+') && !int.is_empty() && (!frac.is_empty() || !rest.contains('.')) {
        return None;
    }
    let int = if int.is_empty() { "0" } else { int };
    let dot = if frac.is_empty() { "" } else { "." };
    Some(format!("{sign}{int}{dot}{frac}{exp}"))
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
#[cfg(hjson)]
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
#[cfg(hjson)]
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
#[cfg(hjson)]
fn line_end(input: &[u8], pos: usize) -> Option<usize> {
    input[pos..]
        .iter()
        .position(|&byte| byte == b'\n' || byte == b'\r')
        .map(|index| pos + index)
}

/// Returns `true` if the byte can be part of a map key without quotes.
#[cfg(hjson)]
fn is_key_byte(byte: u8) -> bool {
    byte > b' ' && !matches!(byte, b'{' | b'}' | b'[' | b']' | b',' | b':')
}

/// Returns the number of characters in valid UTF-8.
#[cfg(hjson)]
fn count_chars(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&byte| byte & 0xc0 != 0x80).count()
}

/// Returns the column after the input starting at `column`.
#[cfg(hjson)]
fn advance_column(column: usize, input: &[u8]) -> usize {
    match input.iter().rposition(|&byte| byte == b'\n') {
        Some(index) => count_chars(&input[index + 1..]),
        None => column + count_chars(input),
    }
}

/// Returns the length of the rest of a line comment (after the slashes)
/// including the line break that ends it.
#[cfg(comments)]
fn line_comment_len(bytes: &[u8]) -> Option<usize> {
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' | b'\r' => return Some(index + 1),
            // the line and paragraph separators
            #[cfg(json5)]
            0xe2 if matches!(bytes.get(index + 1..index + 3), Some([0x80, 0xa8 | 0xa9])) => {
                return Some(index + 3);
            }
            _ => index += 1,
        }
    }
    None
}

/// Returns the length of the whitespace character at the start of the
/// bytes if it's not ASCII.
///
/// These are the characters of the Unicode category Zs, the line and
/// paragraph separators and the byte order mark.
#[cfg(json5)]
fn unicode_whitespace(bytes: &[u8]) -> Option<usize> {
    match bytes {
        [0xc2, 0xa0, ..] => Some(2),
        [0xe1, 0x9a, 0x80, ..]
        | [0xe2, 0x80, 0x80..=0x8a | 0xa8 | 0xa9 | 0xaf, ..]
        | [0xe2, 0x81, 0x9f, ..]
        | [0xe3, 0x80, 0x80, ..]
        | [0xef, 0xbb, 0xbf, ..] => Some(3),
        _ => None,
    }
}

/// Returns `true` if the character can be part of an identifier.
///
/// These are the characters of ECMAScript identifiers: `$`, `_` and the
/// characters with the Unicode property `ID_Start` and (except for the
/// first character) `ID_Continue` and the zero width (non-)joiner.
#[cfg(json5)]
fn is_identifier_char(c: char, first: bool) -> bool {
    matches!(c, '$' | '_')
        || unicode_ident::is_xid_start(c)
        || (!first && (unicode_ident::is_xid_continue(c) || matches!(c, '\u{200c}' | '\u{200d}')))
}

#[cold]
#[cfg(json5)]
fn invalid_identifier() -> Error {
    Error::new(ErrorKind::Syntax, "invalid escape in identifier")
}

#[cold]
fn invalid_escape() -> Error {
    Error::new(ErrorKind::Syntax, "invalid escape in string")
}

#[cold]
fn lone_surrogate() -> Error {
    Error::new(
        ErrorKind::Syntax,
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

    /// Returns the chunk sizes to feed an input of `len` bytes in.
    ///
    /// Miri is too slow for all sizes, it checks chunks of a byte (which
    /// split the input at every position), odd and aligned sizes and the
    /// whole input.
    fn chunk_sizes(len: usize) -> impl Iterator<Item = usize> {
        (1..=len).filter(move |&size| !cfg!(miri) || matches!(size, 1 | 3 | 8) || size == len)
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
            for size in chunk_sizes(input.len()) {
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
            #[cfg(not(trailing_commas))]
            "[1,]",
            "{\"a\": \"\\x\"}",
        ] {
            let expected = parse_complete(input).unwrap_err();
            for size in chunk_sizes(input.len()) {
                assert_eq!(
                    parse_chunked(input, size).unwrap_err(),
                    expected,
                    "{input} size {size}"
                );
            }
        }
    }

    #[test]
    fn test_digit_runs() {
        /// Parses the numbers of a sequence.
        fn parse(text: &str) -> Vec<f64> {
            let mut out = None::<Vec<f64>>;
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
                .unwrap();
            drop(driver);
            out.unwrap()
        }

        // runs of digits of every length in the integer and the fraction,
        // with and without eight bytes after them
        let digits = "12345678901234567890123";
        // miri is too slow for all combinations
        let step = if cfg!(miri) { 4 } else { 1 };
        for int_len in (1..=digits.len()).step_by(step) {
            for frac_len in (0..=digits.len()).step_by(step) {
                let mut number = digits[..int_len].to_string();
                if frac_len > 0 {
                    number.push('.');
                    number.push_str(&digits[digits.len() - frac_len..]);
                }
                let value: f64 = number.parse().unwrap();
                for text in [
                    format!("[{number}]"),
                    format!("[{number},-{number}e1 , {number}]"),
                    format!("[{number}                ]"),
                ] {
                    let values = parse(&text);
                    assert_eq!(values[0].to_bits(), value.to_bits(), "{text}");
                    assert_eq!(values.last().unwrap().to_bits(), value.to_bits(), "{text}");
                }
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
            #[cfg(hjson)]
            _ => assert_eq!(
                parse(text),
                Err("unexpected string, expected f64".into()),
                "{text}"
            ),
            #[cfg(not(hjson))]
            _ => assert_eq!(parse(text), Err("number out of range".into()), "{text}"),
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
        for _ in 0..if cfg!(miri) { 100 } else { 100_000 } {
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
        for _ in 0..if cfg!(miri) { 100 } else { 100_000 } {
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
        for _ in 0..if cfg!(miri) { 10 } else { 10_000 } {
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
    #[cfg(any(comments, json5))]
    fn assert_like_json(inputs: &[(&str, &str)]) {
        for (input, json) in inputs {
            let expected = parse_complete(json).unwrap();
            assert_eq!(parse_complete(input), Ok(expected.clone()), "{input}");
            for size in chunk_sizes(input.len()) {
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
    #[cfg(any(comments, json5))]
    fn assert_errors(inputs: &[(&str, &str)]) {
        for (input, msg) in inputs {
            assert_eq!(parse_complete(input).unwrap_err(), *msg, "{input}");
            for size in chunk_sizes(input.len()) {
                assert_eq!(
                    parse_chunked(input, size).unwrap_err(),
                    *msg,
                    "{input} size {size}"
                );
            }
        }
    }

    #[test]
    #[cfg(comments)]
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
        #[cfg(not(hjson))]
        assert_errors(&[
            ("[1 /* c", "unexpected end of file"),
            ("[1 / 2]", "expected a comma"),
            ("[/", "unexpected character"),
            ("[1 /", "expected a comma"),
            ("/x", "unexpected character"),
        ]);
    }

    #[test]
    #[cfg(trailing_commas)]
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
    #[cfg(json5)]
    fn test_json5() {
        assert_like_json(&[
            // keys
            ("{a: 1, $b_2: 2, _: 3}", r#"{"a": 1, "$b_2": 2, "_": 3}"#),
            (
                "{Infinity: 1, null: 2, true: 3}",
                r#"{"Infinity": 1, "null": 2, "true": 3}"#,
            ),
            ("{ä: 1, a\u{200d}b: 2}", "{\"ä\": 1, \"a\u{200d}b\": 2}"),
            // combining marks and other digits continue identifiers
            (
                "{u\u{308}ber: 1, x\u{665}: 2}",
                "{\"u\u{308}ber\": 1, \"x\u{665}\": 2}",
            ),
            (
                r"{sig\u03A3ma: 1, \u0061b: 2, a\u0062: 3}",
                r#"{"sigΣma": 1, "ab": 2, "ab": 3}"#,
            ),
            ("{'a': 1}", r#"{"a": 1}"#),
            // strings
            ("'a\"b'", r#""a\"b""#),
            ("\"a'b\"", r#""a'b""#),
            (r"'a\'b'", r#""a'b""#),
            (r"'\v\0\x41\a\ä'", r#""\u000b\u0000Aaä""#),
            ("'a\\\nb\\\r\nc\\\rd\\\u{2028}e'", r#""abcde""#),
            ("'a\tb'", r#""a\tb""#),
            // numbers
            (
                "[0x1F, 0XfF, -0x10, +1, +1.5, .5, -.5, 5., 5.e1]",
                "[31, 255, -16, 1, 1.5, 0.5, -0.5, 5.0, 5e1]",
            ),
            (
                "[0xFFFFFFFFFFFFFFFF, -0x8000000000000000]",
                "[18446744073709551615, -9223372036854775808]",
            ),
            (
                "[0x10000000000000000, -0x8000000000000001]",
                "[18446744073709551616, -9223372036854775809]",
            ),
            (
                "[0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF, -0x80000000000000000000000000000000]",
                "[340282366920938463463374607431768211455, \
                 -170141183460469231731687303715884105728]",
            ),
            // whitespace and line breaks
            ("\u{feff}[1,\u{a0}2\u{2028}\u{3000}\x0b\x0c]", "[1, 2]"),
            ("[1, // a\u{2028}2, // b\u{2029}3]", "[1, 2, 3]"),
        ]);
        assert_errors(&[
            ("{1: 2}", "expected map key"),
            ("{\u{308}u: 1}", "expected map key"),
            ("{a b: 1}", "expected colon"),
            (r"{\u0031: 1}", "invalid escape in identifier"),
            (r"{a\u0020: 1}", "invalid escape in identifier"),
            (r"{a\x41: 1}", "invalid escape in identifier"),
            (r"{a\uD800: 1}", "invalid escape in identifier"),
            (r"{a\u00: 1}", "invalid hex escape"),
            ("[a]", "unexpected character"),
            ("'a", "unexpected end of string"),
            ("'a\nb'", "unexpected character in string"),
            (r"'\01'", "invalid escape in string"),
            (r"'\1'", "invalid escape in string"),
            (r"'\xZ0'", "invalid hex escape"),
            ("[.]", "expected a digit"),
            ("[0x]", "expected a hex digit"),
            ("[Inf]", "unexpected character"),
            ("\u{2029}x", "unexpected character"),
        ]);
    }

    #[test]
    #[cfg(hjson)]
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

    /// Returns `2^exp` exactly (`powi` may be imprecise, and is in miri).
    #[cfg(json5)]
    fn pow2(exp: u64) -> f64 {
        f64::from_bits((1023 + exp) << 52)
    }

    #[test]
    #[cfg(json5)]
    fn test_json5_hex_floats_are_rounded_correctly() {
        let value = |text: &str| match parse_complete(text).unwrap()[..] {
            [Event::Atom(Atom::F64(value))] => value,
            ref events => panic!("{events:?}"),
        };
        // (2^53 + 1) * 2^80 is half way between 2^133 and the next float
        // (rounded to the even one).  The digits beyond 128 bits are
        // dropped, if one of them is not zero the value is above half way.
        let half = format!("0x20000000000001{}", "0".repeat(20));
        let above = format!("0x20000000000001{}1", "0".repeat(19));
        let next = pow2(133) + pow2(81);
        assert_eq!(value(&half), pow2(133));
        assert_eq!(value(&above), next);
        assert_eq!(value(&format!("-{above}")), -next);
    }

    #[test]
    #[cfg(json5)]
    fn test_json5_large_hex() {
        // hexadecimal integers beyond 128 bits are approximated as floats
        let events = parse_complete(&format!("[0x1{}, -0x1{0}]", "0".repeat(32))).unwrap();
        assert_eq!(
            events[1..3],
            [Event::from(pow2(128)), Event::from(-pow2(128))]
        );
        assert_errors(&[(&format!("0x1{}", "0".repeat(256)), "number out of range")]);
    }

    #[test]
    #[cfg(all(comments, not(hjson)))]
    fn test_line_scan() {
        fn lines(input: &str) -> Vec<&str> {
            let mut rv = Vec::new();
            let mut start = 0;
            let mut scan = crate::scan::LineScan::default();
            while let Some(end) = scan.find_end(input.as_bytes(), start) {
                rv.push(&input[start..end]);
                start = end + 1;
            }
            rv.push(&input[start..]);
            rv
        }

        assert_eq!(lines("1\n2"), ["1", "2"]);
        assert_eq!(lines("1 /* a\nb */\n2"), ["1 /* a\nb */", "2"]);
        assert_eq!(lines("1 // a\n2"), ["1 // a", "2"]);
        assert_eq!(lines("\"/*\"\n2 */"), ["\"/*\"", "2 */"]);
        assert_eq!(lines("\"a\n\"b"), ["\"a", "\"b"]);
        assert_eq!(lines("1 / 2\n3"), ["1 / 2", "3"]);
        assert_eq!(lines("1 /\"a\n\"\n2"), ["1 /\"a", "\"", "2"]);
        #[cfg(json5)]
        {
            assert_eq!(lines("'/*'\n2 */"), ["'/*'", "2 */"]);
            assert_eq!(
                lines("'a\\\nb'\n'c\\\r\nd'\n2"),
                ["'a\\\nb'", "'c\\\r\nd'", "2"]
            );
        }
    }

    #[test]
    #[cfg(json5)]
    fn test_json5_number_text() {
        for (text, json) in [
            ("+1.5", Some("1.5")),
            ("-.5e3", Some("-0.5e3")),
            (".5", Some("0.5")),
            ("5.", Some("5")),
            ("5.e1", Some("5e1")),
            ("+5.E1", Some("5E1")),
            ("1.5e3", None),
            ("-1", None),
        ] {
            assert_eq!(json_number(text).as_deref(), json, "{text}");
        }
    }

    #[test]
    #[cfg(json5)]
    fn test_json5_non_finite() {
        let events = parse_complete("[Infinity, -Infinity, +Infinity, NaN, -NaN]").unwrap();
        let floats: Vec<f64> = events
            .iter()
            .filter_map(|event| match event {
                Event::Atom(Atom::F64(value)) => Some(*value),
                _ => None,
            })
            .collect();
        assert_eq!(floats.len(), 5);
        assert_eq!(
            floats[..3],
            [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY]
        );
        assert!(floats[3].is_nan() && floats[4].is_nan());
    }
}
