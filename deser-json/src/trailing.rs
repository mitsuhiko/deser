//! What may follow a value.

/// Controls what may follow a value.
///
/// See [`DeserializerConfig::trailing`](crate::DeserializerConfig::trailing).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Trailing {
    /// Only whitespace may follow the value.
    ///
    /// This is the default.  Anything else after the value is an error.
    #[default]
    Strict,
    /// Every value is on a line of its own ([JSON
    /// Lines](https://jsonlines.org/), also known as NDJSON).
    ///
    /// Only whitespace may follow a value on its line, the next line holds
    /// the next value.  Lines that only contain whitespace are skipped.
    /// Errors are contained to their line: if a line fails to deserialize
    /// (even if it's malformed) the next call to
    /// [`Deserializer::deserialize`](crate::Deserializer::deserialize) continues with the next line.
    Newline,
    /// Parsing stops after the value, regardless of what follows.
    ///
    /// The data after the value is not looked at.  The next call to
    /// [`Deserializer::deserialize`](crate::Deserializer::deserialize) continues after the value (see
    /// [`Deserializer::offset`](crate::Deserializer::offset)), which also reads concatenated JSON.
    Stop,
}
