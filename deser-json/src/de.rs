use std::str;

use deser::de::{Deserialize, DeserializeDriver};
use deser::ext::ExtValue;
use deser::Atom;
use deser::Event;
use deser::{Error, ErrorKind};

use crate::scan::skip_to_escape;

enum Token<'a> {
    Null,
    Bool(bool),
    Str(&'a str),
    I64(i64),
    /// An integer that does not fit into 64 bits but into 128 bits.  This
    /// holds the (validated) textual representation.
    BigInt(&'a str),
    U64(u64),
    F64(f64),
    SeqStart,
    SeqEnd,
    MapStart,
    MapEnd,
    Comma,
    Colon,
}

macro_rules! overflow {
    ($a:ident * 10 + $b:ident, $c:expr) => {
        $a >= $c / 10 && ($a > $c / 10 || $b > $c % 10)
    };
}

/// Deserializes a serializable from JSON.
pub struct Deserializer<'a> {
    input: &'a [u8],
    pos: usize,
    buffer: Vec<u8>,
    // the offset where the last token started
    #[cfg(feature = "locations")]
    token_start: usize,
    #[cfg(feature = "locations")]
    source: &'a str,
    #[cfg(feature = "locations")]
    track_locations: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    Top,
    Seq,
    Map,
}

impl<'a> Deserializer<'a> {
    /// Creates a new deserializer.
    pub fn new(input: &'a str) -> Deserializer<'a> {
        Deserializer {
            // the parser works on bytes but relies on the input being valid
            // UTF-8 when it hands out string slices.
            input: input.as_bytes(),
            pos: 0,
            buffer: Vec::new(),
            #[cfg(feature = "locations")]
            token_start: 0,
            #[cfg(feature = "locations")]
            source: input,
            #[cfg(feature = "locations")]
            track_locations: false,
        }
    }

    /// Enables or disables location tracking.
    ///
    /// When enabled the byte offsets of every event and a source map are
    /// published into the deserializer state as
    /// [`Locations`](deser_location::Locations).  Types like
    /// [`Spanned`](deser_location::Spanned) can then pick them up.
    #[cfg(feature = "locations")]
    pub fn track_locations(mut self, yes: bool) -> Deserializer<'a> {
        self.track_locations = yes;
        self
    }

    /// Publishes the offsets of the token that was parsed last.
    #[inline(always)]
    fn publish_span<const LOCATIONS: bool>(&mut self, driver: &DeserializeDriver) {
        #[cfg(feature = "locations")]
        if LOCATIONS {
            deser_location::Locations::set_current(driver.state(), self.token_start, self.pos);
        }
        #[cfg(not(feature = "locations"))]
        let _ = driver;
    }

