use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::{self, Write as _};

use crate::num::{Float, IntBuffer};
use deser_core::ext::Number;
use deser_core::ser::SerializeRef;
use deser_core::ser::{self, EventSink, SerializeDriver};
use deser_core::{Atom, BytesFormat, Error, ErrorKind, Event, Serialize, State};

use crate::parser::{Dialect, load_u32, load_u64};

/// Reads sixteen bytes at `pos`.
#[inline(always)]
fn load_u128(bytes: &[u8], pos: usize) -> u128 {
    u128::from_ne_bytes(*bytes[pos..].first_chunk().unwrap())
}
use crate::{Escape, Nulls, QuoteStyle, Terminator};

/// Configures how values are serialized into delimited text.
///
/// The value is a sequence of records.  Records are maps (for instance
/// structs), the keys of the first record are the names of the columns
/// which are written first (see [`headers`](Self::headers)), or sequences
/// (for instance tuples).  Fields are written in the order of the names,
/// missing fields are empty and keys that are not a column are an error.
/// Fields cannot hold maps or sequences (see
/// [`Separated`](deser_core::adapters::Separated) for lists in a field).
///
/// Numbers are written with the shortest text that reads back as the same
/// value, booleans as `true` and `false`, null as an empty field (see
/// [`nulls`](Self::nulls)) and bytes as base64 (see
/// [`bytes`](Self::bytes)).  Fields are quoted if necessary (see
/// [`quote_style`](Self::quote_style)).
///
/// ```
/// use deser_csv::{SerializerConfig, Terminator};
///
/// #[derive(deser::Serialize)]
/// struct Row {
///     name: &'static str,
///     note: Option<&'static str>,
/// }
///
/// let rows =
///     [Row { name: "a", note: Some("x;y") }, Row { name: "b", note: None }];
/// let config =
///     SerializerConfig::new().delimiter(b';').terminator(Terminator::CrLf);
/// assert_eq!(
///     config.to_string(&rows).unwrap(),
///     "name;note\r\na;\"x;y\"\r\nb;\r\n"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerializerConfig {
    delimiter: u8,
    quote: Option<u8>,
    double_quote: bool,
    escape: Escape,
    terminator: Terminator,
    quote_style: QuoteStyle,
    headers: bool,
    columns: Option<&'static [&'static str]>,
    nulls: Nulls,
    flexible: bool,
    escape_formulas: bool,
    bytes: BytesFormat,
}

impl Default for SerializerConfig {
    fn default() -> SerializerConfig {
        SerializerConfig::new()
    }
}

impl SerializerConfig {
    /// Creates the default configuration (CSV).
    pub const fn new() -> SerializerConfig {
        SerializerConfig {
            delimiter: b',',
            quote: Some(b'"'),
            double_quote: true,
            escape: Escape::None,
            terminator: Terminator::Newline,
            quote_style: QuoteStyle::Necessary,
            headers: true,
            columns: None,
            nulls: Nulls::None,
            flexible: false,
            escape_formulas: false,
            bytes: BytesFormat::BASE64,
        }
    }

    /// Creates the configuration for tab separated values.
    ///
    /// This is the counterpart of
    /// [`DeserializerConfig::tsv`](crate::DeserializerConfig::tsv): fields
    /// are separated by tabs, special characters are escaped with
    /// backslashes and null is `\N`.
    ///
    /// ```
    /// let rows = vec![("a\tb", Some(1)), ("c", None)];
    /// let tsv = deser_csv::SerializerConfig::tsv().to_string(&rows).unwrap();
    /// assert_eq!(tsv, "a\\tb\t1\nc\t\\N\n");
    /// ```
    pub const fn tsv() -> SerializerConfig {
        SerializerConfig::new()
            .delimiter(b'\t')
            .quote(None)
            .escape(Escape::Backslash)
            .nulls(Nulls::Text("\\N"))
    }

    /// Sets the character that separates fields (`,` by default).
    pub const fn delimiter(mut self, delimiter: u8) -> SerializerConfig {
        self.delimiter = delimiter;
        self
    }

    /// Sets the character that quotes fields (`"` by default).
    ///
    /// Without quotes, fields that need them are an error (unless they
    /// can be escaped, see [`escape`](Self::escape)).
    pub const fn quote(mut self, quote: Option<u8>) -> SerializerConfig {
        self.quote = quote;
        self
    }

    /// Sets if quotes in quoted fields are doubled (`true` by default).
    ///
    /// Otherwise they are escaped (see [`escape`](Self::escape)).
    pub const fn double_quote(mut self, yes: bool) -> SerializerConfig {
        self.double_quote = yes;
        self
    }

    /// Sets how characters are escaped (not at all by default).
    ///
    /// With an escape character, special characters in unquoted fields are
    /// escaped instead of quoting the field.
    pub const fn escape(mut self, escape: Escape) -> SerializerConfig {
        self.escape = escape;
        self
    }

    /// Sets the line ending (`\n` by default, see [`Terminator`]).
    pub const fn terminator(mut self, terminator: Terminator) -> SerializerConfig {
        self.terminator = terminator;
        self
    }

    /// Sets when fields are quoted ([`QuoteStyle::Necessary`] by default).
    pub const fn quote_style(mut self, style: QuoteStyle) -> SerializerConfig {
        self.quote_style = style;
        self
    }

    /// Sets if the names of the columns are written before the first
    /// record (`true` by default).
    ///
    /// The names are the keys of the first record (or the given columns,
    /// see [`columns`](Self::columns)).  Records that are sequences have no
    /// names.
    pub const fn headers(mut self, yes: bool) -> SerializerConfig {
        self.headers = yes;
        self
    }

