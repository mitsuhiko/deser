use std::str;

use deser::de::{Deserialize, DeserializeDriver, Format};
use deser::ext::{ExtValue, Number as ExactNumber};
use deser::Atom;
use deser::Event;
use deser::{Error, ErrorKind};

use crate::scan::{is_ascii, skip_to_escape, validate_utf8_slice};

/// A parsed string.
enum Str<'a, 'b> {
    /// The string is a slice of the input.
    Borrowed(&'a str),
    /// The string was unescaped into the scratch buffer.
    Scratch(&'b str),
}

enum Number<'a> {
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

/// Configures how JSON is deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The methods
/// [`from_str`](Self::from_str) and [`from_slice`](Self::from_slice) work
/// like the functions of the same name.  To create a [`Deserializer`] with
/// the configuration use [`Deserializer::from_str_with_config`] or
/// [`Deserializer::from_slice_with_config`].
///
/// ```
/// use deser_json::DeserializerConfig;
///
/// const CONFIG: DeserializerConfig = DeserializerConfig::new().exact_numbers(false);
/// let value: Vec<f64> = CONFIG.from_str("[0.10, 1e5]").unwrap();
/// assert_eq!(value, [0.1, 1e5]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializerConfig {
    track_locations: bool,
    exact_numbers: bool,
}

impl Default for DeserializerConfig {
    fn default() -> DeserializerConfig {
        DeserializerConfig::new()
    }
}

impl DeserializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> DeserializerConfig {
        DeserializerConfig {
            track_locations: false,
            exact_numbers: true,
        }
    }

    /// Enables or disables location tracking.
    ///
    /// The byte range of every event is always published into the state
    /// (see [`State::input_range`](deser::State::input_range)).  When
    /// enabled additionally the input is set as source (see
    /// [`State::source`](deser::State::source)) which allows resolving the
    /// ranges into lines and columns, for instance with the `Spanned` type
    /// of [`deser-location`](https://docs.rs/deser-location).  This copies
    /// the input.
    pub const fn track_locations(mut self, yes: bool) -> DeserializerConfig {
        self.track_locations = yes;
        self
    }

    /// Enables or disables exact numbers.
    ///
    /// When enabled (which is the default) floats which lose precision as
    /// `f64` and integers that do not fit into 128 bits are emitted as
    /// [`Number`](deser::ext::Number) extension values.  These carry the
    /// text of the number together with its value as `f64`, which is what
    /// types that do not know about exact numbers receive.  Types like
    /// [`Decimal`](deser::ext::Decimal) (and the types of `rust_decimal` or
    /// `bigdecimal`) use the text to deserialize the number exactly:
    ///
    /// ```
    /// use deser::ext::Decimal;
    ///
    /// let value: Decimal = deser_json::from_str("0.10000000000000000001").unwrap();
    /// assert_eq!(value.as_str(), "0.10000000000000000001");
    /// let value: f64 = deser_json::from_str("0.10000000000000000001").unwrap();
    /// assert_eq!(value, 0.1);
    /// ```
    ///
    /// Floats whose text is the shortest representation of their value (as
    /// formatted by `Debug`, for instance `0.5` or `3.14`) are emitted as
    /// `F64` as the text can be recovered from the value.  This keeps the
    /// common case fast.  When disabled, all floats are emitted as `F64`.
    pub const fn exact_numbers(mut self, yes: bool) -> DeserializerConfig {
        self.exact_numbers = yes;
        self
    }

    /// Deserializes JSON from the given string.
    pub fn from_str<'de, T: Deserialize<'de>>(&self, s: &'de str) -> Result<T, Error> {
        Deserializer::from_str_with_config(s, self).deserialize()
    }

    /// Deserializes JSON from the given bytes.
    ///
    /// The input must be UTF-8.  Rather than validating the input upfront,
    /// the strings are validated while parsing (see
    /// [`Deserializer::from_slice`]).
    pub fn from_slice<'de, T: Deserialize<'de>>(&self, bytes: &'de [u8]) -> Result<T, Error> {
        Deserializer::from_slice_with_config(bytes, self).deserialize()
    }
}

/// Deserializes a serializable from JSON.
///
/// Most of the time the [`from_str`](crate::from_str) and
/// [`from_slice`](crate::from_slice) functions (or the methods of the same
/// name on [`DeserializerConfig`]) are all that is needed.  The
/// deserializer is useful to [`drive`](Self::drive) a custom sink.
pub struct Deserializer<'a> {
    input: &'a [u8],
    pos: usize,
    buffer: Vec<u8>,
    // `true` if the input is a byte slice which needs to be validated
    validate_utf8: bool,
    config: DeserializerConfig,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    Top,
    Seq,
    Map,
}

