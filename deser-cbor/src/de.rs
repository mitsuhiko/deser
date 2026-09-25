use std::borrow::Cow;
use std::marker::PhantomData;
use std::str;

use deser::de::{Deserialize, DeserializeDriver};
use deser::ext::ExtValue;
use deser::{Atom, Error, ErrorKind, Event};

use crate::float::f16_to_f64;
use crate::simple::Simple;
use crate::tag::CurrentTags;

const MAJOR_UNSIGNED: u8 = 0;
const MAJOR_NEGATIVE: u8 = 1;
const MAJOR_BYTES: u8 = 2;
const MAJOR_TEXT: u8 = 3;
const MAJOR_ARRAY: u8 = 4;
const MAJOR_MAP: u8 = 5;
const MAJOR_TAG: u8 = 6;

/// The additional information for indefinite lengths.
const INDEFINITE: u8 = 31;
const BREAK: u8 = 0xff;

/// Deserializes a deserializable from CBOR.
///
/// A deserializer reads data items from a slice.  Because CBOR sequences
/// (RFC 8742) are just data items following each other, a deserializer can
/// be used to read more than one item:
///
/// ```
/// use deser_cbor::Deserializer;
///
/// let mut de = Deserializer::new(&[0x01, 0x62, b'h', b'i']);
/// assert_eq!(de.deserialize::<u32>().unwrap(), 1);
/// assert_eq!(de.deserialize::<String>().unwrap(), "hi");
/// assert!(de.is_end());
/// ```
pub struct Deserializer<'a> {
    input: &'a [u8],
    pos: usize,
    max_depth: Option<usize>,
    // scratch space for strings split into chunks
    buffer: Vec<u8>,
    // the tags in front of the current item
    tags: Vec<u64>,
}

/// An open array or map.
#[derive(Clone, Copy)]
struct Frame {
    is_map: bool,
    /// The number of items (array) or entries (map) that are still expected.
    /// `None` for indefinite length containers.
    remaining: Option<u64>,
    /// For maps: `true` if a value is expected next.
    in_value: bool,
}