    /// Sets the names of the columns (by default they are the keys of the
    /// first record).
    ///
    /// This is needed if the first record does not have all keys, for
    /// instance because records are enums or skip fields.  Fields are
    /// written in the order of the columns, missing fields are empty.
    ///
    /// ```
    /// #[derive(deser::Serialize)]
    /// #[deser(tag = "kind", rename_all = "lowercase")]
    /// enum Shape {
    ///     Circle { radius: f64 },
    ///     Rect { width: f64, height: f64 },
    /// }
    ///
    /// let shapes = [
    ///     Shape::Circle { radius: 1.0 },
    ///     Shape::Rect { width: 2.0, height: 3.0 },
    /// ];
    /// let config = deser_csv::SerializerConfig::new()
    ///     .columns(&["kind", "radius", "width", "height"]);
    /// assert_eq!(
    ///     config.to_string(&shapes).unwrap(),
    ///     "kind,radius,width,height\ncircle,1.0,,\nrect,,2.0,3.0\n"
    /// );
    /// ```
    pub const fn columns(mut self, names: &'static [&'static str]) -> SerializerConfig {
        self.columns = Some(names);
        self
    }

    /// Sets how null is written ([`Nulls::None`] by default).
    ///
    /// Null is written as an empty field unless it's [`Nulls::Text`].
    /// Strings that would read back as null are quoted (the empty string
    /// with [`Nulls::Empty`]).
    pub const fn nulls(mut self, nulls: Nulls) -> SerializerConfig {
        self.nulls = nulls;
        self
    }

    /// Sets if records can have a different number of fields (`false` by
    /// default).
    pub const fn flexible(mut self, yes: bool) -> SerializerConfig {
        self.flexible = yes;
        self
    }

    /// Sets if strings that spreadsheets would run as formulas are escaped
    /// (`false` by default).
    ///
    /// Spreadsheets run fields that start with `=`, `+`, `-` or `@` (or a
    /// tab or carriage return) as formulas, which is a problem when a file
    /// contains data of untrusted users (["CSV
    /// injection"](https://owasp.org/www-community/attacks/CSV_Injection)).
    /// With this enabled, such strings are prefixed with `'` and quoted
    /// (as recommended by OWASP).  Numbers are written as they are.
    ///
    /// ```
    /// let config = deser_csv::SerializerConfig::new().escape_formulas(true);
    /// let rows = vec![("=1+2", -3)];
    /// assert_eq!(config.to_string(&rows).unwrap(), "\"'=1+2\",-3\n");
    /// ```
    pub const fn escape_formulas(mut self, yes: bool) -> SerializerConfig {
        self.escape_formulas = yes;
        self
    }

    /// Sets how bytes are represented.
    ///
    /// By default bytes are written as base64 ([`BytesFormat::BASE64`]).
    /// Values can request a different format (see
    /// [bytes](deser_core::adapters#bytes)) which takes precedence.
    pub const fn bytes(mut self, format: BytesFormat) -> SerializerConfig {
        self.bytes = format;
        self
    }

    /// Serializes the records of a value.
    ///
    /// The value has to be a sequence of records.
    pub fn to_string<T: Serialize + ?Sized>(&self, value: &T) -> Result<String, Error> {
        self.to_string_with(value, |_| {})
    }

    /// Serializes the records of a value with a configured driver.
    ///
    /// The callback is invoked with the driver before the serialization
    /// starts, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn to_string_with<F, T: Serialize + ?Sized>(
        &self,
        value: &T,
        setup: F,
    ) -> Result<String, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(&value);
        setup(&mut driver);
        let mut out = Vec::new();
        self.write_whole(&mut WriterState::default(), &mut driver, true, &mut out)?;
        Ok(into_string(out))
    }

    /// Serializes the records (or the record) of a driver and appends them
    /// to the output.
    ///
    /// Only the output is changed if this fails.  Between records, the
    /// driver is paused once the output holds at least `limit` bytes and
    /// `false` is returned (the next call continues with the next record).
    pub(crate) fn write(
        &self,
        state: &mut WriterState,
        driver: &mut SerializeDriver<'_>,
        document: bool,
        out: &mut Vec<u8>,
        limit: usize,
    ) -> Result<bool, Error> {
        let drive: DriveFn = if limit == usize::MAX {
            drive_whole
        } else {
            drive_partial
        };
        self.write_with(state, driver, document, out, limit, drive)
    }

    /// Serializes the records of a driver at once and appends them to the
    /// output (see `write`).
    ///
    /// Unlike `write` this does not refer to the pausable instance of the
    /// driver which is only needed by stream serializers.
    pub(crate) fn write_whole(
        &self,
        state: &mut WriterState,
        driver: &mut SerializeDriver<'_>,
        document: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        self.write_with(state, driver, document, out, usize::MAX, drive_whole)
            .map(|_| ())
    }

    /// Implements `write` with the function that drives the driver.
    fn write_with(
        &self,
        state: &mut WriterState,
        driver: &mut SerializeDriver<'_>,
        document: bool,
        out: &mut Vec<u8>,
        limit: usize,
        drive: DriveFn,
    ) -> Result<bool, Error> {
        let dialect = match state.dialect {
            Some(ref dialect) => dialect,
            None => state.dialect.insert(Dialect::new(
                self.delimiter,
                self.quote,
                self.double_quote,
                self.escape,
                self.terminator,
                None,
            )?),
        };
        let mut writer = RecordWriter {
            encoder: FieldEncoder {
                config: self,
                dialect,
                plain: matches!(self.quote_style, QuoteStyle::Necessary | QuoteStyle::Never)
                    && self.nulls == Nulls::None
                    && !self.escape_formulas,
            },
            names: state.names.take(),
            len: state.len,
            document,
            direct: false,
            is_map: false,
            fields: 0,
            record_start: 0,
            field_ends: core::mem::take(&mut state.buffers.field_ends),
            record: core::mem::take(&mut state.buffers.record),
            scratch: Scratch::new(core::mem::take(&mut state.buffers.scratch)),
            open: false,
            limit,
            out,
        };
        let had_names = writer.names.is_some();
        let rv = drive(driver, &mut writer);
        // the state only changes if the value was written (or a part of it,
        // which cannot be taken back)
        if rv.is_ok() || had_names {
            state.names = writer.names;
        }
        if rv.is_ok() {
            state.len = writer.len;
        }
        // the buffers are reused by the next record
        state.buffers = Buffers {
            field_ends: writer.field_ends,
            record: writer.record,
            scratch: writer.scratch.bytes,
        };
        rv
    }
}

