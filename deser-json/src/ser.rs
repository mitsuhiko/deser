use deser::ext::ExtValue;
use deser::ser::SerializeDriver;
use deser::{Atom, Descriptor, Error, ErrorKind, Event, Serialize};

use crate::scan::skip_to_escape;

/// Serializes a serializable to JSON.
pub struct Serializer {
    out: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    Top,
    Seq,
    Map,
}

impl Default for Serializer {
    fn default() -> Serializer {
        Serializer::new()
    }
}

impl Serializer {
    /// Creates a new serializer that writes into the given writer.
    pub fn new() -> Serializer {
        Serializer {
            out: String::with_capacity(128),
        }
    }

    /// Serializes the given value.
    pub fn serialize(mut self, value: &dyn Serialize) -> Result<String, Error> {
        let mut driver = SerializeDriver::new(value);

        // the state of the current container is held in locals, the state
        // of the outer containers is saved on the stack.
        let mut stack = Vec::new();
        let mut container = Container::Top;
        let mut first = true;
        let mut is_key = false;

        macro_rules! unsupported {
            ($msg:expr) => {{
                return Err(Error::new(ErrorKind::UnsupportedType, $msg));
            }};
        }

        while let Some((event, descriptor, _)) = driver.next()? {
            let atom = match event {
                Event::Atom(atom) => atom,
                Event::MapStart | Event::SeqStart if is_key => {
                    unsupported!("JSON does not support this value for map keys")
                }
                Event::MapStart | Event::SeqStart => {
                    if container == Container::Seq && !first {
                        self.write_char(',');
                    }
                    stack.push(container);
                    first = true;
                    if let Event::MapStart = event {
                        container = Container::Map;
                        is_key = true;
                        self.write_char('{');
                    } else {
                        container = Container::Seq;
                        is_key = false;
                        self.write_char('[');
                    }
                    continue;
                }
                Event::MapEnd | Event::SeqEnd => {
                    if event == Event::MapEnd {
                        if container != Container::Map || !is_key {
                            return Err(Error::new(ErrorKind::Unexpected, "unexpected map end"));
                        }
                        self.write_char('}');
                    } else {
                        if container != Container::Seq {
                            return Err(Error::new(ErrorKind::Unexpected, "unexpected array end"));
                        }
                        self.write_char(']');
                    }
                    container = stack.pop().unwrap_or(Container::Top);
                    // a container is never a key, so after it the next item
                    // in a map is a key again.
                    first = false;
                    is_key = container == Container::Map;
                    continue;
                }
            };

            if is_key {
                if !first {
                    self.write_char(',');
                }
                first = false;
                is_key = false;
                match atom {
                    Atom::Str(val) => self.write_escaped_str(&val),
                    Atom::Char(c) => self.write_escaped_str(c.encode_utf8(&mut [0u8; 4])),
                    Atom::U64(val) => {
                        self.write_char('"');
                        self.write_u64(val);
                        self.write_char('"');
                    }
                    Atom::I64(val) => {
                        self.write_char('"');
                        self.write_i64(val);
                        self.write_char('"');
                    }
                    Atom::Ext(ext) => self.write_ext_key(&ext)?,
                    _ => unsupported!("JSON does not support this value for map keys"),
                }
                self.write_char(':');
                continue;
            }

            match container {
                Container::Seq => {
                    if !first {
                        self.write_char(',');
                    }
                    first = false;
                }
                Container::Map => is_key = true,
                Container::Top => {}
            }

            match atom {
                Atom::Null => self.write_str("null"),
                Atom::Bool(true) => self.write_str("true"),
                Atom::Bool(false) => self.write_str("false"),
                Atom::Str(val) => self.write_escaped_str(&val),
                Atom::Bytes(_val) => unsupported!("JSON doesn't support bytes"),
                Atom::Char(c) => self.write_escaped_str(c.encode_utf8(&mut [0u8; 4])),
                Atom::U64(val) => self.write_u64(val),
                Atom::I64(val) => self.write_i64(val),
                Atom::F64(val) => self.write_float(val, descriptor),
                Atom::Ext(ext) => self.write_ext_value(&ext)?,
                _ => unsupported!("unknown atom"),
            }
        }

        Ok(self.out)
    }

