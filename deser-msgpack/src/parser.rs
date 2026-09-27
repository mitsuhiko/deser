//! The MessagePack parser.
//!
//! The parser is a state machine which can be suspended between items: if
//! the input ends within an item and more input can follow, the parser
//! returns how much of the input it consumed (up to the start of the item)
//! and continues with more input.  With the complete input (`eof`) it parses
//! an item in one go.
use std::borrow::Cow;
use std::str;

use deser_core::de::DeserializeDriver;
use deser_core::ext::ExtValue;
use deser_core::{Atom, Bytes, ContainerShape, Error, ErrorKind, Event, State};

use crate::ext::{Ext, TIMESTAMP, decode_timestamp};
use crate::head::{Head, HeadError, decode_head};

/// An open array or map.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    is_map: bool,
    /// The number of items (array) or entries (map) that are still expected.
    remaining: u32,
    /// For maps: `true` if a value is expected next.
    in_value: bool,
}

/// Receives the events of the parser.
///
/// Events which borrow from the input are passed to
/// [`emit_input`](Self::emit_input), which can pass them on borrowed if the
/// input lives long enough.
pub(crate) trait Out<'i> {
    fn state_mut(&mut self) -> &mut State;
    fn emit<'e, E: Into<Event<'e>>>(&mut self, event: E) -> Result<(), Error>;
    fn emit_input(&mut self, event: Event<'i>) -> Result<(), Error>;
}

/// Passes events of the input on borrowed.
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
    fn emit_input(&mut self, event: Event<'i>) -> Result<(), Error> {
        self.0.emit_borrowed(event)
    }
}

#[cfg(any(test, feature = "io"))]
/// Passes events of the input on as data that is only valid for the call.
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
    fn emit_input(&mut self, event: Event<'i>) -> Result<(), Error> {
        self.0.emit(event)
    }
}

#[cfg(feature = "io")]
/// Discards the events.
///
/// This is used to skip the rest of an item after an error.
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
    fn emit_input(&mut self, _event: Event<'i>) -> Result<(), Error> {
        Ok(())
    }
}

/// The result of [`Parser::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Progress {
    /// The item is complete, it ends at the given position.
    Done(usize),
    /// More input is needed.  The input up to the given position was
    /// consumed, the parser continues with the input after it.
    NeedMore(usize),
}

/// A MessagePack parser which can be suspended between items.
#[derive(Default)]
pub(crate) struct Parser {
    // the outer containers, the current one is held in `frame`
    stack: Vec<Frame>,
    frame: Option<Frame>,
    // the top-level item is complete (after an error of a sink)
    complete: bool,
    // the last error was an error of a sink, the rest of the item
    // continues at the position
    recoverable: Option<usize>,
    // where the last call stopped
    position: usize,
}

impl Parser {
    /// Resets the parser to parse a new item.
    ///
    /// This is needed after an error.
    pub(crate) fn reset(&mut self) {
        self.stack.clear();
        self.frame = None;
        self.complete = false;
        self.recoverable = None;
    }

    /// Returns where the last call stopped (also after an error).
    pub(crate) fn position(&self) -> usize {
        self.position
    }

    #[cfg(feature = "io")]
    /// Returns where the rest of the item continues if the last error was
    /// an error of a sink.
    ///
    /// In that case the state is as if the event was accepted and the rest
    /// of the item can be parsed from the position (for instance with
    /// [`Discard`]).
    pub(crate) fn recoverable(&self) -> Option<usize> {
        self.recoverable
    }

