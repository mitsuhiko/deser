//! Writing JSON streams.
use std::io::Write;

use deser_core::Error;
use deser_core::io::Encoder;
use deser_core::ser::{Serialize, SerializeDriver};

use crate::ser::SerializerConfig;

/// Writes JSON values to a stream (see [`deser::io`](deser_core::io)).
///
/// What follows the values depends on [`SerializerConfig::trailing`].
impl Encoder for SerializerConfig {
    /// The number of values written.
    type State = usize;

    fn encode(
        &self,
        written: &mut usize,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        self.encode_value(driver, *written, out)?;
        *written += 1;
        Ok(())
    }
}

impl SerializerConfig {
    /// Serializes a value to a writer.
    ///
    /// See [`to_writer`](crate::to_writer).
    pub fn to_writer<W: Write>(&self, writer: W, value: &dyn Serialize) -> Result<(), Error> {
        deser_core::io::to_writer(writer, self, value)
    }
}

/// Serializes a value to a writer.
///
/// The value is written with a single write.
///
/// ```
/// let mut out = Vec::new();
/// deser_json::to_writer(&mut out, &vec![1, 2, 3]).unwrap();
/// assert_eq!(out, b"[1,2,3]");
/// ```
pub fn to_writer<W: Write>(writer: W, value: &dyn Serialize) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}