    fn write_str(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn write_char(&mut self, c: char) {
        self.out.push(c);
    }

    /// Writes a float atom.
    ///
    /// Floats are widened to f64 in the data model, the descriptor tells us
    /// the original precision.
    #[inline(never)]
    fn write_float(&mut self, val: f64, descriptor: &dyn Descriptor) {
        if descriptor.precision() == Some(32) {
            self.write_f32(val as f32);
        } else {
            self.write_f64(val);
        }
    }

    fn write_f32(&mut self, val: f32) {
        if val.is_finite() {
            #[cfg(feature = "speedups")]
            {
                self.write_str(ryu::Buffer::new().format_finite(val))
            }
            #[cfg(not(feature = "speedups"))]
            {
                self.write_str(val.to_string().as_str())
            }
        } else {
            self.write_str("null")
        }
    }

    fn write_f64(&mut self, val: f64) {
        if val.is_finite() {
            #[cfg(feature = "speedups")]
            {
                self.write_str(ryu::Buffer::new().format_finite(val))
            }
            #[cfg(not(feature = "speedups"))]
            {
                self.write_str(val.to_string().as_str())
            }
        } else {
            self.write_str("null")
        }
    }

    /// Writes an extension value as map key.
    ///
    /// Extension values that JSON does not natively support are written in
    /// their fallback representation.
    #[cold]
    fn write_ext_key(&mut self, ext: &ExtValue) -> Result<(), Error> {
        if ext.is::<u128>() || ext.is::<i128>() {
            self.write_char('"');
            self.write_ext_value(ext)?;
            self.write_char('"');
            return Ok(());
        }
        match ext.fallback() {
            Atom::Str(val) => self.write_escaped_str(&val),
            Atom::Char(c) => self.write_escaped_str(c.encode_utf8(&mut [0u8; 4])),
            Atom::U64(val) => {
                self.write_char('"');
                self.write_u64(val);
                self.write_char('"');
            }
            Atom::I64(val) => {
                self.write_char('"');
                self.write_i64(val);
                self.write_char('"');
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "JSON does not support this value for map keys",
                ))
            }
        }
        Ok(())
    }

    /// Writes an extension value.
    ///
    /// Extension values that JSON does not natively support are written in
    /// their fallback representation.
    #[cold]
    fn write_ext_value(&mut self, ext: &ExtValue) -> Result<(), Error> {
        // JSON numbers have arbitrary precision, so wide integers can be
        // written natively.
        if let Some(&val) = ext.downcast_ref::<u128>() {
            self.write_int(val);
            return Ok(());
        } else if let Some(&val) = ext.downcast_ref::<i128>() {
            self.write_int(val);
            return Ok(());
        }
        match ext.fallback() {
            Atom::Null => self.write_str("null"),
            Atom::Bool(val) => self.write_str(if val { "true" } else { "false" }),
            Atom::Str(val) => self.write_escaped_str(&val),
            Atom::Char(c) => self.write_escaped_str(c.encode_utf8(&mut [0u8; 4])),
            Atom::U64(val) => self.write_u64(val),
            Atom::I64(val) => self.write_i64(val),
            Atom::F64(val) => self.write_f64(val),
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "JSON does not support this value",
                ))
            }
        }
        Ok(())
    }

    #[cfg(feature = "speedups")]
    fn write_int<I: itoa::Integer>(&mut self, val: I) {
        self.write_str(itoa::Buffer::new().format(val))
    }

    #[cfg(not(feature = "speedups"))]
    fn write_int<I: std::fmt::Display>(&mut self, val: I) {
        self.write_str(&val.to_string())
    }

    fn write_u64(&mut self, val: u64) {
        #[cfg(feature = "speedups")]
        {
            self.write_str(itoa::Buffer::new().format(val))
        }
        #[cfg(not(feature = "speedups"))]
        {
            self.write_str(&val.to_string())
        }
    }

    fn write_i64(&mut self, val: i64) {
        #[cfg(feature = "speedups")]
        {
            self.write_str(itoa::Buffer::new().format(val))
        }
        #[cfg(not(feature = "speedups"))]
        {
            self.write_str(&val.to_string())
        }
    }

    fn write_escaped_str(&mut self, value: &str) {
        self.write_char('"');

        let bytes = value.as_bytes();
        let mut start = 0;

        loop {
            let next = skip_to_escape(bytes, start);
            if start < next {
                self.write_str(&value[start..next]);
            }
            if next == bytes.len() {
                break;
            }

            let byte = bytes[next];
            match ESCAPE[byte as usize] {
                self::BB => self.write_str("\\b"),
                self::TT => self.write_str("\\t"),
                self::NN => self.write_str("\\n"),
                self::FF => self.write_str("\\f"),
                self::RR => self.write_str("\\r"),
                self::QU => self.write_str("\\\""),
                self::BS => self.write_str("\\\\"),
                self::U => {
                    static HEX_DIGITS: [u8; 16] = *b"0123456789abcdef";
                    self.write_str("\\u00");
                    self.write_char(HEX_DIGITS[(byte >> 4) as usize] as char);
                    self.write_char(HEX_DIGITS[(byte & 0xF) as usize] as char);
                }
                _ => unreachable!(),
            }

            start = next + 1;
        }

        self.write_char('"');
    }
}

const BB: u8 = b'b'; // \x08
const TT: u8 = b't'; // \x09
const NN: u8 = b'n'; // \x0A
const FF: u8 = b'f'; // \x0C
const RR: u8 = b'r'; // \x0D
const QU: u8 = b'"'; // \x22
const BS: u8 = b'\\'; // \x5C
const U: u8 = b'u'; // \x00...\x1F except the ones above

// Lookup table of escape sequences. A value of b'x' at index i means that byte
// i is escaped as "\x" in JSON. A value of 0 means that byte i is not escaped.
#[rustfmt::skip]
static ESCAPE: [u8; 256] = [
    //  1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
    U,  U,  U,  U,  U,  U,  U,  U, BB, TT, NN,  U, FF, RR,  U,  U, // 0
    U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U,  U, // 1
    0,  0, QU,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 2
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 3
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 4
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, BS,  0,  0,  0, // 5
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 6
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 7
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 8
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // 9
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // A
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // B
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // C
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // D
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // E
    0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0,  0, // F
];

/// Serializes a value to JSON.
pub fn to_string(value: &dyn Serialize) -> Result<String, Error> {
    Serializer::new().serialize(value)
}