/// The head of a data item.
#[derive(Clone, Copy)]
struct Head {
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

impl<'a> Deserializer<'a> {
    /// Creates a new deserializer.
    pub fn new(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer {
            input,
            pos: 0,
            max_depth: None,
            buffer: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// Limits the nesting depth of arrays and maps.
    ///
    /// Deser does not use the stack to process nested data so arbitrarily
    /// deep structures do not overflow the stack.  Still it can be useful to
    /// limit the depth of untrusted inputs.  By default the depth is not
    /// limited.
    pub fn max_depth(mut self, depth: Option<usize>) -> Deserializer<'a> {
        self.max_depth = depth;
        self
    }

    /// Returns the current offset in the input.
    pub fn offset(&self) -> usize {
        self.pos
    }

    /// Returns `true` if the entire input was consumed.
    pub fn is_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    /// Fails if the input was not consumed entirely.
    pub fn end(&self) -> Result<(), Error> {
        if self.is_end() {
            Ok(())
        } else {
            Err(syntax_error(self.pos, "trailing data after item"))
        }
    }

    /// Deserializes the next data item.
    ///
    /// This does not check if there is more data after the item.  Use
    /// [`end`](Self::end) for this or [`from_slice`] which does it
    /// automatically.
    pub fn deserialize<T: Deserialize>(&mut self) -> Result<T, Error> {
        let mut out = None;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            self.drive(&mut driver)?;
        }
        out.take()
            .ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))
    }

    /// Returns an iterator over the remaining data items.
    ///
    /// This is useful to read CBOR sequences.  The iterator stops after
    /// the first error.
    ///
    /// ```
    /// let mut de = deser_cbor::Deserializer::new(&[0x01, 0x02, 0x03]);
    /// let items = de.iter::<u32>().collect::<Result<Vec<_>, _>>().unwrap();
    /// assert_eq!(items, [1, 2, 3]);
    /// ```
    pub fn iter<T: Deserialize>(&mut self) -> Iter<'_, 'a, T> {
        Iter {
            de: self,
            failed: false,
            _marker: PhantomData,
        }
    }

    /// Parses the next data item and feeds the events into the given driver.
    ///
    /// This is useful to deserialize into a custom
    /// [`Sink`](deser::de::Sink) or to wrap the sink of a value.
    pub fn drive(&mut self, driver: &mut DeserializeDriver) -> Result<(), Error> {
        // the scratch buffer is moved out of the deserializer so that
        // strings borrowing from it do not borrow the deserializer.
        let mut buffer = std::mem::take(&mut self.buffer);
        let rv = self.drive_impl(driver, &mut buffer);
        self.buffer = buffer;
        rv
    }

    fn drive_impl(
        &mut self,
        driver: &mut DeserializeDriver,
        buffer: &mut Vec<u8>,
    ) -> Result<(), Error> {
        // the current container is held in a local, the outer containers are
        // saved on the stack.
        let mut stack = Vec::new();
        let mut frame: Option<Frame> = None;
        self.tags.clear();

        loop {
            // close all completed containers and account for the next item
            while let Some(ref mut current) = frame {
                let done = match current.remaining {
                    Some(0) => !current.in_value,
                    Some(_) => false,
                    None => self.peek() == Some(BREAK),
                };
                if !done {
                    if current.is_map {
                        if !current.in_value {
                            if let Some(ref mut remaining) = current.remaining {
                                *remaining -= 1;
                            }
                        }
                        current.in_value = !current.in_value;
                    } else if let Some(ref mut remaining) = current.remaining {
                        *remaining -= 1;
                    }
                    break;
                }
                if current.remaining.is_none() {
                    if current.in_value {
                        return Err(syntax_error(self.pos, "missing map value"));
                    }
                    self.pos += 1;
                }
                driver.emit(if current.is_map {
                    Event::MapEnd
                } else {
                    Event::SeqEnd
                })?;
                frame = stack.pop();
                if frame.is_none() {
                    return Ok(());
                }
            }

            let depth = stack.len() + usize::from(frame.is_some());
            if let Some(new) = self.parse_item(driver, buffer, depth)? {
                if let Some(outer) = frame.replace(new) {
                    stack.push(outer);
                }
            } else if frame.is_none() {
                return Ok(());
            }
        }
    }

    /// Parses a data item including its tags and emits its (first) event.
    ///
    /// If the item is an array or map, the frame for it is returned.
    #[inline]
    fn parse_item(
        &mut self,
        driver: &mut DeserializeDriver,
        buffer: &mut Vec<u8>,
        depth: usize,
    ) -> Result<Option<Frame>, Error> {
        let mut start = self.pos;
        let mut head = self.read_head()?;
        while head.major == MAJOR_TAG {
            if head.arg == 2 || head.arg == 3 {
                self.parse_bignum(driver, buffer, head.arg == 3)?;
                return Ok(None);
            }
            self.tags.push(head.arg);
            start = self.pos;
            head = self.read_head()?;
        }

        match head.major {
            MAJOR_UNSIGNED => self.emit(driver, Atom::U64(head.arg))?,
            MAJOR_NEGATIVE => {
                if head.arg <= i64::MAX as u64 {
                    // -1 - n without overflows
                    self.emit(driver, Atom::I64(!(head.arg as i64)))?
                } else {
                    let value = -1 - i128::from(head.arg);
                    self.emit(driver, Atom::Ext(ExtValue::borrowed(&value)))?
                }
            }
            MAJOR_BYTES => {
                let bytes = self.read_string(head, buffer)?;
                self.emit(driver, Atom::Bytes(Cow::Borrowed(bytes)))?
            }
            MAJOR_TEXT => {
                let bytes = self.read_string(head, buffer)?;
                // SAFETY: text chunks are validated as UTF-8 when read
                let text = unsafe { str::from_utf8_unchecked(bytes) };
                self.emit(driver, Atom::Str(Cow::Borrowed(text)))?
            }
            MAJOR_ARRAY | MAJOR_MAP => {
                let is_map = head.major == MAJOR_MAP;
                if self.max_depth.is_some_and(|max| depth >= max) {
                    return Err(Error::new(
                        ErrorKind::Unexpected,
                        format!("recursion limit exceeded at offset {}", start),
                    ));
                }
                self.emit(
                    driver,
                    if is_map {
                        Event::MapStart
                    } else {
                        Event::SeqStart
                    },
                )?;
                return Ok(Some(Frame {
                    is_map,
                    remaining: if head.is_indefinite() {
                        None
                    } else {
                        Some(head.arg)
                    },
                    in_value: false,
                }));
            }
            _ => {
                let atom = match head.info {
                    20 => Atom::Bool(false),
                    21 => Atom::Bool(true),
                    // undefined is deserialized as null
                    22 | 23 => Atom::Null,
                    24 => {
                        if head.arg < 32 {
                            return Err(syntax_error(start, "invalid simple value"));
                        }
                        Atom::Ext(ExtValue::owned(Simple::new(head.arg as u8).unwrap()))
                    }
                    25 => Atom::F64(f16_to_f64(head.arg as u16)),
                    26 => Atom::F64(f64::from(f32::from_bits(head.arg as u32))),
                    27 => Atom::F64(f64::from_bits(head.arg)),
                    INDEFINITE => return Err(syntax_error(start, "unexpected break")),
                    info => Atom::Ext(ExtValue::owned(Simple::new(info).unwrap())),
                };
                self.emit(driver, atom)?
            }
        }
        Ok(None)
    }

    /// Emits an event with the pending tags.
    #[inline(always)]
    fn emit<'e, E: Into<Event<'e>>>(
        &mut self,
        driver: &mut DeserializeDriver,
        event: E,
    ) -> Result<(), Error> {
        if self.tags.is_empty() {
            driver.emit(event)
        } else {
            self.emit_tagged(driver, event.into())
        }
    }

    #[cold]
    fn emit_tagged(&mut self, driver: &mut DeserializeDriver, event: Event) -> Result<(), Error> {
        let tags = &mut self.tags;
        let rv = driver.emit_with(event, |state| {
            // swapping retains the memory of both vectors
            std::mem::swap(&mut state.event_mut::<CurrentTags>().0, tags);
        });
        self.tags.clear();
        rv
    }

    /// Parses the content of a bignum (tag 2 or 3) and emits it.
    ///
    /// Bignums that fit into 128 bits are emitted as integers, larger ones
    /// are emitted as tagged byte strings.
    #[cold]
    fn parse_bignum(
        &mut self,
        driver: &mut DeserializeDriver,
        buffer: &mut Vec<u8>,
        negative: bool,
    ) -> Result<(), Error> {
        let start = self.pos;
        let head = self.read_head()?;
        if head.major != MAJOR_BYTES {
            return Err(Error::new(
                ErrorKind::Unexpected,
                format!("invalid bignum at offset {}, expected byte string", start),
            ));
        }
        let bytes = self.read_string(head, buffer)?;
        let skip = bytes.iter().take_while(|&&b| b == 0).count();
        let significant = &bytes[skip..];
        if significant.len() > 16 {
            self.tags.push(if negative { 3 } else { 2 });
            return self.emit(driver, Atom::Bytes(Cow::Borrowed(bytes)));
        }
        let mut buf = [0u8; 16];
        buf[16 - significant.len()..].copy_from_slice(significant);
        let value = u128::from_be_bytes(buf);
        if !negative {
            match u64::try_from(value) {
                Ok(value) => self.emit(driver, Atom::U64(value)),
                Err(_) => self.emit(driver, Atom::Ext(ExtValue::borrowed(&value))),
            }
        } else if value <= i64::MAX as u128 {
            self.emit(driver, Atom::I64(-1 - value as i64))
        } else if value <= i128::MAX as u128 {
            let value = -1 - value as i128;
            self.emit(driver, Atom::Ext(ExtValue::borrowed(&value)))
        } else {
            self.tags.push(3);
            self.emit(driver, Atom::Bytes(Cow::Borrowed(bytes)))
        }
    }

    #[inline(always)]
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    /// Reads the head of a data item.
    #[inline(always)]
    fn read_head(&mut self) -> Result<Head, Error> {
        let start = self.pos;
        let initial = match self.input.get(start) {
            Some(&byte) => byte,
            None => return Err(eof_error(start)),
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
                return Err(syntax_error(start, "reserved additional information"));
            }
            _ => {
                // indefinite lengths are only allowed for strings and
                // containers, for major type 7 this is the break code.
                if matches!(major, MAJOR_UNSIGNED | MAJOR_NEGATIVE | MAJOR_TAG) {
                    return Err(syntax_error(start, "invalid indefinite length"));
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
            None => Err(eof_error(self.input.len())),
        }
    }

    /// Reads the body of a definite length string.
    #[inline]
    fn read_body(&mut self, head: Head) -> Result<&'a [u8], Error> {
        let len = head.arg;
        let input = self.input;
        if len > (input.len() - self.pos) as u64 {
            return Err(eof_error(input.len()));
        }
        let bytes = &input[self.pos..self.pos + len as usize];
        if head.major == MAJOR_TEXT && !is_ascii(bytes) && !is_utf8(bytes) {
            return Err(syntax_error(self.pos, "invalid UTF-8 in text string"));
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
                return Err(syntax_error(start, "invalid chunk in indefinite string"));
            }
            buffer.extend_from_slice(self.read_body(chunk)?);
        }
    }
}

/// An iterator over the data items of a CBOR sequence.
///
/// See [`Deserializer::iter`].
pub struct Iter<'de, 'a, T> {
    de: &'de mut Deserializer<'a>,
    failed: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<'de, 'a, T: Deserialize> Iterator for Iter<'de, 'a, T> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.de.is_end() {
            return None;
        }
        let rv = self.de.deserialize();
        self.failed = rv.is_err();
        Some(rv)
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

#[cold]
fn syntax_error(offset: usize, msg: &str) -> Error {
    Error::new(
        ErrorKind::Unexpected,
        format!("syntax error at offset {}: {}", offset, msg),
    )
}

#[cold]
fn eof_error(offset: usize) -> Error {
    Error::new(
        ErrorKind::EndOfFile,
        format!("unexpected end of input at offset {}", offset),
    )
}

/// Deserializes a value from CBOR.
///
/// The input must contain exactly one data item.
pub fn from_slice<T: Deserialize>(input: &[u8]) -> Result<T, Error> {
    let mut de = Deserializer::new(input);
    let rv = de.deserialize()?;
    de.end()?;
    Ok(rv)
}
