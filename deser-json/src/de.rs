use std::str;

use deser::de::{Deserialize, DeserializeDriver};
use deser::Atom;
use deser::Event;
use deser::{Error, ErrorKind};

enum Token<'a> {
    Null,
    Bool(bool),
    Str(&'a str),
    I64(i64),
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
}

enum ContainerState {
    Map { first: bool, key_pos: bool },
    Seq { first: bool },
}

impl<'a> Deserializer<'a> {
    /// Creates a new deserializer.
    pub fn new(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer {
            input,
            pos: 0,
            buffer: Vec::new(),
        }
    }

    /// Deserializes the value.
    pub fn deserialize<T: Deserialize>(&mut self) -> Result<T, Error> {
        let mut out = None;
        self.deserialize_into(&mut out)?;
        out.take()
            .ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))
    }

    fn deserialize_into<T: Deserialize>(&mut self, out: &mut Option<T>) -> Result<(), Error> {
        let mut token = self.next_token()?;
        let mut driver = DeserializeDriver::new(out);
        let mut stack = vec![];

        loop {
            // try to exit containers first
            match token {
                Token::MapEnd => {
                    if !matches!(stack.pop(), Some(ContainerState::Map { .. })) {
                        return Err(Error::new(ErrorKind::Unexpected, "unexpected end of map"));
                    }
                    driver.emit(Event::MapEnd)?;
                }
                Token::SeqEnd => {
                    if !matches!(stack.pop(), Some(ContainerState::Seq { .. })) {
                        return Err(Error::new(ErrorKind::Unexpected, "unexpected end of seq"));
                    }
                    driver.emit(Event::SeqEnd)?;
                }
                _ => {
                    // do we need a comma?
                    if let Some(
                        ContainerState::Seq { first: false }
                        | ContainerState::Map {
                            first: false,
                            key_pos: true,
                        },
                    ) = stack.last_mut()
                    {
                        if !matches!(token, Token::Comma) {
                            return Err(Error::new(ErrorKind::Unexpected, "expected a comma"));
                        }
                        token = self.next_token()?;
                    }

                    // handle keys
                    if let Some(ContainerState::Map {
                        first,
                        key_pos: key_pos @ true,
                    }) = stack.last_mut()
                    {
                        match token {
                            Token::Str(val) => driver.emit(Event::from(val))?,
                            _ => return Err(Error::new(ErrorKind::Unexpected, "expected map key")),
                        }
                        match self.next_token()? {
                            Token::Colon => {}
                            _ => return Err(Error::new(ErrorKind::Unexpected, "expected colon")),
                        }
                        token = self.next_token()?;
                        *first = false;
                        *key_pos = false;
                        continue;
                    }

                    match token {
                        Token::Null => driver.emit(Event::Atom(Atom::Null))?,
                        Token::Bool(val) => driver.emit(Event::from(val))?,
                        Token::Str(val) => driver.emit(Event::from(val))?,
                        Token::I64(val) => driver.emit(Event::from(val))?,
                        Token::U64(val) => driver.emit(Event::from(val))?,
                        Token::F64(val) => driver.emit(Event::from(val))?,
                        Token::MapStart => {
                            stack.push(ContainerState::Map {
                                first: true,
                                key_pos: true,
                            });
                            driver.emit(Event::MapStart)?;
                            token = self.next_token()?;
                            continue;
                        }
                        Token::SeqStart => {
                            stack.push(ContainerState::Seq { first: true });
                            driver.emit(Event::SeqStart)?;
                            token = self.next_token()?;
                            continue;
                        }
                        Token::Comma => {
                            return Err(Error::new(ErrorKind::Unexpected, "unexpected comma"));
                        }
                        Token::Colon => {
                            return Err(Error::new(ErrorKind::Unexpected, "unexpected colon"));
                        }
                        Token::SeqEnd | Token::MapEnd => unreachable!(),
                    }
                }
            }

            match stack.last_mut() {
                None => {
                    return if self.parse_whitespace().is_some() {
                        Err(Error::new(ErrorKind::Unexpected, "garbage after input"))
                    } else {
                        Ok(())
                    }
                }
                Some(ContainerState::Map { first, key_pos }) => {
                    token = self.next_token()?;
                    *key_pos = true;
                    *first = false;
                }
                Some(ContainerState::Seq { first }) => {
                    token = self.next_token()?;
                    *first = false;
                }
            }
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

    fn parse_str(&mut self) -> Result<&str, Error> {
        fn result(bytes: &[u8]) -> &str {
            // The input is assumed to be valid UTF-8 and the \u-escapes are
            // checked along the way, so don't need to check here.
            unsafe { str::from_utf8_unchecked(bytes) }
        }

        // Index of the first byte not yet copied into the scratch space.
        let mut start = self.pos;
        self.buffer.clear();

        loop {
            while self.pos < self.input.len() && !ESCAPE[usize::from(self.input[self.pos])] {
                self.pos += 1;
            }
            if self.pos == self.input.len() {
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    "unexpected end of string",
                ));
            }
            match self.input[self.pos] {
                b'"' => {
                    if self.buffer.is_empty() {
                        // Fast path: return a slice of the raw JSON without any
                        // copying.
                        let borrowed = &self.input[start..self.pos];
                        self.pos += 1;
                        return Ok(result(borrowed));
                    } else {
                        self.buffer.extend_from_slice(&self.input[start..self.pos]);
                        self.pos += 1;
                        return Ok(result(&self.buffer));
                    }
                }
                b'\\' => {
                    self.buffer.extend_from_slice(&self.input[start..self.pos]);
                    self.pos += 1;
                    self.parse_escape()?;
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
    fn parse_escape(&mut self) -> Result<(), Error> {
        let ch = self.next_or_eof()?;

        match ch {
            b'"' => self.buffer.push(b'"'),
            b'\\' => self.buffer.push(b'\\'),
            b'/' => self.buffer.push(b'/'),
            b'b' => self.buffer.push(b'\x08'),
            b'f' => self.buffer.push(b'\x0c'),
            b'n' => self.buffer.push(b'\n'),
            b'r' => self.buffer.push(b'\r'),
            b't' => self.buffer.push(b'\t'),
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

                        if n2 < 0xDC00 || n2 > 0xDFFF {
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

                self.buffer
                    .extend_from_slice(c.encode_utf8(&mut [0_u8; 4]).as_bytes());
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

    fn parse_whitespace(&mut self) -> Option<u8> {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\n') | Some(b'\t') | Some(b'\r') => {
                    self.bump();
                }
                other => {
                    return other;
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

    fn parse_integer(&mut self, nonnegative: bool, first_digit: u8) -> Result<Token, Error> {
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
                            if overflow!(res * 10 + digit, u64::max_value()) {
                                return self
                                    .parse_long_integer(
                                        nonnegative,
                                        res,
                                        1, // res * 10^1
                                    )
                                    .map(Token::F64);
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

    fn parse_number(&mut self, nonnegative: bool, significand: u64) -> Result<Token, Error> {
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

                    // Convert into a float if we underflow.
                    if neg > 0 {
                        Token::F64(-(significand as f64))
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

            if overflow!(significand * 10 + digit, u64::max_value()) {
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

            if overflow!(exp * 10 + digit, i32::max_value()) {
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

    fn next_token(&mut self) -> Result<Token, Error> {
        let peek = match self.parse_whitespace() {
            Some(b) => b,
            None => return Err(Error::new(ErrorKind::EndOfFile, "unexpected end of file")),
        };
        self.bump();
        match peek {
            b'"' => self.parse_str().map(Token::Str),
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
        match POW10.get(exponent.abs() as usize) {
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

const CT: bool = true; // control character \x00..=\x1F
const QU: bool = true; // quote \x22
const BS: bool = true; // backslash \x5C
const O: bool = false; // allow unescaped

// Lookup table of bytes that must be escaped. A value of true at index i means
// that byte i requires an escape sequence in the input.
#[rustfmt::skip]
static ESCAPE: [bool; 256] = [
    //   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
    CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, // 0
    CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, CT, // 1
     O,  O, QU,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 2
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 3
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 4
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, BS,  O,  O,  O, // 5
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 6
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 7
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 8
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // 9
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // A
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // B
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // C
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // D
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // E
     O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O,  O, // F
];

/// Deserializes JSON from the given string.
pub fn from_str<T: Deserialize>(s: &str) -> Result<T, Error> {
    Deserializer::new(s.as_bytes()).deserialize()
}
