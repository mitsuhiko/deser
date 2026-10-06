//! The code shared by the fuzz targets.
//!
//! Every format has a fuzz target which runs [`run`] with the format's
//! [`Format`] implementation.  The input of the fuzz targets starts with a
//! header (see [`Input`]) which configures the deserializer, the
//! serializer and how the input is split into chunks for the stream
//! deserializer, the rest is the input of the format.
//!
//! The targets check that:
//!
//! * nothing panics when the input is deserialized into dynamic values
//!   ([`Value`]), recordings and a derived type that uses most features
//!   of the derive ([`typed::Typed`]), and when the results are serialized
//!   again.
//! * the stream deserializer finds the same values as the deserializer of
//!   complete inputs, no matter how the input is split into chunks (both
//!   with values which are deserialized while their input arrives and
//!   values which are read from their frames).
//! * the output of the serializer can be deserialized again, and
//!   serializing that value gives the same output.
use std::fmt::Write as _;
use std::io::Read;

use deser::de::{DuplicateKeys, Recording};
use deser::{Context, Error, TrackLocations};
use deser_value::Value;

pub mod formats;
pub mod generate;
pub mod typed;

pub use self::formats::Format;

/// The length of the header of the inputs.
pub const HEADER_LEN: usize = 9;

/// A fuzz input.
///
/// The input starts with a header of [`HEADER_LEN`] bytes: the flags of
/// the deserializer and of the serializer (little endian `u32`s) and the
/// seed of the chunks the stream deserializer receives.  A header of
/// zeroes is the default configuration with the input in a single chunk.
pub struct Input<'a> {
    /// The configuration of the deserializer.
    ///
    /// The upper bits configure the context (see [`context`]), the lower
    /// bits the format (see [`Format::config`]).
    pub flags: u32,
    /// The configuration of the serializer (see [`Format::ser_config`]).
    pub ser_flags: u32,
    /// The seed of the chunk sizes (see [`Chunked`]).
    pub chunks: u8,
    /// The input of the format.
    pub data: &'a [u8],
}

impl<'a> Input<'a> {
    /// Splits the header off a fuzz input.
    pub fn parse(raw: &'a [u8]) -> Option<Input<'a>> {
        let (header, data) = raw.split_first_chunk::<HEADER_LEN>()?;
        Some(Input {
            flags: u32::from_le_bytes([header[0], header[1], header[2], header[3]]),
            ser_flags: u32::from_le_bytes([header[4], header[5], header[6], header[7]]),
            chunks: header[8],
            data,
        })
    }
}

/// Creates the context of the deserializer from the upper bits of the
/// flags.
pub fn context(flags: u32) -> Context {
    let mut context = Context::default();
    if flags & (1 << 31) != 0 {
        context.set(TrackLocations(true));
    }
    match (flags >> 29) & 3 {
        1 => context.set(DuplicateKeys::Last),
        2 => context.set(DuplicateKeys::First),
        _ => {}
    }
    context
}

/// A reader which returns its data in chunks of pseudo random sizes.
///
/// With a seed of zero the data is returned in one piece.
pub struct Chunked<'a> {
    data: &'a [u8],
    max: usize,
    state: u32,
}

impl<'a> Chunked<'a> {
    pub fn new(data: &'a [u8], seed: u8) -> Chunked<'a> {
        Chunked {
            data,
            // chunks of up to 1 to 128 bytes
            max: if seed == 0 {
                usize::MAX
            } else {
                1 << (seed & 7)
            },
            state: u32::from(seed) | 1 << 16,
        }
    }
}

impl Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // xorshift
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        let size = match self.max {
            usize::MAX => self.data.len(),
            max => 1 + self.state as usize % max,
        };
        let len = size.min(buf.len()).min(self.data.len());
        buf[..len].copy_from_slice(&self.data[..len]);
        self.data = &self.data[len..];
        Ok(len)
    }
}

/// The values of a stream up to its first error.
#[derive(Default)]
pub struct Values {
    /// The values (as debug output of their recordings).
    pub values: Vec<String>,
    /// The error that ended the stream.
    pub error: Option<Error>,
}

impl Values {
    /// Collects values until the first error.
    pub fn collect(iter: impl IntoIterator<Item = Result<Recording, Error>>) -> Values {
        let mut rv = Values::default();
        for item in iter {
            if !rv.push(item.map(Some)) {
                break;
            }
        }
        rv
    }

    /// Adds the result of reading a value.
    ///
    /// Returns `false` if there are no more values.
    pub fn push(&mut self, item: Result<Option<Recording>, Error>) -> bool {
        match item {
            Ok(Some(value)) => {
                // only the events, not where they are in the input
                let mut events = String::new();
                for event in value.events() {
                    write!(events, "{event:?} ").unwrap();
                }
                self.values.push(events);
                true
            }
            Ok(None) => false,
            Err(err) => {
                self.error = Some(err);
                false
            }
        }
    }
}

impl std::fmt::Debug for Values {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();
        for value in &self.values {
            writeln!(s, "  {value}")?;
        }
        if let Some(ref err) = self.error {
            writeln!(s, "  error: {err}")?;
        }
        f.write_str(&s)
    }
}

