//! Reading and writing delimited text from and to streams.
use std::io::{Read, Write};

use deser_core::Error;
use deser_core::de::{Deserialize, DeserializeDriver, DeserializeOwned};
use deser_core::io::{Decoder, Encoder, Frame};
use deser_core::ser::{Serialize, SerializeDriver};

use crate::de::{self, Deserializer, DeserializerConfig, StreamState};
use crate::ser::{SerializerConfig, WriterState};

/// Reads the records of a stream (see [`deser::io`](deser_core::io)).
///
/// Every value of the stream is a record.  The names of the columns are
/// read before the first record and kept in the state (see
/// [`StreamState::headers`]).  Errors of records (like fields that do not
/// fit the type or records with the wrong number of fields) only discard
/// the record, reading continues with the next one.  Errors of the stream
/// (like a record that exceeds
/// [`max_record_len`](DeserializerConfig::max_record_len)) end it.
///
/// ```
/// use deser::io::Reader;
/// use deser_csv::DeserializerConfig;
///
/// #[derive(deser::Deserialize)]
/// struct Row {
///     name: String,
///     age: u32,
/// }
///
/// let mut reader = Reader::new(&b"name,age\njane,42\njohn,23\n"[..], DeserializerConfig::new());
/// let rows = reader.iter::<Row>().collect::<Result<Vec<_>, _>>().unwrap();
/// assert_eq!(rows[1].age, 23);
/// ```
///
/// Records are parsed like with a [`Deserializer`], so they can borrow
/// from the stream's buffer (see
/// [`deser::io::Reader::read_borrowed`](deser_core::io::Reader::read_borrowed)).
/// The input ranges (and thus locations) of fields refer to the start of
/// their record.
///
/// The methods that read a single value ([`from_slice`](Decoder::from_slice)
/// and [`from_reader`](Decoder::from_reader)) read all records as a
/// sequence, like [`from_str`](crate::from_str).
impl Decoder for DeserializerConfig {
    type State = StreamState;

    fn frame(&self, state: &mut StreamState, input: &[u8], eof: bool) -> Result<Frame, Error> {
        Ok(match state.frame(self, input, eof)? {
            de::Frame::Value {
                start,
                end,
                consumed,
            } => Frame::Value {
                start,
                end,
                consumed,
            },
            de::Frame::Incomplete { consumed } => Frame::Incomplete { consumed },
            de::Frame::End => Frame::End,
        })
    }

    fn drive<'de>(
        &self,
        state: &mut StreamState,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        state
            .emit_record(self, frame, 0, driver)
            .map_err(|err| err.resolve_position(frame))
    }

    fn is_text(&self) -> bool {
        true
    }

    fn from_slice_with<'de, T, F>(&self, input: &'de [u8], setup: F) -> Result<T, Error>
    where
        T: Deserialize<'de>,
        F: FnOnce(&mut DeserializeDriver<'_, 'de>),
    {
        Deserializer::from_slice_with_config(input, self).deserialize_with(setup)
    }

    fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        DeserializerConfig::from_reader(self, reader)
    }
}

/// Writes records to a stream (see [`deser::io`](deser_core::io)).
///
/// Every value is a record, the names of the columns are written before
/// the first one (see [`SerializerConfig::headers`]).  The state holds the
/// names (see [`WriterState`]), a record that fails to serialize is not
/// written.
///
/// ```
/// use deser::io::Writer;
/// use deser_csv::SerializerConfig;
///
/// #[derive(deser::Serialize)]
/// struct Row {
///     name: &'static str,
///     age: u32,
/// }
///
/// let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
/// writer.write(&Row { name: "jane", age: 42 }).unwrap();
/// writer.write(&Row { name: "john", age: 23 }).unwrap();
/// assert_eq!(writer.into_inner(), b"name,age\njane,42\njohn,23\n");
/// ```
///
/// The methods that write a single value ([`to_vec`](Encoder::to_vec) and
/// [`to_writer`](Encoder::to_writer)) write all records of a sequence,
/// like [`to_string`](crate::to_string).
impl Encoder for SerializerConfig {
    type State = WriterState;

    fn encode(
        &self,
        state: &mut WriterState,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        self.write(state, driver, false, out)
    }

    fn to_vec_with<F>(&self, value: &dyn Serialize, setup: F) -> Result<Vec<u8>, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        self.to_string_with(value, setup).map(String::into_bytes)
    }

    fn to_writer<W: Write>(&self, writer: W, value: &dyn Serialize) -> Result<(), Error> {
        SerializerConfig::to_writer(self, writer, value)
    }
}

impl DeserializerConfig {
    /// Deserializes the records of a reader.
    ///
    /// See [`from_reader`](crate::from_reader).
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, mut reader: R) -> Result<T, Error> {
        let mut input = Vec::new();
        reader.read_to_end(&mut input)?;
        self.from_slice(&input)
    }
}

impl SerializerConfig {
    /// Serializes the records of a value to a writer.
    ///
    /// See [`to_writer`](crate::to_writer).
    pub fn to_writer<W: Write>(&self, mut writer: W, value: &dyn Serialize) -> Result<(), Error> {
        let output = self.to_string(value)?;
        writer.write_all(output.as_bytes())?;
        Ok(())
    }
}

/// Deserializes the records of a reader.
///
/// This works like [`from_str`](crate::from_str): all records are
/// deserialized as a sequence.  The reader is read to the end, it does not
/// need to be buffered.  To read one record at a time use a
/// [`deser::io::Reader`](deser_core::io::Reader) with a
/// [`DeserializerConfig`].
///
/// ```
/// use std::collections::BTreeMap;
///
/// let rows: Vec<BTreeMap<String, u32>> = deser_csv::from_reader(&b"a,b\n1,2\n"[..]).unwrap();
/// assert_eq!(rows[0]["b"], 2);
/// ```
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}

/// Serializes the records of a value to a writer.
///
/// The output is written with a single write.  To write one record at a
/// time use a [`deser::io::Writer`](deser_core::io::Writer) with a
/// [`SerializerConfig`].
///
/// ```
/// let mut out = Vec::new();
/// deser_csv::to_writer(&mut out, &vec![(1, "a"), (2, "b")]).unwrap();
/// assert_eq!(out, b"1,a\n2,b\n");
/// ```
pub fn to_writer<W: Write>(writer: W, value: &dyn Serialize) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}