    /// Parses an item (or continues it) from `input[pos..]`.
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
        out: &mut O,
    ) -> Result<Progress, Error> {
        self.recoverable = None;
        let mut cur = Cursor {
            input,
            pos,
            base,
            hit_end: false,
            sink_failed: false,
            opened: None,
        };
        let rv = self.run(&mut cur, eof, out);
        self.position = cur.pos;
        rv
    }

    #[inline(always)]
    fn run<'i, O: Out<'i>>(
        &mut self,
        cur: &mut Cursor<'i>,
        eof: bool,
        out: &mut O,
    ) -> Result<Progress, Error> {
        if self.complete {
            self.complete = false;
            return Ok(Progress::Done(cur.pos));
        }
        // the stack is held in a local while parsing
        let mut stack = std::mem::take(&mut self.stack);
        let mut frame = self.frame;

        // returns from the function with the stack put back
        macro_rules! ret {
            ($rv:expr) => {{
                self.stack = stack;
                return $rv;
            }};
        }

        // fails with an error of a sink.  The state is stored as if the
        // event was accepted so that the rest of the item can be skipped.
        macro_rules! sink_failed {
            ($err:expr) => {{
                self.frame = frame;
                self.complete = frame.is_none();
                self.recoverable = Some(cur.pos);
                ret!(Err($err));
            }};
        }

        loop {
            // close all completed containers
            while let Some(current) = frame {
                if current.remaining != 0 || current.in_value {
                    break;
                }
                frame = stack.pop();
                // the end has no bytes of its own
                out.state_mut()
                    .set_input_range(cur.base + cur.pos, cur.base + cur.pos);
                if let Err(err) = out.emit(if current.is_map {
                    Event::MapEnd
                } else {
                    Event::SeqEnd
                }) {
                    sink_failed!(err);
                }
                if frame.is_none() {
                    self.frame = None;
                    ret!(Ok(Progress::Done(cur.pos)));
                }
            }

            // account for the next item (this is undone if the item is
            // incomplete)
            if let Some(ref mut current) = frame {
                if !current.in_value {
                    current.remaining -= 1;
                }
                if current.is_map {
                    current.in_value = !current.in_value;
                }
            }

            // items only run into the end of the input if they fail and
            // errors end the call, so the flags do not need to be reset
            // for every item.
            let start = cur.pos;
            let rv = cur.parse_item(out);
            if cur.hit_end && !eof {
                if let Some(ref mut current) = frame {
                    if current.is_map {
                        current.in_value = !current.in_value;
                    }
                    if !current.in_value {
                        current.remaining += 1;
                    }
                }
                self.frame = frame;
                ret!(Ok(Progress::NeedMore(start)));
            }
            match rv {
                Ok(Some(new)) => {
                    if let Some(outer) = frame.replace(new) {
                        stack.push(outer);
                    }
                }
                Ok(None) if frame.is_none() => {
                    self.frame = None;
                    ret!(Ok(Progress::Done(cur.pos)));
                }
                Ok(None) => {}
                Err(err) if cur.sink_failed => {
                    if let Some(new) = cur.opened.take()
                        && let Some(outer) = frame.replace(new)
                    {
                        stack.push(outer);
                    }
                    sink_failed!(err);
                }
                Err(err) => ret!(Err(err)),
            }
        }
    }
}

/// Reads items from the input.
pub(crate) struct Cursor<'a> {
    input: &'a [u8],
    pos: usize,
    // the offset of the input in the stream
    base: usize,
    // `true` if an item ran into the end of the input
    hit_end: bool,
    // `true` if a sink failed
    sink_failed: bool,
    // the container that was opened by the item (if its start failed)
    opened: Option<Frame>,
}

impl<'a> Cursor<'a> {
    /// Parses an item and emits its (first) event.
    ///
    /// If the item is an array or map, the frame for it is returned.
    #[inline]
    fn parse_item<O: Out<'a>>(&mut self, out: &mut O) -> Result<Option<Frame>, Error> {
        let start = self.pos;
        let (head, len) = match decode_head(&self.input[start..]) {
            Ok(rv) => rv,
            Err(HeadError::Incomplete) => {
                self.hit_end = true;
                return Err(eof_error(self.base + self.input.len()));
            }
            Err(HeadError::Reserved) => {
                // the next item is read after the byte
                self.pos += 1;
                return Err(syntax_error(self.base + start, "reserved byte 0xc1"));
            }
        };
        self.pos += len;