/// Drives a driver into a record writer (see `SerializerConfig::write_with`).
type DriveFn = fn(&mut SerializeDriver<'_>, &mut RecordWriter<'_>) -> Result<bool, Error>;

/// Writes the records of a driver at once.
fn drive_whole(
    driver: &mut SerializeDriver<'_>,
    writer: &mut RecordWriter<'_>,
) -> Result<bool, Error> {
    driver
        .drive(|event, state| writer.event(event, state))
        .map(|()| true)
}

/// Writes the records of a driver until the writer pauses it.
fn drive_partial(
    driver: &mut SerializeDriver<'_>,
    writer: &mut RecordWriter<'_>,
) -> Result<bool, Error> {
    driver.drive_until(writer)
}

/// Buffers that are reused for the records of a stream.
#[derive(Clone, Default)]
struct Buffers {
    field_ends: Vec<usize>,
    record: Record,
    scratch: Vec<u8>,
}

impl core::fmt::Debug for Buffers {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Buffers").finish_non_exhaustive()
    }
}

/// The state of a stream of records that is written.
///
/// This holds the names of the columns after the first record was written
/// (or the given names).
#[derive(Debug, Clone, Default)]
pub(crate) struct WriterState {
    names: Option<Vec<String>>,
    // the number of fields of the records
    len: Option<usize>,
    // created with the first record
    dialect: Option<Dialect>,
    buffers: Buffers,
}

impl WriterState {
    /// Creates the state of a stream that continues with the given names
    /// of the columns.
    fn with_headers(names: Vec<String>) -> WriterState {
        WriterState {
            len: Some(names.len()),
            names: Some(names),
            dialect: None,
            buffers: Buffers::default(),
        }
    }
}

/// Serializes records into delimited text.
///
/// Every value is a record, the names of the columns are written before
/// the first one.
///
/// ```
/// use deser_csv::Serializer;
///
/// #[derive(deser::Serialize)]
/// struct Row {
///     name: &'static str,
///     age: u32,
/// }
///
/// let mut serializer = Serializer::new();
/// serializer.serialize(&Row { name: "jane", age: 42 }).unwrap();
/// serializer.serialize(&Row { name: "john", age: 23 }).unwrap();
/// assert_eq!(serializer.finish(), "name,age\njane,42\njohn,23\n");
/// ```
///
/// The serializer is also the stream serializer of delimited text (see
/// [`StreamSerializer`](ser::StreamSerializer)): the output can be taken
/// while records are written.  To write to a [`Write`](std::io::Write) use
/// [`SerializerConfig::writer`].  A serializer created with
/// [`document`](Self::document) writes the records of sequences instead,
/// like [`SerializerConfig::to_string`].
#[derive(Debug, Clone)]
pub struct Serializer {
    config: SerializerConfig,
    state: WriterState,
    out: Vec<u8>,
    // the values are sequences of records
    document: bool,
    // a document was started with `drive_partial` and is not complete
    in_progress: bool,
}

impl Default for Serializer {
    fn default() -> Serializer {
        Serializer::new()
    }
}

impl Serializer {
    /// Creates a serializer.
    pub fn new() -> Serializer {
        Serializer::with_config(&SerializerConfig::new())
    }

    /// Creates a serializer with the given configuration.
    pub fn with_config(config: &SerializerConfig) -> Serializer {
        Serializer::with_state(config, WriterState::default(), false)
    }

    /// Creates a serializer for a stream that continues with the given
    /// names of the columns.
    ///
    /// The names are not written, for instance because the records are
    /// appended to an existing file.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser_csv::{Serializer, SerializerConfig};
    ///
    /// let mut serializer =
    ///     Serializer::with_headers(&SerializerConfig::new(), ["b", "a"]);
    /// serializer.serialize(&BTreeMap::from([("a", 1), ("b", 2)])).unwrap();
    /// assert_eq!(serializer.finish(), "2,1\n");
    /// ```
    pub fn with_headers<I, S>(config: &SerializerConfig, names: I) -> Serializer
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let names = names.into_iter().map(Into::into).collect();
        Serializer::with_state(config, WriterState::with_headers(names), false)
    }

    /// Creates a serializer whose values are sequences of records.
    ///
    /// Every value is written like with [`SerializerConfig::to_string`]:
    /// the elements of the sequence are the records.  The records are
    /// written while they are serialized, so large documents can be
    /// written in parts (see
    /// [`StreamSerializer::drive_partial`](ser::StreamSerializer::drive_partial)).
    ///
    /// ```
    /// use deser_csv::{Serializer, SerializerConfig};
    ///
    /// let mut serializer = Serializer::document(&SerializerConfig::new());
    /// serializer.serialize(&vec![(1, "a"), (2, "b")]).unwrap();
    /// assert_eq!(serializer.finish(), "1,a\n2,b\n");
    /// ```
    pub fn document(config: &SerializerConfig) -> Serializer {
        Serializer::with_state(config, WriterState::default(), true)
    }

    fn with_state(config: &SerializerConfig, state: WriterState, document: bool) -> Serializer {
        Serializer {
            config: config.clone(),
            state,
            out: Vec::new(),
            document,
            in_progress: false,
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &SerializerConfig {
        &self.config
    }

    /// Returns the names of the columns.
    ///
    /// This is `None` until the first record was written (unless the names
    /// were given, see [`with_headers`](Self::with_headers)).
    pub fn headers(&self) -> Option<&[String]> {
        self.state.names.as_deref()
    }

    /// Serializes a record (or the records of a sequence, see
    /// [`document`](Self::document)).
    ///
    /// If the record fails to serialize, nothing is written.
    pub fn serialize<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::Serializer::serialize(self, value)
    }

    /// Serializes a record with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn serialize_with<F, T: Serialize + ?Sized>(
        &mut self,
        value: &T,
        setup: F,
    ) -> Result<(), Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        ser::Serializer::serialize_with(self, value, setup)
    }

    /// Returns the output written so far (that was not cleared).
    pub fn as_str(&self) -> &str {
        // SAFETY: the output is valid UTF-8, see `into_string`
        unsafe { core::str::from_utf8_unchecked(&self.out) }
    }

    /// Returns the output.
    pub fn finish(self) -> String {
        into_string(self.out)
    }
}