    /// Deserializes the value.
    pub fn deserialize<T: Deserialize>(&mut self) -> Result<T, Error> {
        let mut out = None;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            self.drive(&mut driver)?;
        }
        out.take()
            .ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))
    }

    /// Parses the input and feeds the events into the given driver.
    ///
    /// This is useful to deserialize into a custom [`Sink`](deser::de::Sink)
    /// or to wrap the sink of a value, for instance to track the path.
    pub fn drive(&mut self, driver: &mut DeserializeDriver) -> Result<(), Error> {
        // the scratch buffer for strings is moved out of the deserializer
        // so that tokens borrowing from it do not borrow the deserializer.
        let mut buffer = std::mem::take(&mut self.buffer);
        #[cfg(feature = "locations")]
        let rv = if self.track_locations {
            deser_location::Locations::set_source_map(
                driver.state(),
                std::sync::Arc::new(deser_location::SourceMap::new(self.source)),
            );
            self.drive_impl::<true>(driver, &mut buffer)
        } else {
            self.drive_impl::<false>(driver, &mut buffer)
        };
        #[cfg(not(feature = "locations"))]
        let rv = self.drive_impl::<false>(driver, &mut buffer);
        self.buffer = buffer;
        rv
    }

    fn drive_impl<const LOCATIONS: bool>(
        &mut self,
        driver: &mut DeserializeDriver,
        buffer: &mut Vec<u8>,
    ) -> Result<(), Error> {
        macro_rules! emit {
            ($emit:expr) => {{
                self.publish_span::<LOCATIONS>(driver);
                $emit?
            }};
        }

        // the state of the current container is held in locals, the outer
        // containers are saved on the stack.
        let mut stack = Vec::new();
        let mut container = Container::Top;
        let mut first = true;

        loop {
            let mut token = self.next_token(buffer)?;

            match token {
                Token::MapEnd | Token::SeqEnd => {
                    let (expected, event) = match token {
                        Token::MapEnd => (Container::Map, Event::MapEnd),
                        _ => (Container::Seq, Event::SeqEnd),
                    };
                    if container != expected {
                        return Err(Error::new(
                            ErrorKind::Unexpected,
                            if expected == Container::Map {
                                "unexpected end of map"
                            } else {
                                "unexpected end of seq"
                            },
                        ));
                    }
                    emit!(driver.emit(event));
                    container = stack.pop().unwrap_or(Container::Top);
                }
                _ => {
                    if !first {
                        if !matches!(token, Token::Comma) {
                            return Err(Error::new(ErrorKind::Unexpected, "expected a comma"));
                        }
                        token = self.next_token(buffer)?;
                    }

                    if container == Container::Map {
                        match token {
                            Token::Str(val) => emit!(driver.emit(Event::from(val))),
                            _ => return Err(Error::new(ErrorKind::Unexpected, "expected map key")),
                        }
                        match self.next_token(buffer)? {
                            Token::Colon => {}
                            _ => return Err(Error::new(ErrorKind::Unexpected, "expected colon")),
                        }
                        token = self.next_token(buffer)?;
                    }

                    match token {
                        Token::Null => emit!(driver.emit(Event::Atom(Atom::Null))),
                        Token::Bool(val) => emit!(driver.emit(Event::from(val))),
                        Token::Str(val) => emit!(driver.emit(Event::from(val))),
                        Token::I64(val) => emit!(driver.emit(Event::from(val))),
                        Token::U64(val) => emit!(driver.emit(Event::from(val))),
                        Token::F64(val) => emit!(driver.emit(Event::from(val))),
                        Token::BigInt(val) => emit!(emit_big_int(driver, val)),
                        Token::MapStart | Token::SeqStart => {
                            stack.push(container);
                            first = true;
                            if let Token::MapStart = token {
                                container = Container::Map;
                                emit!(driver.emit(Event::MapStart));
                            } else {
                                container = Container::Seq;
                                emit!(driver.emit(Event::SeqStart));
                            }
                            // containers can close immediately
                            continue;
                        }
                        Token::Comma => {
                            return Err(Error::new(ErrorKind::Unexpected, "unexpected comma"));
                        }
                        Token::Colon => {
                            return Err(Error::new(ErrorKind::Unexpected, "unexpected colon"));
                        }
                        Token::SeqEnd | Token::MapEnd => {
                            return Err(Error::new(ErrorKind::Unexpected, "expected a value"));
                        }
                    }
                }
            }

            // a value was completed
            if container == Container::Top {
                return if self.parse_whitespace().is_some() {
                    Err(Error::new(ErrorKind::Unexpected, "garbage after input"))
                } else {
                    Ok(())
                };
            }
            first = false;
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

    fn parse_str<'b>(&mut self, buffer: &'b mut Vec<u8>) -> Result<&'b str, Error>
    where
        'a: 'b,
    {
        fn result(bytes: &[u8]) -> &str {
            // SAFETY: the input is valid UTF-8 as it comes from a `&str`.  The
            // borrowed slices start and end at ASCII characters (quotes and
            // backslashes) so they are valid UTF-8 too.  The \u-escapes are
            // validated when they are decoded into the buffer.
            unsafe { str::from_utf8_unchecked(bytes) }
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
                        let borrowed = &self.input[start..self.pos];
                        self.pos += 1;
                        return Ok(result(borrowed));
                    } else {
                        buffer.extend_from_slice(&self.input[start..self.pos]);
                        self.pos += 1;
                        return Ok(result(buffer));
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

    fn parse_integer(&mut self, nonnegative: bool, first_digit: u8) -> Result<Token<'a>, Error> {
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
    ) -> Result<Token<'a>, Error> {
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
                return Ok(Token::BigInt(text));
            }
        }
        Ok(Token::F64(float))
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
                    return self.parse_decimal(nonnegative, significand, exponent);
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

    fn parse_number(&mut self, nonnegative: bool, significand: u64) -> Result<Token<'a>, Error> {
        match self.peek_or_nul() {
            b'.' => self
                .parse_decimal(nonnegative, significand, 0)
                .map(Token::F64),
            b'e' | b'E' => self
                .parse_exponent(nonnegative, significand, 0)
                .map(Token::F64),
            _ => {
                Ok(if nonnegative {
                    Token::U64(significand)
                } else {
                    let neg = (significand as i64).wrapping_neg();

                    // Values below i64::MIN are passed on as 128 bit integers.
                    if neg > 0 {
                        Token::BigInt(self.number_text(false))
                    } else {
                        Token::I64(neg)
                    }
                })
            }
        }
    }

    fn parse_decimal(
        &mut self,
        nonnegative: bool,
        mut significand: u64,
        mut exponent: i32,
    ) -> Result<f64, Error> {
        self.bump();

        let mut at_least_one_digit = false;
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
                break;
            }

            significand = significand * 10 + digit;
            exponent -= 1;
        }

        if !at_least_one_digit {
            return Err(Error::new(ErrorKind::Unexpected, "expected a digit"));
        }

        match self.peek_or_nul() {
            b'e' | b'E' => self.parse_exponent(nonnegative, significand, exponent),
            _ => f64_from_parts(nonnegative, significand, exponent),
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

    fn next_token<'b>(&mut self, buffer: &'b mut Vec<u8>) -> Result<Token<'b>, Error>
    where
        'a: 'b,
    {
        let peek = match self.parse_whitespace() {
            Some(b) => b,
            None => return Err(Error::new(ErrorKind::EndOfFile, "unexpected end of file")),
        };
        #[cfg(feature = "locations")]
        {
            self.token_start = self.pos;
        }
        self.bump();
        match peek {
            b'"' => self.parse_str(buffer).map(Token::Str),
            digit @ b'0'..=b'9' => self.parse_integer(true, digit),
            b'-' => {
                let first_digit = self.next_or_nul();
                self.parse_integer(false, first_digit)
            }
            b'{' => Ok(Token::MapStart),
            b'[' => Ok(Token::SeqStart),
            b'}' => Ok(Token::MapEnd),
            b']' => Ok(Token::SeqEnd),
            b',' => Ok(Token::Comma),
            b':' => Ok(Token::Colon),
            b'n' => {
                self.parse_ident(b"ull")?;
                Ok(Token::Null)
            }
            b't' => {
                self.parse_ident(b"rue")?;
                Ok(Token::Bool(true))
            }
            b'f' => {
                self.parse_ident(b"alse")?;
                Ok(Token::Bool(false))
            }
            _ => Err(Error::new(ErrorKind::Unexpected, "unexpected character")),
        }
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

/// Emits an integer that does not fit into 64 bits as extension value.
#[cold]
fn emit_big_int(driver: &mut DeserializeDriver, text: &str) -> Result<(), Error> {
    // the tokenizer already validated that the value fits
    if text.starts_with('-') {
        let value: i128 = text.parse().unwrap();
        driver.emit(Atom::Ext(ExtValue::borrowed(&value)))
    } else {
        let value: u128 = text.parse().unwrap();
        driver.emit(Atom::Ext(ExtValue::borrowed(&value)))
    }
}

/// Deserializes JSON from the given string.
pub fn from_str<T: Deserialize>(s: &str) -> Result<T, Error> {
    Deserializer::new(s).deserialize()
}
