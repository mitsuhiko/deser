//! The CBOR parser.
//!
//! The parser is a state machine which can be suspended between data items:
//! if the input ends within an item (including its tags) and more input can
//! follow, the parser returns how much of the input it consumed (up to the
//! start of the item) and continues with more input.  With the complete
//! input (`eof`) it parses a data item in one go.
use std::borrow::Cow;
use std::str;

use deser::de::DeserializeDriver;
use deser::ext::{BigInt, Datetime, Decimal, ExtValue, Uuid};
use deser::{Atom, Bytes, ContainerShape, Error, ErrorKind, Event, State};

use crate::float::f16_to_f64;
use crate::simple::Simple;
use crate::tag::Tags;

pub(crate) const MAJOR_UNSIGNED: u8 = 0;
pub(crate) const MAJOR_NEGATIVE: u8 = 1;
pub(crate) const MAJOR_BYTES: u8 = 2;
pub(crate) const MAJOR_TEXT: u8 = 3;
pub(crate) const MAJOR_ARRAY: u8 = 4;
pub(crate) const MAJOR_MAP: u8 = 5;
pub(crate) const MAJOR_TAG: u8 = 6;

/// The additional information for indefinite lengths.
pub(crate) const INDEFINITE: u8 = 31;
pub(crate) const BREAK: u8 = 0xff;

/// An open array or map.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    is_map: bool,
    /// The number of items (array) or entries (map) that are still expected.
    /// `None` for indefinite length containers.
    remaining: Option<u64>,
    /// For maps: `true` if a value is expected next.
    in_value: bool,
}

/// The head of a data item.
#[derive(Clone, Copy)]
pub(crate) struct Head {
    major: u8,
    info: u8,
    /// The argument.  For indefinite lengths this is unused.
    arg: u64,
}

impl Head {
    fn is_indefinite(self) -> bool {
        self.info == INDEFINITE
    }
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

/// Passes events of the input on as data that is only valid for the call.
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
    fn emit_input(&mut self, event: Event<'i>) -> Result<(), Error> {
        self.0.emit(event)
    }
}

/// Discards the events.
///
/// This is used to skip the rest of a data item after an error.
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
    fn emit_input(&mut self, _event: Event<'i>) -> Result<(), Error> {
        Ok(())
    }
}

/// The result of [`Parser::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Progress {
    /// The data item is complete, it ends at the given position.
    Done(usize),
    /// More input is needed.  The input up to the given position was
    /// consumed, the parser continues with the input after it.
    NeedMore(usize),
}

/// A CBOR parser which can be suspended between data items.
#[derive(Default)]
pub(crate) struct Parser {
    // the outer containers, the current one is held in `frame`
    stack: Vec<Frame>,
    frame: Option<Frame>,
    tags: Vec<u64>,
    scratch: Vec<u8>,
    // the top-level item is complete (after an error of a sink)
    complete: bool,
    // the last error was an error of a sink, the rest of the item
    // continues at the position
    recoverable: Option<usize>,
    // where the last call stopped
    position: usize,
}

impl Parser {
    /// Resets the parser to parse a new data item.
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

    /// Returns where the rest of the item continues if the last error was
    /// an error of a sink.
    ///
    /// In that case the state is as if the event was accepted and the rest
    /// of the item can be parsed from the position (for instance with
    /// [`Discard`]).
    pub(crate) fn recoverable(&self) -> Option<usize> {
        self.recoverable
    }

