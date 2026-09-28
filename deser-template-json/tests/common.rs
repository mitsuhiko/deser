//! Helpers for the tests of all dialects.
use std::io::Read;

use deser::Event;
use deser::de::Recording;

use super::dialect::{Deserializer, DeserializerConfig, Trailing};

pub const STRICT: DeserializerConfig = DeserializerConfig::new();
pub const NEWLINE: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);
pub const STOP: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Stop);

/// A reader that returns the input in chunks of a fixed size.
pub struct Chunked<'a> {
    pub input: &'a [u8],
    pub size: usize,
}

impl Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.size.min(buf.len()).min(self.input.len());
        buf[..len].copy_from_slice(&self.input[..len]);
        self.input = &self.input[len..];
        Ok(len)
    }
}

/// Returns the chunk sizes to read an input of `len` bytes in.
///
/// Miri is too slow for all sizes, it checks chunks of a byte (which split
/// the input at every position) and the whole input.  The unit tests of
/// the parser check more sizes.
pub fn chunk_sizes(len: usize) -> impl Iterator<Item = usize> {
    (1..=len).filter(move |&size| !cfg!(miri) || size == 1 || size == len)
}

/// A reader that returns its input at once and then blocks forever.
///
/// Blocking is simulated by a panic: the reader must not read while a
/// value is complete.
pub struct Blocking<'a>(pub &'a [u8]);

impl Read for Blocking<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        assert!(!self.0.is_empty(), "read would block");
        let len = buf.len().min(self.0.len());
        buf[..len].copy_from_slice(&self.0[..len]);
        self.0 = &self.0[len..];
        Ok(len)
    }
}

pub fn events(value: Recording) -> Vec<Event<'static>> {
    value.events().cloned().collect()
}

/// Reads all values of a stream in memory.
pub fn read_in_memory(config: &DeserializerConfig, input: &str) -> Vec<Vec<Event<'static>>> {
    let mut de = Deserializer::from_str_with_config(input, config);
    let mut rv = Vec::new();
    while !de.is_end() {
        rv.push(events(de.deserialize::<Recording>().unwrap()));
    }
    rv
}

/// Reads all values of a stream in chunks while their input arrives.
pub fn read_chunked(
    config: &DeserializerConfig,
    input: &str,
    size: usize,
) -> Vec<Vec<Event<'static>>> {
    let mut reader = config.reader(Chunked {
        input: input.as_bytes(),
        size,
    });
    let mut rv = Vec::new();
    while let Some(value) = reader.read::<Recording>().unwrap() {
        rv.push(events(value));
    }
    rv
}

/// Reads all values of a stream in chunks from their frames.
pub fn read_framed(
    config: &DeserializerConfig,
    input: &str,
    size: usize,
) -> Vec<Vec<Event<'static>>> {
    let mut reader = config.reader(Chunked {
        input: input.as_bytes(),
        size,
    });
    let mut rv = Vec::new();
    while let Some(value) = reader.read_borrowed::<Recording>().unwrap() {
        rv.push(events(value));
    }
    rv
}

/// Reads all values of a stream in chunks with the reader's
/// `Deserializer` implementation.
pub fn read_deserializer(
    config: &DeserializerConfig,
    input: &str,
    size: usize,
) -> Vec<Vec<Event<'static>>> {
    use deser::de::Deserializer as _;

    let mut reader = config.reader(Chunked {
        input: input.as_bytes(),
        size,
    });
    let mut rv = Vec::new();
    while !reader.is_end().unwrap() {
        rv.push(events(reader.deserialize::<Recording>().unwrap()));
    }
    rv
}

/// Checks that a stream of `count` values reads the same in memory and in
/// chunks (while the input arrives, from frames and with the reader's
/// `Deserializer` implementation).
pub fn check_stream(config: &DeserializerConfig, input: &str, count: usize) {
    let expected = read_in_memory(config, input);
    assert_eq!(expected.len(), count, "{input:?}");
    for size in chunk_sizes(input.len()) {
        assert_eq!(
            read_chunked(config, input, size),
            expected,
            "fed {input:?} size {size}"
        );
        assert_eq!(
            read_framed(config, input, size),
            expected,
            "framed {input:?} size {size}"
        );
        // miri is slow, reading with the deserializer (which peeks and
        // then feeds or frames) is only checked with the whole input
        if !cfg!(miri) || size == input.len() {
            assert_eq!(
                read_deserializer(config, input, size),
                expected,
                "deserializer {input:?} size {size}"
            );
        }
    }
}