        match head {
            Head::Nil => self.emit(out, start, Atom::Null)?,
            Head::Bool(value) => self.emit(out, start, Atom::Bool(value))?,
            Head::Uint(value) => self.emit(out, start, Atom::U64(value))?,
            Head::Int(value) => self.emit(out, start, Atom::I64(value))?,
            Head::F32(value) => self.emit(out, start, Atom::F32(value))?,
            Head::F64(value) => self.emit(out, start, Atom::F64(value))?,
            // strings and binary data are slices of the input
            Head::Str(len) => {
                let bytes = self.read_body(len)?;
                if !is_ascii(bytes) && !is_utf8(bytes) {
                    return Err(syntax_error(
                        self.base + self.pos - bytes.len(),
                        "invalid UTF-8 in string",
                    ));
                }
                // SAFETY: the string was validated as UTF-8
                let text = unsafe { str::from_utf8_unchecked(bytes) };
                self.emit_borrowed(out, start, Event::Atom(Atom::Str(Cow::Borrowed(text))))?
            }
            Head::Bin(len) => {
                let bytes = self.read_body(len)?;
                self.emit_borrowed(out, start, Event::Atom(Atom::Bytes(Bytes::borrowed(bytes))))?
            }
            Head::Ext(kind, len) => {
                let data = self.read_body(len)?;
                // invalid timestamps are passed on as extensions
                let timestamp = match kind {
                    TIMESTAMP => decode_timestamp(data),
                    _ => None,
                };
                let atom = match timestamp {
                    Some(value) => Atom::Ext(ExtValue::owned(value)),
                    None => Atom::Ext(ExtValue::owned(Ext::new(kind, data))),
                };
                self.emit(out, start, atom)?
            }
            Head::Array(len) | Head::Map(len) => {
                let is_map = matches!(head, Head::Map(_));
                // the declared length is passed on, it's not trusted by sinks
                let shape = ContainerShape::new().with_len(len as usize);
                let frame = Frame {
                    is_map,
                    remaining: len,
                    in_value: false,
                };
                let event = if is_map {
                    Event::MapStart(shape)
                } else {
                    Event::SeqStart(shape)
                };
                if let Err(err) = self.emit(out, start, event) {
                    // the frame is opened even if the sink fails so that
                    // the rest of the container can be skipped
                    self.opened = Some(frame);
                    return Err(err);
                }
                return Ok(Some(frame));
            }
        }
        Ok(None)
    }

    /// Emits an event of the item at `start`.
    #[inline(always)]
    fn emit<'e, E: Into<Event<'e>>, O: Out<'a>>(
        &mut self,
        out: &mut O,
        start: usize,
        event: E,
    ) -> Result<(), Error> {
        out.state_mut()
            .set_input_range(self.base + start, self.base + self.pos);
        let rv = out.emit(event);
        if rv.is_err() {
            self.sink_failed = true;
        }
        rv
    }

    /// Emits a borrowed event of the item at `start`.
    #[inline(always)]
    fn emit_borrowed<O: Out<'a>>(
        &mut self,
        out: &mut O,
        start: usize,
        event: Event<'a>,
    ) -> Result<(), Error> {
        out.state_mut()
            .set_input_range(self.base + start, self.base + self.pos);
        let rv = out.emit_input(event);
        if rv.is_err() {
            self.sink_failed = true;
        }
        rv
    }

    /// Reads the body of a string, binary data or an extension.
    #[inline]
    fn read_body(&mut self, len: u32) -> Result<&'a [u8], Error> {
        let len = len as usize;
        let input = self.input;
        if len > input.len() - self.pos {
            self.hit_end = true;
            return Err(eof_error(self.base + input.len()));
        }
        let bytes = &input[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes)
    }
}