    /// Parses a data item (or continues it) from `input[pos..]`.
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
            tags: std::mem::take(&mut self.tags),
        };
        let rv = self.run(&mut cur, eof, out);
        self.position = cur.pos;
        // the allocation of the tags is reused
        self.tags = std::mem::take(&mut cur.tags);
        self.tags.clear();
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
        let scratch = &mut self.scratch;
        let mut frame = self.frame;

        // fails with an error of a sink.  The state is stored as if the
        // event was accepted so that the rest of the item can be skipped.
        macro_rules! sink_failed {
            ($err:expr) => {{
                self.frame = frame;
                self.complete = frame.is_none();
                self.recoverable = Some(cur.pos);
                self.stack = stack;
                return Err($err);
            }};
        }

        loop {
            // close all completed containers
            while let Some(current) = frame {
                let done = match current.remaining {
                    Some(0) => !current.in_value,
                    Some(_) => false,
                    None => cur.peek() == Some(BREAK),
                };
                if !done {
                    break;
                }
                let start = cur.pos;
                if current.remaining.is_none() {
                    if current.in_value {
                        {
                            self.stack = stack;
                            return Err(syntax_error(cur.base + cur.pos, "missing map value"));
                        }
                    }
                    cur.pos += 1;
                }
                frame = stack.pop();
                out.state_mut()
                    .set_input_range(cur.base + start, cur.base + cur.pos);
                if let Err(err) = out.emit(if current.is_map {
                    Event::MapEnd
                } else {
                    Event::SeqEnd
                }) {
                    sink_failed!(err);
                }
                if frame.is_none() {
                    self.frame = None;
                    {
                        self.stack = stack;
                        return Ok(Progress::Done(cur.pos));
                    }
                }
            }

            // account for the next item (this is undone if the item is
            // incomplete)
            if let Some(ref mut current) = frame {
                if current.is_map {
                    if !current.in_value
                        && let Some(ref mut remaining) = current.remaining
                    {
                        *remaining -= 1;
                    }
                    current.in_value = !current.in_value;
                } else if let Some(ref mut remaining) = current.remaining {
                    *remaining -= 1;
                }
            }

            // items only run into the end of the input if they fail and
            // errors end the call, so the flags do not need to be reset
            // for every item.
            let start = cur.pos;
            let rv = cur.parse_item(out, scratch);
            if cur.hit_end && !eof {
                if let Some(ref mut current) = frame {
                    // arrays and the keys of maps took an item off the
                    // count, after a key a map expects a value
                    let counted = !current.is_map || current.in_value;
                    if current.is_map {
                        current.in_value = !current.in_value;
                    }
                    if counted && let Some(ref mut remaining) = current.remaining {
                        *remaining += 1;
                    }
                }
                self.frame = frame;
                {
                    self.stack = stack;
                    return Ok(Progress::NeedMore(start));
                }
            }
            match rv {
                Ok(Some(new)) => {
                    if let Some(outer) = frame.replace(new) {
                        stack.push(outer);
                    }
                }
                Ok(None) if frame.is_none() => {
                    self.frame = None;
                    {
                        self.stack = stack;
                        return Ok(Progress::Done(cur.pos));
                    }
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
                Err(err) => {
                    self.stack = stack;
                    return Err(err);
                }
            }
        }
    }
}

/// Reads data items from the input.
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
    // the tags in front of the current item
    tags: Vec<u64>,
}