impl ser::Serializer for Serializer {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        if self.in_progress {
            return Err(Error::in_progress());
        }
        let len = self.out.len();
        match self
            .config
            .write_whole(&mut self.state, driver, self.document, &mut self.out)
        {
            Ok(()) => Ok(()),
            Err(err) => {
                self.out.truncate(len);
                Err(err)
            }
        }
    }
}

impl ser::StreamSerializer for Serializer {
    fn output(&self) -> &[u8] {
        &self.out
    }

    fn clear_output(&mut self) {
        self.out.clear();
    }

    /// Documents are written in parts (between records).
    fn supports_partial(&self) -> bool {
        self.document
    }

    fn drive_partial(
        &mut self,
        driver: &mut SerializeDriver<'_>,
        limit: usize,
    ) -> Result<bool, Error> {
        if !self.document || (limit == usize::MAX && !self.in_progress) {
            ser::Serializer::drive(self, driver)?;
            return Ok(true);
        }
        let len = self.out.len();
        match self
            .config
            .write(&mut self.state, driver, true, &mut self.out, limit)
        {
            Ok(done) => {
                self.in_progress = !done;
                Ok(done)
            }
            Err(err) => {
                // the records of the parts that were taken stay written
                // (and the stream broken, see `in_progress`)
                self.out.truncate(len);
                Err(err)
            }
        }
    }

    fn in_progress(&self) -> bool {
        self.in_progress
    }
}

#[cfg(feature = "io")]
impl SerializerConfig {
    /// Creates a writer of a stream of records (see
    /// [`deser::io::Writer`](deser_core::io::Writer)).
    ///
    /// Every value is a record, the names of the columns are written before
    /// the first one (see [`headers`](Self::headers)).  A record that fails
    /// to serialize is not written.
    ///
    /// ```
    /// use deser_csv::SerializerConfig;
    ///
    /// #[derive(deser::Serialize)]
    /// struct Row {
    ///     name: &'static str,
    ///     age: u32,
    /// }
    ///
    /// let mut writer = SerializerConfig::new().writer(Vec::new());
    /// writer.write(&Row { name: "jane", age: 42 }).unwrap();
    /// writer.write(&Row { name: "john", age: 23 }).unwrap();
    /// assert_eq!(writer.into_inner(), b"name,age\njane,42\njohn,23\n");
    /// ```
    pub fn writer<W: std::io::Write>(&self, writer: W) -> deser_core::io::Writer<W, Serializer> {
        deser_core::io::Writer::new(writer, Serializer::with_config(self))
    }

    /// Serializes the records of a value to a writer.
    ///
    /// See [`to_writer`].
    pub fn to_writer<W: std::io::Write, T: Serialize + ?Sized>(
        &self,
        writer: W,
        value: &T,
    ) -> Result<(), Error> {
        deser_core::io::to_writer(writer, Serializer::document(self), value)
    }
}

/// Serializes the records of a value to a writer.
///
/// The records are written while they are serialized (in parts of about
/// 8 KiB, see [`deser::io`](deser_core::io)), so the writer does not need
/// to be buffered and the records are not held in memory.  To write one
/// record at a time use [`SerializerConfig::writer`].
///
/// ```
/// let mut out = Vec::new();
/// deser_csv::to_writer(&mut out, &vec![(1, "a"), (2, "b")]).unwrap();
/// assert_eq!(out, b"1,a\n2,b\n");
/// ```
#[cfg(feature = "io")]
pub fn to_writer<W: std::io::Write, T: Serialize + ?Sized>(
    writer: W,
    value: &T,
) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}

/// Converts the output into a string.
///
/// The output only holds text and ASCII special characters (which the
/// dialect checks), so it's valid UTF-8.
fn into_string(out: Vec<u8>) -> String {
    debug_assert!(core::str::from_utf8(&out).is_ok());
    // SAFETY: see above
    unsafe { String::from_utf8_unchecked(out) }
}

/// Serializes the records of a value to delimited text.
///
/// This uses the default [`SerializerConfig`] (CSV), see there for more
/// information.
///
/// ```
/// #[derive(deser::Serialize)]
/// struct Row {
///     name: &'static str,
///     tags: Vec<&'static str>,
/// }
///
/// #[derive(deser::Serialize)]
/// struct Tagged {
///     name: &'static str,
///     #[deser(as = deser::adapters::Separated<';'>)]
///     tags: Vec<&'static str>,
/// }
///
/// let row = Tagged { name: "a", tags: vec!["x", "y"] };
/// assert_eq!(deser_csv::to_string(&[row]).unwrap(), "name,tags\na,x;y\n");
///
/// let row = Row { name: "a", tags: vec!["x", "y"] };
/// assert!(deser_csv::to_string(&[row]).is_err());
/// ```
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    SerializerConfig::new().to_string(value)
}

/// The fields of a record that is collected.
#[derive(Clone, Default)]
struct Record {
    /// The keys (without separators).
    keys: Vec<u8>,
    key_ends: Vec<usize>,
    /// The encoded fields (without separators).
    fields: Vec<u8>,
    field_ends: Vec<usize>,
}

impl Record {
    fn clear(&mut self) {
        self.keys.clear();
        self.key_ends.clear();
        self.fields.clear();
        self.field_ends.clear();
    }

    fn key(&self, index: usize) -> &[u8] {
        let start = if index == 0 {
            0
        } else {
            self.key_ends[index - 1]
        };
        &self.keys[start..self.key_ends[index]]
    }

    fn field(&self, index: usize) -> &[u8] {
        let start = if index == 0 {
            0
        } else {
            self.field_ends[index - 1]
        };
        &self.fields[start..self.field_ends[index]]
    }
}