/// Checks if the bytes are ASCII.
///
/// Most strings are short, these are checked with (possibly overlapping)
/// word sized loads.
#[inline(always)]
fn is_ascii(bytes: &[u8]) -> bool {
    fn load_u64(bytes: &[u8], pos: usize) -> u64 {
        u64::from_ne_bytes(bytes[pos..pos + 8].try_into().unwrap())
    }
    fn load_u32(bytes: &[u8], pos: usize) -> u32 {
        u32::from_ne_bytes(bytes[pos..pos + 4].try_into().unwrap())
    }
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
fn is_utf8(bytes: &[u8]) -> bool {
    #[cfg(feature = "speedups")]
    {
        simdutf8::basic::from_utf8(bytes).is_ok()
    }
    #[cfg(not(feature = "speedups"))]
    {
        str::from_utf8(bytes).is_ok()
    }
}

#[cold]
pub(crate) fn syntax_error(offset: usize, msg: &str) -> Error {
    Error::new(ErrorKind::Unexpected, format!("syntax error: {}", msg)).with_offset(offset)
}

#[cold]
fn eof_error(offset: usize) -> Error {
    Error::new(ErrorKind::EndOfFile, "unexpected end of input").with_offset(offset)
}

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

#[cfg(test)]
mod tests {
    use deser_core::de::Recording;

    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s = s.replace(' ', "");
        (0..s.len())
            .step_by(2)
            .map(|idx| u8::from_str_radix(&s[idx..idx + 2], 16).unwrap())
            .collect()
    }

    fn events(value: Recording) -> Vec<Event<'static>> {
        value.events().cloned().collect()
    }

    /// Parses the input in one go.
    fn parse_complete(input: &[u8]) -> Result<Vec<Event<'static>>, String> {
        let mut out = None::<Recording>;
        let mut parser = Parser::default();
        {
            let mut driver = DeserializeDriver::new(&mut out);
            parser
                .parse(input, 0, true, 0, &mut Borrowing(&mut driver))
                .map_err(|err| err.message().to_string())?;
        }
        Ok(events(out.unwrap()))
    }

    /// Feeds the input to the parser in chunks like a stream would.
    fn parse_chunked(input: &[u8], size: usize) -> Result<Vec<Event<'static>>, String> {
        let mut out = None::<Recording>;
        let mut parser = Parser::default();
        {
            let mut driver = DeserializeDriver::new(&mut out);
            let mut buffer = Vec::new();
            let mut base = 0;
            let mut rest = input;
            loop {
                let len = size.min(rest.len());
                buffer.extend_from_slice(&rest[..len]);
                rest = &rest[len..];
                let eof = rest.is_empty();
                match parser
                    .parse(&buffer, 0, eof, base, &mut Copying(&mut driver))
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
        let inputs = [
            // [1, [2, 3], "ab", {"a": 1}, bin 0102, nil]
            "96 01 920203 a26162 81a16101 c4020102 c0",
            // timestamps and an extension
            "93 d6ff5a4af6a5 d7ffa1dcd7c85a4af6a5 c70307707172",
            // floats and keys that are not strings
            "82 ca3f800000 cb3ff199999999999a c3 d0ff",
            // binary data with a two byte length
            &format!("c5 0100 {}", "aa".repeat(256)),
            // str 8 and an empty array 16
            "92 d903616263 dc0000",
        ];
        for input in inputs {
            let input = hex(input);
            let expected = parse_complete(&input).unwrap();
            for size in 1..=input.len() {
                assert_eq!(
                    parse_chunked(&input, size).unwrap(),
                    expected,
                    "size {size}"
                );
            }
        }
    }

    #[test]
    fn test_errors_in_chunks() {
        for input in [
            "92 01",
            "81 a1 61",
            "c1",
            "92 01 c1",
            "a2 fffe",
            "c7 03 07 70",
        ] {
            let input = hex(input);
            let expected = parse_complete(&input).unwrap_err();
            for size in 1..=input.len() {
                assert_eq!(
                    parse_chunked(&input, size).unwrap_err(),
                    expected,
                    "size {size}"
                );
            }
        }
    }
}