impl<'a> Cursor<'a> {
    /// Parses a data item including its tags and emits its (first) event.
    ///
    /// If the item is an array or map, the frame for it is returned.
    #[inline]
    fn parse_item<O: Out<'a>>(
        &mut self,
        out: &mut O,
        buffer: &mut Vec<u8>,
    ) -> Result<Option<Frame>, Error> {
        let mut start = self.pos;
        let mut head = self.read_head()?;
        while head.major == MAJOR_TAG {
            match head.arg {
                2 | 3 => {
                    self.parse_bignum(out, buffer, head.arg == 3, start)?;
                    return Ok(None);
                }
                0 | 4 | 37 | 1004 if self.parse_well_known(out, buffer, head.arg, start)? => {
                    return Ok(None);
                }
                _ => {}
            }
            self.tags.push(head.arg);
            start = self.pos;
            head = self.read_head()?;
        }

        match head.major {
            MAJOR_UNSIGNED => self.emit(out, start, Atom::U64(head.arg))?,
            MAJOR_NEGATIVE => {
                if head.arg <= i64::MAX as u64 {
                    // -1 - n without overflows
                    self.emit(out, start, Atom::I64(!(head.arg as i64)))?
                } else {
                    let value = -1 - i128::from(head.arg);
                    self.emit(out, start, Atom::Ext(ExtValue::borrowed(&value)))?
                }
            }
            // definite length strings are slices of the input
            MAJOR_BYTES if !head.is_indefinite() => {
                let bytes = self.read_body(head)?;
                self.emit_borrowed(out, start, Event::Atom(Atom::Bytes(Bytes::borrowed(bytes))))?
            }
            MAJOR_TEXT if !head.is_indefinite() => {
                let bytes = self.read_body(head)?;
                // SAFETY: text is validated as UTF-8 when read
                let text = unsafe { str::from_utf8_unchecked(bytes) };
                self.emit_borrowed(out, start, Event::Atom(Atom::Str(Cow::Borrowed(text))))?
            }
            MAJOR_BYTES => {
                let bytes = self.read_string(head, buffer)?;
                self.emit(out, start, Atom::Bytes(Bytes::borrowed(bytes)))?
            }
            MAJOR_TEXT => {
                let bytes = self.read_string(head, buffer)?;
                // SAFETY: text chunks are validated as UTF-8 when read
                let text = unsafe { str::from_utf8_unchecked(bytes) };
                self.emit(out, start, Atom::Str(Cow::Borrowed(text)))?
            }
            MAJOR_ARRAY | MAJOR_MAP => {
                let is_map = head.major == MAJOR_MAP;
                // the declared length is passed on, it's not trusted by sinks
                let mut shape = ContainerShape::new();
                if !head.is_indefinite()
                    && let Ok(len) = usize::try_from(head.arg)
                {
                    shape = shape.with_len(len);
                }
                let frame = Frame {
                    is_map,
                    remaining: if head.is_indefinite() {
                        None
                    } else {
                        Some(head.arg)
                    },
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
            _ => {
                let atom = match head.info {
                    20 => Atom::Bool(false),
                    21 => Atom::Bool(true),
                    // undefined is deserialized as null
                    22 | 23 => Atom::Null,
                    24 => {
                        if head.arg < 32 {
                            return Err(syntax_error(self.base + start, "invalid simple value"));
                        }
                        Atom::Ext(ExtValue::owned(Simple::new(head.arg as u8).unwrap()))
                    }
                    25 => Atom::F64(f16_to_f64(head.arg as u16)),
                    26 => Atom::F64(f64::from(f32::from_bits(head.arg as u32))),
                    27 => Atom::F64(f64::from_bits(head.arg)),
                    INDEFINITE => return Err(syntax_error(self.base + start, "unexpected break")),
                    info => Atom::Ext(ExtValue::owned(Simple::new(info).unwrap())),
                };
                self.emit(out, start, atom)?
            }
        }
        Ok(None)
    }

    /// Emits an event of the item at `start` with the pending tags.
    #[inline(always)]
    fn emit<'e, E: Into<Event<'e>>, O: Out<'a>>(
        &mut self,
        out: &mut O,
        start: usize,
        event: E,
    ) -> Result<(), Error> {
        out.state_mut()
            .set_input_range(self.base + start, self.base + self.pos);
        if !self.tags.is_empty() {
            self.attach_tags(out);
        }
        let rv = out.emit(event);
        if rv.is_err() {
            self.sink_failed = true;
        }
        rv
    }

    /// Emits a borrowed event of the item at `start` with the pending tags.
    #[inline(always)]
    fn emit_borrowed<O: Out<'a>>(
        &mut self,
        out: &mut O,
        start: usize,
        event: Event<'a>,
    ) -> Result<(), Error> {
        out.state_mut()
            .set_input_range(self.base + start, self.base + self.pos);
        if !self.tags.is_empty() {
            self.attach_tags(out);
        }
        let rv = out.emit_input(event);
        if rv.is_err() {
            self.sink_failed = true;
        }
        rv
    }

    /// Attaches the pending tags to the next event.
    #[cold]
    fn attach_tags<O: Out<'a>>(&mut self, out: &mut O) {
        // swapping retains the memory of both vectors
        std::mem::swap(&mut out.state_mut().event_mut::<Tags>().0, &mut self.tags);
        self.tags.clear();
    }

    /// Parses the content of a tag that maps onto a well-known type and
    /// emits it.
    ///
    /// Returns `false` (without consuming anything) if the content does not
    /// match the tag.  It's then emitted as a regular tagged item.
    #[cold]
    fn parse_well_known<O: Out<'a>>(
        &mut self,
        out: &mut O,
        buffer: &mut Vec<u8>,
        tag: u64,
        item_start: usize,
    ) -> Result<bool, Error> {
        let start = self.pos;
        match self.read_well_known(buffer, tag) {
            Some(value) => {
                self.emit(out, item_start, Atom::Ext(value))?;
                Ok(true)
            }
            // incomplete content is read again with more input
            None if self.hit_end => Err(eof_error(self.base + self.input.len())),
            None => {
                self.pos = start;
                Ok(false)
            }
        }
    }

    /// Reads the content of a tag that maps onto a well-known type.
    fn read_well_known(&mut self, buffer: &mut Vec<u8>, tag: u64) -> Option<ExtValue<'static>> {
        let head = self.read_head().ok()?;
        match tag {
            // date/time string (RFC 8949) and full-date string (RFC 8943)
            0 | 1004 => {
                if head.major != MAJOR_TEXT {
                    return None;
                }
                let bytes = self.read_string(head, buffer).ok()?;
                let value: Datetime = str::from_utf8(bytes).ok()?.parse().ok()?;
                let matches = if tag == 0 {
                    value.offset.is_some()
                } else {
                    value.date.is_some() && value.time.is_none()
                };
                matches.then(|| ExtValue::owned(value))
            }
            // decimal fraction
            4 => {
                if head.major != MAJOR_ARRAY || head.is_indefinite() || head.arg != 2 {
                    return None;
                }
                let exponent = i64::try_from(self.read_integer(buffer)?.to_i128()?).ok()?;
                let mantissa = self.read_integer(buffer)?;
                Some(ExtValue::owned(Decimal::from_parts(&mantissa, exponent)))
            }
            // UUID
            37 => {
                if head.major != MAJOR_BYTES {
                    return None;
                }
                let bytes = self.read_string(head, buffer).ok()?;
                Some(ExtValue::owned(Uuid(bytes.try_into().ok()?)))
            }
            _ => None,
        }
    }

    /// Reads an integer or a bignum.
    fn read_integer(&mut self, buffer: &mut Vec<u8>) -> Option<BigInt> {
        let head = self.read_head().ok()?;
        match head.major {
            MAJOR_UNSIGNED => Some(BigInt::from(head.arg)),
            MAJOR_NEGATIVE => Some(BigInt::from(-1 - i128::from(head.arg))),
            MAJOR_TAG if head.arg == 2 || head.arg == 3 => {
                let bytes_head = self.read_head().ok()?;
                if bytes_head.major != MAJOR_BYTES {
                    return None;
                }
                let bytes = self.read_string(bytes_head, buffer).ok()?.to_vec();
                Some(bignum(head.arg == 3, bytes))
            }
            _ => None,
        }
    }

    /// Parses the content of a bignum (tag 2 or 3) and emits it.
    ///
    /// Bignums that fit into 128 bits are emitted as integers, larger ones
    /// are emitted as [`BigInt`].
    #[cold]
    fn parse_bignum<O: Out<'a>>(
        &mut self,
        out: &mut O,
        buffer: &mut Vec<u8>,
        negative: bool,
        item_start: usize,
    ) -> Result<(), Error> {
        let start = self.pos;
        let head = self.read_head()?;
        if head.major != MAJOR_BYTES {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "invalid bignum, expected byte string",
            )
            .with_offset(self.base + start));
        }
        let bytes = self.read_string(head, buffer)?;
        let skip = bytes.iter().take_while(|&&b| b == 0).count();
        let significant = &bytes[skip..];
        let start = item_start;
        if significant.len() > 16 {
            let value = bignum(negative, significant.to_vec());
            return self.emit(out, start, Atom::Ext(ExtValue::owned(value)));
        }
        let mut buf = [0u8; 16];
        buf[16 - significant.len()..].copy_from_slice(significant);
        let value = u128::from_be_bytes(buf);
        if !negative {
            match u64::try_from(value) {
                Ok(value) => self.emit(out, start, Atom::U64(value)),
                Err(_) => self.emit(out, start, Atom::Ext(ExtValue::borrowed(&value))),
            }
        } else if value <= i64::MAX as u128 {
            self.emit(out, start, Atom::I64(-1 - value as i64))
        } else if value <= i128::MAX as u128 {
            let value = -1 - value as i128;
            self.emit(out, start, Atom::Ext(ExtValue::borrowed(&value)))
        } else {
            let value = bignum(true, significant.to_vec());
            self.emit(out, start, Atom::Ext(ExtValue::owned(value)))
        }
    }

    #[inline(always)]
    pub(crate) fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    /// Reads the head of a data item.
    #[inline(always)]
    fn read_head(&mut self) -> Result<Head, Error> {
        let start = self.pos;
        let initial = match self.input.get(start) {
            Some(&byte) => byte,
            None => {
                self.hit_end = true;
                return Err(eof_error(self.base + start));
            }
        };
        self.pos += 1;
        let major = initial >> 5;
        let info = initial & 0x1f;
        let arg = match info {
            0..=23 => u64::from(info),
            24 => u64::from(self.read_array::<1>()?[0]),
            25 => u64::from(u16::from_be_bytes(self.read_array()?)),
            26 => u64::from(u32::from_be_bytes(self.read_array()?)),
            27 => u64::from_be_bytes(self.read_array()?),
            28..=30 => {
                return Err(syntax_error(
                    self.base + start,
                    "reserved additional information",
                ));
            }
            _ => {
                // indefinite lengths are only allowed for strings and
                // containers, for major type 7 this is the break code.
                if matches!(major, MAJOR_UNSIGNED | MAJOR_NEGATIVE | MAJOR_TAG) {
                    return Err(syntax_error(self.base + start, "invalid indefinite length"));
                }
                0
            }
        };
        Ok(Head { major, info, arg })
    }

    #[inline(always)]
    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        match self.input.get(self.pos..self.pos + N) {
            Some(bytes) => {
                self.pos += N;
                Ok(bytes.try_into().unwrap())
            }
            None => {
                self.hit_end = true;
                Err(eof_error(self.base + self.input.len()))
            }
        }
    }

    /// Reads the body of a definite length string.
    #[inline]
    fn read_body(&mut self, head: Head) -> Result<&'a [u8], Error> {
        let len = head.arg;
        let input = self.input;
        if len > (input.len() - self.pos) as u64 {
            self.hit_end = true;
            return Err(eof_error(self.base + input.len()));
        }
        let bytes = &input[self.pos..self.pos + len as usize];
        if head.major == MAJOR_TEXT && !is_ascii(bytes) && !is_utf8(bytes) {
            return Err(syntax_error(
                self.base + self.pos,
                "invalid UTF-8 in text string",
            ));
        }
        self.pos += len as usize;
        Ok(bytes)
    }

    /// Reads a string or byte string which might be split into chunks.
    ///
    /// Text is validated as UTF-8.
    #[inline]
    fn read_string<'b>(&mut self, head: Head, buffer: &'b mut Vec<u8>) -> Result<&'b [u8], Error>
    where
        'a: 'b,
    {
        if !head.is_indefinite() {
            return self.read_body(head);
        }
        buffer.clear();
        loop {
            if self.peek() == Some(BREAK) {
                self.pos += 1;
                return Ok(buffer);
            }
            let start = self.pos;
            let chunk = self.read_head()?;
            if chunk.major != head.major || chunk.is_indefinite() {
                return Err(syntax_error(
                    self.base + start,
                    "invalid chunk in indefinite string",
                ));
            }
            buffer.extend_from_slice(self.read_body(chunk)?);
        }
    }
}