impl<'a> Deserializer<'a> {
    /// Creates a new deserializer for a string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(input: &'a str) -> Deserializer<'a> {
        Deserializer::from_str_with_config(input, &DeserializerConfig::new())
    }

    /// Creates a new deserializer for a string with the given configuration.
    pub fn from_str_with_config(input: &'a str, config: &DeserializerConfig) -> Deserializer<'a> {
        Deserializer {
            // the parser works on bytes but relies on the input being valid
            // UTF-8 when it hands out string slices.
            input: input.as_bytes(),
            validate_utf8: false,
            pos: 0,
            buffer: Vec::new(),
            config: config.clone(),
        }
    }

    /// Creates a new deserializer for a byte slice.
    ///
    /// The input is not validated upfront.  Instead strings are validated as
    /// UTF-8 when they are parsed (bytes outside of strings are only ever
    /// accepted if they are ASCII).  Invalid UTF-8 is an error.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input, &DeserializerConfig::new())
    }

    /// Creates a new deserializer for a byte slice with the given
    /// configuration.
    ///
    /// See [`from_slice`](Self::from_slice).
    pub fn from_slice_with_config(
        input: &'a [u8],
        config: &DeserializerConfig,
    ) -> Deserializer<'a> {
        Deserializer {
            input,
            validate_utf8: true,
            pos: 0,
            buffer: Vec::new(),
            config: config.clone(),
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Returns the input as string for the source.
    fn source(&self) -> std::borrow::Cow<'a, str> {
        if self.validate_utf8 {
            // invalid UTF-8 fails the parsing when reached, the offsets of
            // the tokens before it are not affected by the replacements.
            String::from_utf8_lossy(self.input)
        } else {
            // SAFETY: the input was created from a string
            std::borrow::Cow::Borrowed(unsafe { str::from_utf8_unchecked(self.input) })
        }
    }

    /// Deserializes the value.
    ///
    /// To configure the deserialization (for instance to add layers) use
    /// [`Format::deserialize_with`].
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        Format::deserialize(self)
    }

    /// Parses the input and feeds the events into the given driver.
    ///
    /// This is useful to deserialize into a custom [`Sink`](deser::de::Sink).
    /// See also [`Format::deserialize_with`].
    ///
    /// Strings without escape sequences are passed on borrowed from the
    /// input (see [`emit_borrowed`](DeserializeDriver::emit_borrowed)).
    /// Errors carry the location in the input (see [`Error::line`]).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        // the scratch buffer for strings is moved out of the deserializer
        // so that tokens borrowing from it do not borrow the deserializer.
        let mut buffer = std::mem::take(&mut self.buffer);
        if self.config.track_locations {
            let source = self.source();
            driver.state_mut().set_source(source);
        }
        let rv = self.drive_impl(driver, &mut buffer);
        self.buffer = buffer;
        rv.map_err(|err| self.locate_error(err))
    }

    /// Attaches the location to an error.
    #[cold]
    fn locate_error(&self, err: Error) -> Error {
        // errors of the parser are located at the current position, errors
        // of the sinks at the event that failed
        let err = match err.offset() {
            Some(_) => err,
            None => err.with_offset(self.pos),
        };
        err.resolve_position(self.input)
    }

    fn drive_impl(
        &mut self,
        driver: &mut DeserializeDriver<'_, 'a>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), Error> {
        // tokens start with the byte consumed by `next_token_byte`.  The
        // start is derived from the position rather than stored as this
        // keeps the tokenizer fast.
        macro_rules! emit {
            ($start:expr, $event:expr) => {{
                driver.state_mut().set_input_range($start, self.pos);
                driver.emit($event)?
            }};
        }

        // the state of the current container is held in a local, the outer
        // containers are saved on the stack.
        let mut stack = Vec::new();
        let mut container = Container::Top;

        'value: loop {
            let byte = self.next_token_byte()?;
            let start = self.pos - 1;
            match byte {
                b'"' => match self.parse_str(buffer)? {
                    Str::Borrowed(val) => {
                        driver.state_mut().set_input_range(start, self.pos);
                        driver.emit_borrowed(val)?
                    }
                    Str::Scratch(val) => emit!(start, Event::from(val)),
                },
                b'0'..=b'9' => {
                    let number = self.parse_integer(true, byte)?;
                    emit_number(
                        driver,
                        number,
                        self.input,
                        self.config.exact_numbers,
                        start,
                        self.pos,
                    )?
                }
                b'-' => {
                    let first_digit = self.next_or_nul();
                    let number = self.parse_integer(false, first_digit)?;
                    emit_number(
                        driver,
                        number,
                        self.input,
                        self.config.exact_numbers,
                        start,
                        self.pos,
                    )?
                }
                b'n' => {
                    self.parse_ident(b"ull")?;
                    emit!(start, Event::Atom(Atom::Null))
                }
                b't' => {
                    self.parse_ident(b"rue")?;
                    emit!(start, Event::from(true))
                }
                b'f' => {
                    self.parse_ident(b"alse")?;
                    emit!(start, Event::from(false))
                }
                b'{' | b'[' => {
                    stack.push(container);
                    let close = if byte == b'{' {
                        container = Container::Map;
                        emit!(start, Event::MapStart);
                        b'}'
                    } else {
                        container = Container::Seq;
                        emit!(start, Event::SeqStart);
                        b']'
                    };
                    // containers can close immediately, otherwise the first
                    // value follows.
                    if self.parse_whitespace() != Some(close) {
                        if container == Container::Map {
                            self.parse_key(driver, buffer)?;
                        }
                        continue 'value;
                    }
                    self.next_token_byte()?;
                    emit!(
                        self.pos - 1,
                        if close == b'}' {
                            Event::MapEnd
                        } else {
                            Event::SeqEnd
                        }
                    );
                    container = stack.pop().unwrap_or(Container::Top);
                }
                b',' => return Err(token_error(start, "unexpected comma")),
                b':' => return Err(token_error(start, "unexpected colon")),
                b']' | b'}' => return Err(token_error(start, "expected a value")),
                _ => return Err(token_error(start, "unexpected character")),
            }

            // a value was completed, either the container ends or the next
            // value follows.
            loop {
                let close = match container {
                    Container::Top => {
                        return if self.parse_whitespace().is_some() {
                            Err(Error::new(ErrorKind::Unexpected, "garbage after input"))
                        } else {
                            Ok(())
                        };
                    }
                    Container::Map => b'}',
                    Container::Seq => b']',
                };
                match self.parse_whitespace() {
                    Some(b',') => {
                        self.bump();
                        if container == Container::Map {
                            self.parse_key(driver, buffer)?;
                        }
                        continue 'value;
                    }
                    Some(byte) if byte == close => {
                        self.next_token_byte()?;
                        emit!(
                            self.pos - 1,
                            if close == b'}' {
                                Event::MapEnd
                            } else {
                                Event::SeqEnd
                            }
                        );
                        container = stack.pop().unwrap_or(Container::Top);
                    }
                    Some(b']' | b'}') => {
                        return Err(Error::new(
                            ErrorKind::Unexpected,
                            if container == Container::Map {
                                "unexpected end of seq"
                            } else {
                                "unexpected end of map"
                            },
                        ));
                    }
                    Some(_) => {
                        return Err(Error::new(ErrorKind::Unexpected, "expected a comma"));
                    }
                    None => {
                        return Err(Error::new(ErrorKind::EndOfFile, "unexpected end of file"));
                    }
                }
            }
        }
    }

    /// Parses a map key and the colon after it.
    #[inline]
    fn parse_key(
        &mut self,
        driver: &mut DeserializeDriver<'_, 'a>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), Error> {
        if self.next_token_byte()? != b'"' {
            return Err(token_error(self.pos - 1, "expected map key"));
        }
        let start = self.pos - 1;
        let key = self.parse_str(buffer)?;
        driver.state_mut().set_input_range(start, self.pos);
        match key {
            Str::Borrowed(key) => driver.emit_borrowed(key)?,
            Str::Scratch(key) => driver.emit(key)?,
        }
        match self.parse_whitespace() {
            Some(b':') => {
                self.bump();
                Ok(())
            }
            Some(_) => Err(Error::new(ErrorKind::Unexpected, "expected colon")),
            None => Err(Error::new(ErrorKind::EndOfFile, "unexpected end of file")),
        }
    }

    /// Skips whitespace and consumes the first byte of the next token.
    #[inline]
    fn next_token_byte(&mut self) -> Result<u8, Error> {
        match self.parse_whitespace() {
            Some(byte) => {
                self.bump();
                Ok(byte)
            }
            None => Err(Error::new(ErrorKind::EndOfFile, "unexpected end of file")),
        }
    }

    fn next(&mut self) -> Option<u8> {
        if self.pos < self.input.len() {
            let ch = self.input[self.pos];
            self.pos += 1;
            Some(ch)
        } else {
            None
        }
    }

    fn next_or_nul(&mut self) -> u8 {
        self.next().unwrap_or(b'\0')
    }

    fn peek(&mut self) -> Option<u8> {
        if self.pos < self.input.len() {
            Some(self.input[self.pos])
        } else {
            None
        }
    }

    fn peek_or_nul(&mut self) -> u8 {
        self.peek().unwrap_or(b'\0')
    }

    fn bump(&mut self) {
        self.pos += 1;
    }

    fn parse_str<'b>(&mut self, buffer: &'b mut Vec<u8>) -> Result<Str<'a, 'b>, Error> {
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
        let mut start = self.pos;
        buffer.clear();

        loop {
            self.pos = skip_to_escape(self.input, self.pos);
            if self.pos == self.input.len() {
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
                        let borrowed = &input[start..self.pos];
                        self.pos += 1;
                        return result(validate_utf8, borrowed).map(Str::Borrowed);
                    } else {
                        buffer.extend_from_slice(&self.input[start..self.pos]);
                        self.pos += 1;
                        return result(validate_utf8, buffer).map(Str::Scratch);
                    }
                }
                b'\\' => {
                    buffer.extend_from_slice(&self.input[start..self.pos]);
                    self.pos += 1;
                    self.parse_escape(buffer)?;
                    start = self.pos;
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

    #[inline]
    fn parse_whitespace(&mut self) -> Option<u8> {
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
                other => {
                    self.pos = pos;
                    return other.copied();
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
fn emit_number(
    driver: &mut DeserializeDriver<'_, '_>,
    number: Number,
    input: &[u8],
    exact_numbers: bool,
    start: usize,
    end: usize,
) -> Result<(), Error> {
    driver.state_mut().set_input_range(start, end);
    match number {
        Number::U64(val) => driver.emit(Event::from(val)),
        Number::I64(val) => driver.emit(Event::from(val)),
        Number::F64(val) => driver.emit(Event::from(val)),
        Number::Literal(val) if exact_numbers => emit_literal(driver, input, val, start, end),
        Number::Literal(val) => driver.emit(Event::from(val)),
        Number::BigInt(val) => emit_big_int(driver, val),
    }
}

/// Emits a number as number extension value with its text.
///
/// This is not inlined to keep the code of the parser loop small.
#[inline(never)]
fn emit_literal(
    driver: &mut DeserializeDriver<'_, '_>,
    input: &[u8],
    value: f64,
    start: usize,
    end: usize,
) -> Result<(), Error> {
    // SAFETY: numbers only consist of ASCII characters
    let text = unsafe { str::from_utf8_unchecked(&input[start..end]) };
    let number = ExactNumber::new(text, value);
    driver.emit(Atom::Ext(ExtValue::borrowed_value::<ExactNumber>(&number)))
}

/// Emits an integer that does not fit into 64 bits as extension value.
#[cold]
fn emit_big_int(driver: &mut DeserializeDriver<'_, '_>, text: &str) -> Result<(), Error> {
    // the tokenizer already validated that the value fits
    if text.starts_with('-') {
        let value: i128 = text.parse().unwrap();
        driver.emit(Atom::Ext(ExtValue::borrowed(&value)))
    } else {
        let value: u128 = text.parse().unwrap();
        driver.emit(Atom::Ext(ExtValue::borrowed(&value)))
    }
}

impl<'a> Format<'a> for Deserializer<'a> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        Deserializer::drive(self, driver)
    }
}

/// Deserializes JSON from the given string.
///
/// This uses the default [`DeserializerConfig`].
pub fn from_str<'de, T: Deserialize<'de>>(s: &'de str) -> Result<T, Error> {
    Deserializer::from_str(s).deserialize()
}

/// Deserializes JSON from the given bytes.
///
/// The input must be UTF-8.  Rather than validating the input upfront, the
/// strings are validated while parsing (see [`Deserializer::from_slice`]).
/// This uses the default [`DeserializerConfig`].
pub fn from_slice<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    Deserializer::from_slice(bytes).deserialize()
}