/// Writes the events of records.
///
/// Fields are written directly to the output while the keys of a record
/// match the names of the columns in their order.  Otherwise (and for the
/// first record, which comes after the names) the fields are collected and
/// written in the order of the names at the end of the record.
struct RecordWriter<'a> {
    encoder: FieldEncoder<'a>,
    names: Option<Vec<String>>,
    len: Option<usize>,
    document: bool,
    /// The fields are written to the output directly.
    direct: bool,
    is_map: bool,
    /// The number of fields of the current record.
    fields: usize,
    /// Where the current record starts in the output.
    record_start: usize,
    /// Where the fields of the current record end in the output (while
    /// writing directly).
    field_ends: Vec<usize>,
    /// The collected record (while not writing directly).
    record: Record,
    /// The text of numbers and other atoms that are not text.
    scratch: Scratch,
    /// A record is being written.
    open: bool,
    /// The driver is paused between records once the output is this long.
    limit: usize,
    out: &'a mut Vec<u8>,
}

impl EventSink for RecordWriter<'_> {
    #[inline]
    fn event(
        &mut self,
        event: Event<'_>,
        _value: SerializeRef<'_>,
        state: &mut State,
    ) -> Result<(), Error> {
        RecordWriter::event(self, event, state)
    }

    #[inline]
    fn pause(&mut self) -> bool {
        // the fields of a record can still move (see `collect`)
        !self.open && self.out.len() >= self.limit
    }
}

impl RecordWriter<'_> {
    #[inline]
    fn event(&mut self, event: Event<'_>, state: &State) -> Result<(), Error> {
        // most events are the fields of records
        if let Event::Atom(ref atom) = event
            && state.depth() == usize::from(self.document) + 1
        {
            return self.atom(atom, state.is_map_key());
        }
        self.structure(event, state)
    }

    /// Handles the events that are not fields.
    fn structure(&mut self, event: Event<'_>, state: &State) -> Result<(), Error> {
        // the depth of the event, without the container it starts: 0 at the
        // top level, 1 in a record (2 in the record of a document)
        let depth = match event {
            Event::MapStart(_) | Event::SeqStart(_) => state.depth().saturating_sub(1),
            _ => state.depth(),
        };
        let record_depth = usize::from(self.document);
        match event {
            Event::SeqStart(_) | Event::SeqEnd if self.document && depth == 0 => {}
            _ if depth < record_depth => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "CSV documents are sequences of records",
                ));
            }
            Event::MapStart(_) | Event::SeqStart(_) if depth == record_depth => {
                if self.names.is_none()
                    && let Some(columns) = self.encoder.config.columns
                {
                    let names: Vec<String> = columns.iter().map(|name| name.to_string()).collect();
                    if self.encoder.config.headers {
                        self.write_names(&names)?;
                    }
                    self.names = Some(names);
                }
                self.is_map = matches!(event, Event::MapStart(_));
                self.open = true;
                self.direct = !self.is_map || self.names.is_some();
                self.fields = 0;
                self.record_start = self.out.len();
                self.field_ends.clear();
                self.record.clear();
            }
            Event::MapEnd | Event::SeqEnd if depth == record_depth => {
                self.open = false;
                self.finish_record()?
            }
            _ if depth == record_depth => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "CSV records must be maps or sequences",
                ));
            }
            // the ends only if layers emitted maps or sequences
            Event::MapStart(_) | Event::SeqStart(_) | Event::MapEnd | Event::SeqEnd => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    "CSV fields cannot hold maps or sequences",
                ));
            }
            Event::Atom(ref atom) => return self.atom(atom, state.is_map_key()),
        }
        Ok(())
    }

    /// Handles a key or field of a record.
    #[inline(always)]
    fn atom(&mut self, atom: &Atom<'_>, is_key: bool) -> Result<(), Error> {
        if is_key {
            return self.key(atom);
        }
        let text = self.encoder.text(atom, &mut self.scratch)?;
        if self.direct {
            if self.fields > 0 {
                self.out.push(self.encoder.dialect.delimiter);
            }
            self.encoder.encode(text, self.out)?;
            self.field_ends.push(self.out.len());
        } else {
            self.encoder.encode(text, &mut self.record.fields)?;
            self.record.field_ends.push(self.record.fields.len());
        }
        self.fields += 1;
        Ok(())
    }

    /// Handles the key of a field.
    #[inline(always)]
    fn key(&mut self, atom: &Atom<'_>) -> Result<(), Error> {
        // most keys are the names of the columns in their order
        if self.direct
            && let Atom::Str(key) = atom
            && let Some(names) = &self.names
            && let Some(name) = names.get(self.fields)
            && same_key(name.as_bytes(), key.as_bytes())
        {
            return Ok(());
        }
        self.other_key(atom)
    }

    /// Handles a key that is not the name of the next column (see `key`).
    #[inline(never)]
    fn other_key(&mut self, atom: &Atom<'_>) -> Result<(), Error> {
        let key = match atom {
            Atom::Null | Atom::Bytes(_) => None,
            atom => self.encoder.text(atom, &mut self.scratch)?,
        };
        let key = key.ok_or_else(unsupported_key)?;
        if !self.direct {
            self.record.keys.extend_from_slice(key.bytes);
        } else {
            let names = self.names.as_deref().unwrap_or_default();
            if let Some(name) = names.get(self.fields)
                && same_key(name.as_bytes(), key.bytes)
            {
                return Ok(());
            }
            let key = key.bytes.to_vec();
            self.collect();
            self.record.keys.extend_from_slice(&key);
        }
        self.record.key_ends.push(self.record.keys.len());
        Ok(())
    }

    /// Moves the fields that were written directly into the record.
    fn collect(&mut self) {
        let names = self.names.as_deref().unwrap_or_default();
        let mut start = self.record_start;
        for (index, &end) in self.field_ends.iter().enumerate() {
            if index > 0 {
                // the delimiter
                start += 1;
            }
            self.record.fields.extend_from_slice(&self.out[start..end]);
            self.record.field_ends.push(self.record.fields.len());
            self.record.keys.extend_from_slice(names[index].as_bytes());
            self.record.key_ends.push(self.record.keys.len());
            start = end;
        }
        self.out.truncate(self.record_start);
        self.direct = false;
    }

    /// Ends the record.
    fn finish_record(&mut self) -> Result<(), Error> {
        let start = self.record_start;
        if !self.is_map {
            self.check_len(self.fields)?;
            return self.terminate(self.fields, start);
        }

        if self.direct {
            // the fields that follow are missing, they are null
            let len = self.names.as_ref().map_or(0, Vec::len);
            for column in self.fields..len {
                if column > 0 {
                    self.out.push(self.encoder.dialect.delimiter);
                }
                self.encoder.encode(None, self.out)?;
            }
            self.check_len(len)?;
            return self.terminate(len, start);
        }

        let count = self.record.field_ends.len();
        if self.names.is_none() {
            // the keys are UTF-8, they are strings or written by us
            let names: Vec<String> = (0..count)
                .map(|index| String::from_utf8_lossy(self.record.key(index)).into_owned())
                .collect();
            if self.encoder.config.headers {
                self.write_names(&names)?;
            }
            self.names = Some(names);
        }
        let len = self.names.as_ref().map_or(0, Vec::len);
        self.check_len(len)?;
        let names = self.names.as_ref().unwrap();
        let start = self.out.len();
        let mut order = vec![None; len];
        for index in 0..count {
            let key = String::from_utf8_lossy(self.record.key(index));
            match names.iter().position(|name| *name == key) {
                Some(column) if order[column].is_none() => order[column] = Some(index),
                Some(_) => {
                    return Err(Error::new(
                        ErrorKind::Unexpected,
                        format!("duplicate field `{}`", key),
                    ));
                }
                None => {
                    return Err(Error::new(
                        ErrorKind::Unexpected,
                        format!("field `{}` is not a column", key),
                    ));
                }
            }
        }
        for (column, index) in order.into_iter().enumerate() {
            if column > 0 {
                self.out.push(self.encoder.dialect.delimiter);
            }
            match index {
                Some(index) => self.out.extend_from_slice(self.record.field(index)),
                // missing fields are null
                None => self.encoder.encode(None, self.out)?,
            }
        }
        self.terminate(len, start)
    }

    /// Checks the number of fields of a record.
    fn check_len(&mut self, len: usize) -> Result<(), Error> {
        match self.len {
            Some(expected) if expected != len && !self.encoder.config.flexible => Err(Error::new(
                ErrorKind::Unexpected,
                format!("record has {} fields, expected {}", len, expected),
            )),
            Some(_) => Ok(()),
            None => {
                self.len = Some(len);
                Ok(())
            }
        }
    }

    /// Ends a record with `len` fields which starts at `start` in the
    /// output.
    fn terminate(&mut self, len: usize, start: usize) -> Result<(), Error> {
        if len == 1 && self.out.len() == start {
            // an empty line is a blank line, the only field is quoted
            match self.encoder.dialect.quote {
                Some(quote) => self.out.extend_from_slice(&[quote, quote]),
                None => {
                    return Err(Error::new(
                        ErrorKind::Unexpected,
                        "a record with a single empty field needs quotes",
                    ));
                }
            }
        }
        match self.encoder.config.terminator {
            Terminator::Newline => self.out.push(b'\n'),
            Terminator::CrLf => self.out.extend_from_slice(b"\r\n"),
            Terminator::Byte(byte) => self.out.push(byte),
        }
        Ok(())
    }

    /// Writes the names of the columns.
    fn write_names(&mut self, names: &[String]) -> Result<(), Error> {
        let start = self.out.len();
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                self.out.push(self.encoder.dialect.delimiter);
            }
            let text = Text {
                bytes: name.as_bytes(),
                numeric: false,
            };
            self.encoder.encode(Some(text), self.out)?;
        }
        self.terminate(names.len(), start)
    }
}

