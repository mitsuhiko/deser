use std::io::{self, Write};

use deser::ser::SerializeDriver;
use deser::{Atom, Error, ErrorKind, Event, Serialize};

/// Serializes a serializable to JSON.
pub struct Serializer<W: Write> {
    writer: W,
}

enum ContainerState {
    Map { first: bool, key_pos: bool },
    Seq { first: bool },
}

impl<W: Write> Serializer<W> {
    pub fn new(writer: W) -> Serializer<W> {
        Serializer { writer }
    }

    pub fn serialize(&mut self, value: &dyn Serialize) -> Result<(), Error> {
        let mut driver = SerializeDriver::new(value);
        let mut container_stack = Vec::new();

        macro_rules! unsupported {
            ($msg:expr) => {{
                return Err(Error::new(ErrorKind::UnsupportedType, $msg));
            }};
        }

        macro_rules! try_io {
            ($expr:expr) => {
                $expr.map_err(|err| {
                    Error::new(ErrorKind::Unexpected, "IO error while serializing").with_source(err)
                })?
            };
        }

        while let Some((event, _, _)) = driver.next()? {
            // try to exit containers first
            match event {
                Event::MapEnd => {
                    container_stack.pop();
                    try_io!(self.write_byte(b'}'));
                    continue;
                }
                Event::SeqEnd => {
                    container_stack.pop();
                    try_io!(self.write_byte(b']'));
                    continue;
                }
                _ => {}
            }

            // do we need a comma?
            if let Some(
                ContainerState::Seq { first }
                | ContainerState::Map {
                    first,
                    key_pos: true,
                },
            ) = container_stack.last_mut()
            {
                if !*first {
                    try_io!(self.write_byte(b','));
                }
                *first = false;
            }

            // keys need special handling
            if let Some(ContainerState::Map { key_pos, .. }) = container_stack.last_mut() {
                let is_key = *key_pos;
                *key_pos = !*key_pos;
                if is_key {
                    match event {
                        Event::Atom(Atom::Str(val)) => {
                            try_io!(self.write_escaped_str(&val))
                        }
                        Event::Atom(Atom::Char(c)) => {
                            try_io!(self.write_escaped_str(&(c as u32).to_string()))
                        }
                        Event::Atom(Atom::U64(val)) => {
                            try_io!(self.write_byte(b'"'));
                            try_io!(self.write_str(&val.to_string()));
                            try_io!(self.write_byte(b'"'));
                        }
                        Event::Atom(Atom::I64(val)) => {
                            try_io!(self.write_byte(b'"'));
                            try_io!(self.write_str(&val.to_string()));
                            try_io!(self.write_byte(b'"'));
                        }
                        _ => unsupported!("JSON does not support this value for map keys"),
                    }
                    try_io!(self.write_byte(b':'));
                    continue;
                }
            }

            try_io!(match event {
                Event::Atom(atom) => match atom {
                    Atom::Null => self.write_str("null"),
                    Atom::Bool(true) => self.write_str("true"),
                    Atom::Bool(false) => self.write_str("false"),
                    Atom::Str(val) => self.write_escaped_str(&val),
                    Atom::Bytes(_val) => unsupported!("JSON doesn't support bytes"),
                    Atom::Char(c) => self.write_escaped_str(&(c as u32).to_string()),
                    Atom::U64(val) => self.write_str(&val.to_string()),
                    Atom::I64(val) => self.write_str(&val.to_string()),
                    Atom::F64(val) => {
                        if val.is_finite() {
                            self.write_str(val.to_string().as_str())
                        } else {
                            self.write_str("null")
                        }
                    }
                    _ => unsupported!("unknown atom"),
                },
                Event::MapStart => {
                    container_stack.push(ContainerState::Map {
                        first: true,
                        key_pos: true,
                    });
                    self.write_byte(b'{')
                }
                Event::SeqStart => {
                    container_stack.push(ContainerState::Seq { first: true });
                    self.write_byte(b'[')
                }
                Event::SeqEnd | Event::MapEnd => unreachable!(),
            })
        }

        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)
    }

    fn write_str(&mut self, s: &str) -> io::Result<()> {
        self.write(s.as_bytes())
    }

    fn write_byte(&mut self, c: u8) -> io::Result<()> {
        self.writer.write_all(&[c])
    }

    fn write_escaped_str(&mut self, value: &str) -> io::Result<()> {
        self.write_byte(b'"')?;

        let bytes = value.as_bytes();
        let mut start = 0;

        for (i, &byte) in bytes.iter().enumerate() {
            let escape = ESCAPE[byte as usize];
            if escape == 0 {
                continue;
            }

            if start < i {
                self.write_str(&value[start..i])?;
            }

            match escape {
                self::BB => self.write_str("\\b")?,
                self::TT => self.write_str("\\t")?,
                self::NN => self.write_str("\\n")?,
                self::FF => self.write_str("\\f")?,
                self::RR => self.write_str("\\r")?,
                self::QU => self.write_str("\\\"")?,
                self::BS => self.write_str("\\\\")?,
                self::U => {
                    static HEX_DIGITS: [u8; 16] = *b"0123456789abcdef";
                    self.write_str("\\u00")?;
                    self.write_byte(HEX_DIGITS[(byte >> 4) as usize])?;
                    self.write_byte(HEX_DIGITS[(byte & 0xF) as usize])?;
                }
                _ => unreachable!(),
            }

            start = i + 1;
        }

        if start != bytes.len() {
            self.write_str(&value[start..])?;
        }

        self.write_byte(b'"')?;

        Ok(())
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
    let mut rv: Vec<u8> = Vec::new();
    let mut serializer = Serializer::new(&mut rv);
    serializer.serialize(value)?;
    Ok(unsafe { String::from_utf8_unchecked(rv) })
}
