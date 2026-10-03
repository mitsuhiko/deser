//! Reading delimited text from streams.
#[cfg(feature = "io")]
use std::io::Read;

use alloc::string::String;
use deser_core::Error;
#[cfg(feature = "io")]
use deser_core::de::DeserializeOwned;
use deser_core::de::{self, DeserializeDriver, Frame};

use crate::de::{DeserializerConfig, StreamState};

/// Reads the records of a stream (see [`deser::stream`](deser_core::stream)).
///
/// Every value of the stream is a record.  The names of the columns are
/// read before the first record (see [`headers`](Self::headers)).  Errors
/// of records (like fields that do not fit the type or records with the
/// wrong number of fields) only discard the record, reading continues with
/// the next one.  Errors of the stream (like a record that exceeds
/// [`set_max_record_len`](DeserializerConfig::set_max_record_len)) end it.
///
/// ```
/// # #[cfg(feature = "io")] {
/// use deser_csv::DeserializerConfig;
///
/// #[derive(deser::Deserialize)]
/// struct Row {
///     name: String,
///     age: u32,
/// }
///
/// let mut reader =
///     DeserializerConfig::new().reader(&b"name,age\njane,42\njohn,23\n"[..]);
/// let rows = reader.iter::<Row>().collect::<Result<Vec<_>, _>>().unwrap();
/// assert_eq!(rows[1].age, 23);
/// # }
/// ```
///
/// Records are parsed like with a [`Deserializer`](crate::Deserializer), so
/// they can borrow from the stream's buffer (see
/// [`InputBuffer::deserialize`](deser_core::stream::InputBuffer::deserialize)).
/// The input ranges (and thus locations) of fields refer to the start of
/// their record.
#[derive(Debug)]
pub struct StreamDeserializer {
    config: DeserializerConfig,
    state: StreamState,
}

impl Default for StreamDeserializer {
    fn default() -> StreamDeserializer {
        StreamDeserializer::new()
    }
}

impl StreamDeserializer {
    /// Creates a stream deserializer.
    pub fn new() -> StreamDeserializer {
        StreamDeserializer::with_config(DeserializerConfig::new())
    }

    /// Creates a stream deserializer with the given configuration.
    pub fn with_config(config: DeserializerConfig) -> StreamDeserializer {
        StreamDeserializer {
            config,
            state: StreamState::default(),
        }
    }

    /// Creates a stream deserializer for a stream which continues with the
    /// given names of the columns.
    ///
    /// The stream does not start with names (they are not read from the
    /// first record), for instance because it's the second half of a file:
    ///
    /// ```
    /// # #[cfg(feature = "io")] {
    /// use deser::io::Reader;
    /// use deser_csv::{DeserializerConfig, StreamDeserializer};
    ///
    /// #[derive(deser::Deserialize)]
    /// struct Row {
    ///     name: String,
    ///     age: u32,
    /// }
    ///
    /// let de = StreamDeserializer::with_headers(DeserializerConfig::new(), ["name", "age"]);
    /// let mut reader = Reader::new(&b"jane,42\n"[..], de);
    /// let row: Row = reader.read().unwrap().unwrap();
    /// assert_eq!((row.name.as_str(), row.age), ("jane", 42));
    /// # }
    /// ```
    pub fn with_headers<I, S>(config: DeserializerConfig, names: I) -> StreamDeserializer
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        StreamDeserializer {
            config,
            state: StreamState::with_headers(names.into_iter().map(Into::into).collect()),
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Returns the names of the columns.
    ///
    /// This is `None` until the first record was read (with
    /// [`Headers::First`](crate::Headers::First)) or if records have no
    /// names ([`Headers::None`](crate::Headers::None)).
    pub fn headers(&self) -> Option<&[String]> {
        self.state.headers()
    }
}

impl de::StreamDeserializer for StreamDeserializer {
    fn context(&self) -> deser_core::Context {
        self.config.context().clone()
    }

    fn frame(&mut self, input: &[u8], eof: bool) -> Result<Frame, Error> {
        self.state.frame(&self.config, input, eof)
    }

    fn drive_frame<'de>(
        &mut self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        self.state
            .emit_record(&self.config, frame, 0, false, driver)
            .map_err(|mut err| {
                err.resolve_position(frame);
                err
            })
    }

    fn is_text(&self) -> bool {
        true
    }
}

#[cfg(feature = "io")]
impl DeserializerConfig {
    /// Creates a reader of a stream of records (see
    /// [`deser::io::Reader`](deser_core::io::Reader)).
    ///
    /// Every value is a record, see [`StreamDeserializer`].
    pub fn reader<R: Read>(&self, reader: R) -> deser_core::io::Reader<R, StreamDeserializer> {
        deser_core::io::Reader::new(reader, StreamDeserializer::with_config(self.clone()))
    }

    /// Deserializes the records of a reader.
    ///
    /// See [`from_reader`].
    pub fn from_reader<T: DeserializeOwned, R: Read>(&self, mut reader: R) -> Result<T, Error> {
        let mut input = alloc::vec::Vec::new();
        reader.read_to_end(&mut input)?;
        self.from_slice(&input)
    }
}

/// Deserializes the records of a reader.
///
/// This works like [`from_str`](crate::from_str): all records are
/// deserialized as a sequence.  The reader is read to the end, it does not
/// need to be buffered.  To read one record at a time use
/// [`DeserializerConfig::reader`].
///
/// ```
/// use std::collections::BTreeMap;
///
/// let rows: Vec<BTreeMap<String, u32>> =
///     deser_csv::from_reader(&b"a,b\n1,2\n"[..]).unwrap();
/// assert_eq!(rows[0]["b"], 2);
/// ```
#[cfg(feature = "io")]
pub fn from_reader<T: DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    DeserializerConfig::new().from_reader(reader)
}