/// The text of a field.
#[derive(Clone, Copy)]
struct Text<'a> {
    bytes: &'a [u8],
    numeric: bool,
}

/// Encodes fields.
#[derive(Clone, Copy)]
struct FieldEncoder<'a> {
    config: &'a SerializerConfig,
    dialect: &'a Dialect,
    /// Text without special characters is written as it is: fields are
    /// only quoted if necessary, null is empty and formulas are not
    /// escaped.
    plain: bool,
}

impl FieldEncoder<'_> {
    /// Encodes a field (`None` for null).
    #[inline(always)]
    fn encode(&self, text: Option<Text<'_>>, out: &mut Vec<u8>) -> Result<(), Error> {
        // most fields are written as they are
        if self.plain
            && let Some(Text { bytes, .. }) = text
            && !self.dialect.has_special(bytes)
        {
            push_bytes(out, bytes);
            return Ok(());
        }
        self.encode_special(text, out)
    }

    /// Encodes a field that is not written as it is (see `encode`).
    #[inline(never)]
    fn encode_special(&self, text: Option<Text<'_>>, out: &mut Vec<u8>) -> Result<(), Error> {
        let config = self.config;
        let Some(Text { bytes, numeric }) = text else {
            if let Nulls::Text(null) = config.nulls {
                out.extend_from_slice(null.as_bytes());
            }
            return Ok(());
        };
        let reads_as_null = match config.nulls {
            Nulls::None => false,
            Nulls::Empty => bytes.is_empty(),
            Nulls::Text(null) => bytes == null.as_bytes(),
        };
        // formulas get a `'` in front and are quoted (as recommended by
        // OWASP)
        let formula = config.escape_formulas
            && !numeric
            && matches!(
                bytes.first(),
                Some(b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r')
            );
        let has_special = self.dialect.has_special(bytes);
        let quote_style = match config.quote_style {
            QuoteStyle::Always => true,
            QuoteStyle::NonNumeric => !numeric,
            QuoteStyle::Necessary | QuoteStyle::Never => false,
        };
        if !quote_style && !has_special && !reads_as_null && !formula {
            out.extend_from_slice(bytes);
            return Ok(());
        }

        let prefix: &[u8] = if formula { b"'" } else { b"" };
        let escape = config.escape.byte();
        let prefix_is_special = formula && self.dialect.is_special(b'\'');
        // without quotes, special characters are escaped (and so is the
        // first character of a text that would read as null)
        let quoted = quote_style
            || (formula && self.dialect.quote.is_some() && config.quote_style != QuoteStyle::Never)
            || (escape.is_none() && (has_special || prefix_is_special || reads_as_null))
            || (reads_as_null && bytes.is_empty());
        let text = prefix.iter().chain(bytes).copied();

        if !quoted {
            for (index, byte) in text.enumerate() {
                if self.dialect.is_special(byte) || (index == 0 && reads_as_null) {
                    // `escape` is set, otherwise the field would be quoted
                    out.push(escape.unwrap_or(b'\\'));
                    out.push(self.escaped(byte));
                } else {
                    out.push(byte);
                }
            }
            return Ok(());
        }

        let quote = match self.dialect.quote {
            Some(quote) if config.quote_style != QuoteStyle::Never => quote,
            _ => {
                return Err(Error::new(
                    ErrorKind::Unexpected,
                    format!(
                        "field {:?} needs to be quoted",
                        String::from_utf8_lossy(bytes)
                    ),
                ));
            }
        };
        out.push(quote);
        if formula {
            self.push_quoted(b'\'', quote, out)?;
        }
        let mut bytes = bytes;
        loop {
            // copy the text up to the next character that is escaped
            let run = bytes
                .iter()
                .position(|&b| b == quote || Some(b) == escape)
                .unwrap_or(bytes.len());
            out.extend_from_slice(&bytes[..run]);
            let Some((&byte, rest)) = bytes[run..].split_first() else {
                break;
            };
            bytes = rest;
            self.push_quoted(byte, quote, out)?;
        }
        out.push(quote);
        Ok(())
    }

    /// Writes a character in a quoted field, doubled or escaped if needed.
    fn push_quoted(&self, byte: u8, quote: u8, out: &mut Vec<u8>) -> Result<(), Error> {
        let escape = self.config.escape.byte();
        if byte == quote && self.config.double_quote {
            out.extend_from_slice(&[quote, quote]);
        } else if byte == quote || Some(byte) == escape {
            match escape {
                Some(escape) => out.extend_from_slice(&[escape, self.escaped(byte)]),
                None => {
                    return Err(Error::new(
                        ErrorKind::Unexpected,
                        "quotes in quoted fields need to be doubled or escaped",
                    ));
                }
            }
        } else {
            out.push(byte);
        }
        Ok(())
    }

    /// Returns what follows the escape character for a byte.
    fn escaped(&self, byte: u8) -> u8 {
        match (self.config.escape, byte) {
            (Escape::Backslash, b'\t') => b't',
            (Escape::Backslash, b'\n') => b'n',
            (Escape::Backslash, b'\r') => b'r',
            (_, byte) => byte,
        }
    }

    /// Returns the text of an atom, `None` for null.
    ///
    /// Text that is not a string is written into `scratch`.
    #[inline(always)]
    fn text<'a>(
        &self,
        atom: &'a Atom<'_>,
        scratch: &'a mut Scratch,
    ) -> Result<Option<Text<'a>>, Error> {
        let (bytes, numeric) = match *atom {
            Atom::Str(ref value) | Atom::Lexical(ref value) => (value.as_bytes(), false),
            Atom::Null => return Ok(None),
            Atom::Bool(value) => (if value { &b"true"[..] } else { b"false" }, false),
            // numbers are formatted on the stack
            Atom::U64(value) => (scratch.int.format_u64(value).as_bytes(), true),
            Atom::I64(value) => (scratch.int.format_i64(value).as_bytes(), true),
            Atom::F64(value) => (scratch.float(value), true),
            Atom::F32(value) => (scratch.float(value), true),
            _ => return self.other_text(atom, &mut scratch.bytes),
        };
        Ok(Some(Text { bytes, numeric }))
    }

    /// Returns the text of an atom that is not a number (see `text`).
    #[inline(never)]
    fn other_text<'a>(
        &self,
        atom: &'a Atom<'_>,
        scratch: &'a mut Vec<u8>,
    ) -> Result<Option<Text<'a>>, Error> {
        scratch.clear();
        let numeric = match *atom {
            Atom::Char(value) => {
                scratch.extend_from_slice(value.encode_utf8(&mut [0; 4]).as_bytes());
                false
            }
            Atom::Bytes(ref bytes) => {
                let format = bytes.fallback.copied().unwrap_or(self.config.bytes);
                let text = format
                    .encode(bytes)
                    .or_else(|| BytesFormat::BASE64.encode(bytes))
                    .unwrap_or_default();
                scratch.extend_from_slice(text.as_bytes());
                false
            }
            Atom::Ext(ref ext) => {
                if let Some(number) = ext.downcast_value_ref::<Number>() {
                    // numbers keep their text
                    scratch.extend_from_slice(number.as_str().as_bytes());
                } else if let Some(value) = ext.downcast_ref::<u128>() {
                    let _ = write!(ByteWriter(scratch), "{}", value);
                } else if let Some(value) = ext.downcast_ref::<i128>() {
                    let _ = write!(ByteWriter(scratch), "{}", value);
                } else {
                    return match ext.fallback() {
                        Atom::Ext(_) => Err(Error::new(
                            ErrorKind::UnsupportedType,
                            format!("CSV does not support {}", ext.name()),
                        )),
                        fallback => {
                            let mut inner = Scratch::new(Vec::new());
                            let numeric = match self.text(&fallback, &mut inner)? {
                                Some(text) => {
                                    let numeric = text.numeric;
                                    scratch.extend_from_slice(text.bytes);
                                    numeric
                                }
                                None => return Ok(None),
                            };
                            Ok(Some(Text {
                                bytes: scratch,
                                numeric,
                            }))
                        }
                    };
                }
                true
            }
            // values whose type was inferred from text are written as value
            Atom::Implicit(ref value) => {
                let mut inner = Scratch::new(Vec::new());
                return Ok(match self.text(&value.value().to_atom(), &mut inner)? {
                    Some(text) => {
                        let numeric = text.numeric;
                        scratch.extend_from_slice(text.bytes);
                        Some(Text {
                            bytes: scratch,
                            numeric,
                        })
                    }
                    None => None,
                });
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::UnsupportedType,
                    format!("CSV does not support {}", atom.name()),
                ));
            }
        };
        Ok(Some(Text {
            bytes: scratch,
            numeric,
        }))
    }
}