/// Reads the values of a stream with the format's stream deserializer.
///
/// With `framed` the values are read from their frames, otherwise they
/// are deserialized while their input arrives (if the format supports it).
pub fn read_stream<F: Format>(config: &F::Config, data: &[u8], seed: u8, framed: bool) -> Values {
    let mut reader = deser::io::Reader::new(Chunked::new(data, seed), F::stream(config));
    let mut rv = Values::default();
    loop {
        let item = if framed {
            reader.read_borrowed::<Recording>()
        } else {
            reader.read::<Recording>()
        };
        if !rv.push(item) {
            break;
        }
        // every value needs at least a byte (but formats like YAML have
        // empty documents)
        assert!(rv.values.len() <= data.len() + 1, "the stream does not end");
    }
    rv
}

/// Checks that the stream deserializer finds the values the deserializer
/// of complete inputs finds.
///
/// If the input is invalid, the stream deserializer can find more values
/// before it notices (for instance a value which is followed by garbage)
/// or fewer (if it splits the input differently).  The values that both
/// find have to be the same.
pub fn check_stream<F: Format>(config: &F::Config, input: &Input<'_>) {
    let expected = F::values(config, input.data);
    for framed in [false, true] {
        let actual = read_stream::<F>(config, input.data, input.chunks, framed);
        let same = if expected.error.is_some() {
            actual.error.is_some()
                && (actual.values.starts_with(&expected.values)
                    || expected.values.starts_with(&actual.values))
        } else {
            actual.error.is_none() && actual.values == expected.values
        };
        if !same {
            panic!(
                "the stream deserializer (framed: {framed}) found other values than the \
                 deserializer\ninput: {:?}\ndeserializer:\n{expected:?}stream:\n{actual:?}",
                Escaped(input.data)
            );
        }
    }
}

/// Checks that the output of the serializer can be deserialized again and
/// that values survive round trips.
///
/// The serializer can fail (formats cannot express everything, and some
/// cannot even express everything their deserializer produces, like CSV
/// with duplicate columns).  If it does not, its output has to be
/// deserializable.  The first round trip can change the value as it does
/// not need to be a value of the format and configuration: types that the
/// format does not have are converted and different keys can become the
/// same (the integer `1` and the string `"1"` are the same key in JSON,
/// the last one is kept).  After that the value has to be stable: it
/// deserializes to the same value and serializes to the same output
/// again.
pub fn check_roundtrip<F: Format>(ser_flags: u32, value: &Value) {
    let (ser, de) = F::ser_config(ser_flags, Context::with(DuplicateKeys::Last));
    let Ok(output) = F::serialize(&ser, value) else {
        return;
    };
    let first = reparse::<F>(&de, &output);

    let (ser, de) = F::ser_config(ser_flags, Context::default());
    let Ok(output) = F::serialize(&ser, &first) else {
        return;
    };
    let second = reparse::<F>(&de, &output);
    if first != second {
        panic!(
            "the value changed after a round trip\noutput: {:?}\nbefore: {first:?}\nafter:  {second:?}",
            Escaped(&output)
        );
    }
    let output2 = F::serialize(&ser, &second).unwrap_or_else(|err| {
        panic!(
            "cannot serialize a value that was deserialized from the output: {err}\n\
             output: {:?}\nvalue: {second:?}",
            Escaped(&output)
        )
    });
    if output != output2 {
        panic!(
            "the value serializes differently after a round trip\nbefore: {:?}\nafter:  {:?}",
            Escaped(&output),
            Escaped(&output2)
        );
    }
}

/// Deserializes the output of a serializer.
pub fn reparse<F: Format>(config: &F::Config, output: &[u8]) -> Value {
    F::from_slice(config, output).unwrap_or_else(|err| {
        panic!(
            "cannot deserialize the output of the serializer: {err}\noutput: {:?}",
            Escaped(output)
        )
    })
}

/// Runs the checks of a format with a fuzz input.
pub fn run<F: Format>(raw: &[u8]) {
    let Some(input) = Input::parse(raw) else {
        return;
    };
    let config = F::config(input.flags, context(input.flags));
    let (ser, _) = F::ser_config(input.ser_flags, Context::default());

    if let Ok(typed) = F::from_slice::<typed::Typed>(&config, input.data) {
        let _ = F::serialize(&ser, &typed);
    }
    if let Ok(recording) = F::from_slice::<Recording>(&config, input.data) {
        let _ = F::serialize(&ser, &recording);
    }
    check_stream::<F>(&config, &input);
    if let Ok(value) = F::from_slice::<Value>(&config, input.data) {
        check_roundtrip::<F>(input.ser_flags, &value);
    }
}

/// Formats bytes as string (if they are UTF-8) or as bytes.
pub struct Escaped<'a>(pub &'a [u8]);

impl std::fmt::Debug for Escaped<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match std::str::from_utf8(self.0) {
            Ok(s) => write!(f, "{s:?}"),
            Err(_) => write!(f, "b\"{}\"", self.0.escape_ascii()),
        }
    }
}