/// Converts the content of a bignum into a [`BigInt`].
///
/// Negative bignums (tag 3) hold `-1 - n`.
fn bignum(negative: bool, mut magnitude: Vec<u8>) -> BigInt {
    if negative {
        // add one to the magnitude
        let mut carry = true;
        for byte in magnitude.iter_mut().rev() {
            let (value, overflow) = byte.overflowing_add(1);
            *byte = value;
            if !overflow {
                carry = false;
                break;
            }
        }
        if carry {
            magnitude.insert(0, 1);
        }
    }
    BigInt {
        negative,
        magnitude,
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
    #[cfg(feature = "simdutf8")]
    {
        simdutf8::basic::from_utf8(bytes).is_ok()
    }
    #[cfg(not(feature = "simdutf8"))]
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
    use deser::de::Recording;

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
            // [_ 1, [2, 3], (_ "ab", "c"), {_ "a": 1}, h'0102']
            "9f 01 820203 7f62616261 63ff bf616101ff 420102 ff",
            // tags: 1(1363896240), 0("2013-03-21T20:04:00Z"), 4([-2, 27315])
            "83 c11a514b67b0 c074323031332d30332d32315432303a30343a30305a c48221196ab3",
            // bignums and UUIDs: 2(h'010000000000000000'), 37(h'...')
            "82 c249010000000000000000 d82550f81d4fae7dec11d0a76500a0c91e6bf6",
            // nested tags and floats
            "a2 6161 d9d9f7 fb3ff199999999999a 6162 f93c00",
            // a byte string with a two byte length
            &format!("59 0100 {}", "aa".repeat(256)),
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
        for input in ["82 01", "9f 01", "a1 61", "1c", "82 01 ff", "7f 01 ff"] {
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