/// Formats into a byte buffer (`std::io::Write` is not in `core`).
struct ByteWriter<'a>(&'a mut Vec<u8>);

impl fmt::Write for ByteWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

/// Holds the text of atoms that are not text (see `FieldEncoder::text`).
struct Scratch {
    /// Text that is not a number.
    bytes: Vec<u8>,
    int: IntBuffer,
    #[cfg(feature = "zmij")]
    float: zmij::Buffer,
}

impl Scratch {
    fn new(bytes: Vec<u8>) -> Scratch {
        Scratch {
            bytes,
            int: IntBuffer::new(),
            #[cfg(feature = "zmij")]
            float: zmij::Buffer::new(),
        }
    }

    /// Formats a float with the shortest text that reads back as the same
    /// value of its type (`f32` or `f64`), like the other formats.
    #[inline]
    fn float<F: FormatFloat>(&mut self, value: F) -> &[u8] {
        if !value.is_finite() {
            // `NaN`, `inf` and `-inf`
            self.bytes.clear();
            let _ = write!(ByteWriter(&mut self.bytes), "{}", value.to_f64());
            return &self.bytes;
        }
        #[cfg(feature = "zmij")]
        {
            self.float.format_finite(value).as_bytes()
        }
        #[cfg(not(feature = "zmij"))]
        {
            self.bytes.clear();
            self.bytes
                .extend_from_slice(crate::num::format_finite(value).as_bytes());
            &self.bytes
        }
    }
}

/// The floats that can be formatted.
#[cfg(feature = "zmij")]
trait FormatFloat: zmij::Float + Float {}

#[cfg(feature = "zmij")]
impl<F: zmij::Float + Float> FormatFloat for F {}

#[cfg(not(feature = "zmij"))]
trait FormatFloat: Float {}

#[cfg(not(feature = "zmij"))]
impl<F: Float> FormatFloat for F {}

/// Appends bytes to the output.
///
/// Fields are mostly short, and for them `extend_from_slice` spends more
/// time calling `memcpy` than copying.  Up to 32 bytes are copied with two
/// writes that overlap.
#[inline(always)]
fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = bytes.len();
    if len > 32 {
        out.extend_from_slice(bytes);
        return;
    }
    out.reserve(32);
    let start = out.len();
    // SAFETY: there is room for 32 bytes after the end of the output.  The
    // writes are within the first `len` of them and cover all of them, so
    // the output is initialized up to its new length.
    unsafe {
        let dst = out.as_mut_ptr().add(start);
        match len {
            0 => {}
            // the first, middle and last byte are all bytes
            1..=3 => {
                *dst = bytes[0];
                *dst.add(len / 2) = bytes[len / 2];
                *dst.add(len - 1) = bytes[len - 1];
            }
            4..=8 => {
                dst.cast::<u32>().write_unaligned(load_u32(bytes, 0));
                dst.add(len - 4)
                    .cast::<u32>()
                    .write_unaligned(load_u32(bytes, len - 4));
            }
            9..=16 => {
                dst.cast::<u64>().write_unaligned(load_u64(bytes, 0));
                dst.add(len - 8)
                    .cast::<u64>()
                    .write_unaligned(load_u64(bytes, len - 8));
            }
            _ => {
                dst.cast::<u128>().write_unaligned(load_u128(bytes, 0));
                dst.add(len - 16)
                    .cast::<u128>()
                    .write_unaligned(load_u128(bytes, len - 16));
            }
        }
        out.set_len(start + len);
    }
}

/// Returns `true` if a key is the name of a column.
///
/// Keys are short, comparing them with words that overlap is faster than
/// calling `memcmp` for every field.
#[inline(always)]
fn same_key(name: &[u8], key: &[u8]) -> bool {
    let len = name.len();
    if len != key.len() {
        return false;
    }
    match len {
        0 => true,
        // the first, middle and last byte are all bytes
        1..=3 => {
            name[0] == key[0] && name[len / 2] == key[len / 2] && name[len - 1] == key[len - 1]
        }
        4..=8 => {
            load_u32(name, 0) == load_u32(key, 0)
                && load_u32(name, len - 4) == load_u32(key, len - 4)
        }
        9..=16 => {
            load_u64(name, 0) == load_u64(key, 0)
                && load_u64(name, len - 8) == load_u64(key, len - 8)
        }
        _ => name == key,
    }
}

#[cold]
fn unsupported_key() -> Error {
    Error::new(
        ErrorKind::UnsupportedType,
        "the keys of records must be strings, numbers or booleans",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_bytes() {
        let text: Vec<u8> = (b'a'..=b'z').chain(b'A'..=b'Z').collect();
        for len in 0..=text.len() {
            for prefix in [0, 1, 7] {
                let mut out = vec![b'-'; prefix];
                push_bytes(&mut out, &text[..len]);
                assert_eq!(&out[prefix..], &text[..len]);
                assert_eq!(out.len(), prefix + len);
            }
        }
    }

    #[test]
    fn test_same_key() {
        let text: Vec<u8> = (b'a'..=b'z').chain(b'A'..=b'Z').collect();
        for len in 0..=text.len() {
            let name = &text[..len];
            assert!(same_key(name, &text[..len]));
            if len > 0 {
                assert!(!same_key(name, &text[..len - 1]));
                assert!(!same_key(&text[..len - 1], name));
            }
            for pos in 0..len {
                let mut key = name.to_vec();
                key[pos] ^= 1;
                assert!(!same_key(name, &key), "{:?}", key);
            }
        }
    }
}
