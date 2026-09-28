use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::marker::PhantomData;

use deser_core::Text;
use deser_core::de::{self, Deserialize, DeserializeDriver, Frame, LexicalRules};
use deser_core::{Atom, Bytes, BytesFormat, ContainerShape, Error, ErrorKind, Event, Source};

use crate::parser::{Dialect, Field, Options, QUOTED, Scan, Scanner, UNESCAPE, unescape};
use crate::{Escape, Headers, Nulls, Terminator, Trim};

/// Configures how delimited text is deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The methods
/// [`from_str`](Self::from_str) and [`from_slice`](Self::from_slice) work
/// like the functions of the same name.  The default is CSV as described
/// by [RFC 4180](https://www.rfc-editor.org/rfc/rfc4180), with a header,
/// any line ending and blank lines skipped.
///
/// ```
/// use deser_csv::DeserializerConfig;
///
/// const SEMICOLONS: DeserializerConfig =
///     DeserializerConfig::new().delimiter(b';');
/// let rows: Vec<(String, u32)> = SEMICOLONS
///     .headers(deser_csv::Headers::None)
///     .from_str("a;1\nb;2\n")
///     .unwrap();
/// assert_eq!(rows, [("a".into(), 1), ("b".into(), 2)]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializerConfig {
    pub(crate) delimiter: u8,
    pub(crate) quote: Option<u8>,
    pub(crate) double_quote: bool,
    pub(crate) escape: Escape,
    pub(crate) terminator: Terminator,
    pub(crate) comment: Option<u8>,
    pub(crate) headers: Headers,
    pub(crate) trim: Trim,
    pub(crate) nulls: Nulls,
    pub(crate) skip_blank_lines: bool,
    pub(crate) flexible: bool,
    pub(crate) lenient_quotes: bool,
    pub(crate) sep_line: bool,
    pub(crate) max_record_len: usize,
    pub(crate) bytes: BytesFormat,
    pub(crate) track_locations: bool,
}

impl Default for DeserializerConfig {
    fn default() -> DeserializerConfig {
        DeserializerConfig::new()
    }
}

impl DeserializerConfig {
    /// Creates the default configuration (CSV).
    pub const fn new() -> DeserializerConfig {
        DeserializerConfig {
            delimiter: b',',
            quote: Some(b'"'),
            double_quote: true,
            escape: Escape::None,
            terminator: Terminator::Newline,
            comment: None,
            headers: Headers::First,
            trim: Trim::None,
            nulls: Nulls::None,
            skip_blank_lines: true,
            flexible: false,
            lenient_quotes: false,
            sep_line: false,
            max_record_len: 64 * 1024 * 1024,
            bytes: BytesFormat::BASE64,
            track_locations: false,
        }
    }

    /// Creates the configuration for tab separated values.
    ///
    /// Fields are separated by tabs and are not quoted.  Tabs, line breaks
    /// and backslashes in fields are escaped with backslashes (`\t`, `\n`,
    /// `\r` and `\\`) and `\N` is null (see [`Escape::Backslash`] and
    /// [`Nulls::Text`]).  This is how databases (like PostgreSQL's `COPY`
    /// and MySQL's `LOAD DATA`) and many tools write TSV, and it reads the
    /// TSV of [IANA](https://www.iana.org/assignments/media-types/text/tab-separated-values)
    /// (which cannot contain tabs and line breaks in fields) as well.  For
    /// TSV with quotes (as written by spreadsheets) use
    /// `DeserializerConfig::new().delimiter(b'\t')`.
    ///
    /// ```
    /// use deser_csv::DeserializerConfig;
    ///
    /// #[derive(deser::Deserialize)]
    /// struct Row {
    ///     name: String,
    ///     note: Option<String>,
    /// }
    ///
    /// let rows: Vec<Row> = DeserializerConfig::tsv()
    ///     .from_str("name\tnote\nJane\ta\\tb\nJohn\t\\N\n")
    ///     .unwrap();
    /// assert_eq!(rows[0].note.as_deref(), Some("a\tb"));
    /// assert_eq!(rows[1].note, None);
    /// ```
    pub const fn tsv() -> DeserializerConfig {
        DeserializerConfig::new()
            .delimiter(b'\t')
            .quote(None)
            .escape(Escape::Backslash)
            .nulls(Nulls::Text("\\N"))
    }

    /// Sets the character that separates fields (`,` by default).
    ///
    /// Special characters (the delimiter, quote, escape and terminator)
    /// have to be distinct ASCII characters, otherwise deserializing fails.
    pub const fn delimiter(mut self, delimiter: u8) -> DeserializerConfig {
        self.delimiter = delimiter;
        self
    }

    /// Sets the character that quotes fields (`"` by default).
    ///
    /// Quoted fields can contain the delimiter and line breaks.  With
    /// `None` quotes are regular characters.
    pub const fn quote(mut self, quote: Option<u8>) -> DeserializerConfig {
        self.quote = quote;
        self
    }

    /// Sets if two quotes in a quoted field are a quote (`true` by
    /// default).
    ///
    /// Without doubled quotes, quotes in quoted fields have to be escaped
    /// (see [`escape`](Self::escape)).
    pub const fn double_quote(mut self, yes: bool) -> DeserializerConfig {
        self.double_quote = yes;
        self
    }

    /// Sets how characters are escaped (not at all by default).
    pub const fn escape(mut self, escape: Escape) -> DeserializerConfig {
        self.escape = escape;
        self
    }

    /// Sets what ends records ([`Terminator::Newline`] by default).
    pub const fn terminator(mut self, terminator: Terminator) -> DeserializerConfig {
        self.terminator = terminator;
        self
    }

    /// Sets the character that starts comment lines (none by default).
    ///
    /// Lines that start with it are skipped.  The character only starts a
    /// comment at the start of a line (`a,#b` is a regular record).
    pub const fn comment(mut self, comment: Option<u8>) -> DeserializerConfig {
        self.comment = comment;
        self
    }

    /// Sets where the names of the columns come from ([`Headers::First`] by
    /// default).
    ///
    /// With names, records are maps of the names to the fields.  Without
    /// names ([`Headers::None`]) records are sequences.
    pub const fn headers(mut self, headers: Headers) -> DeserializerConfig {
        self.headers = headers;
        self
    }

    /// Sets which whitespace is removed ([`Trim::None`] by default).
    ///
    /// Spaces and tabs are removed from the start and end of unquoted
    /// fields and around the quotes of quoted fields (`a, "b" ,c`).
    pub const fn trim(mut self, trim: Trim) -> DeserializerConfig {
        self.trim = trim;
        self
    }

    /// Sets which fields are null ([`Nulls::None`] by default).
    ///
    /// Without nulls, empty fields are `None` for optionals of types that
    /// do not accept the empty string (like `Option<u32>`) and `Some("")`
    /// for strings.  Quoted fields are never null.
    pub const fn nulls(mut self, nulls: Nulls) -> DeserializerConfig {
        self.nulls = nulls;
        self
    }

    /// Sets if blank lines are skipped (`true` by default).
    ///
    /// Otherwise a blank line is a record with a single empty field.  Lines
    /// with only whitespace are not blank.
    pub const fn skip_blank_lines(mut self, yes: bool) -> DeserializerConfig {
        self.skip_blank_lines = yes;
        self
    }

    /// Sets if records can have a different number of fields (`false` by
    /// default).
    ///
    /// By default, records must have as many fields as there are columns
    /// (or as the first record has, without names).  With flexible records,
    /// missing fields are missing in the map and fields without names are
    /// keyed with their index (`"3"`), so they end up in a flattened map
    /// or are ignored like unknown fields.
    pub const fn flexible(mut self, yes: bool) -> DeserializerConfig {
        self.flexible = yes;
        self
    }

    /// Sets if quotes that do not follow the rules are accepted (`false` by
    /// default).
    ///
    /// By default, quotes in unquoted fields (`5'10"`) and characters after
    /// the closing quote of a field (`"a"b`) are errors.  With lenient
    /// quotes the quotes of unquoted fields are regular characters and
    /// characters after the closing quote are part of the field (`ab`).
    pub const fn lenient_quotes(mut self, yes: bool) -> DeserializerConfig {
        self.lenient_quotes = yes;
        self
    }

    /// Sets if a `sep=` line at the start selects the delimiter (`false` by
    /// default).
    ///
    /// Excel writes and understands a first line like `sep=;` which sets
    /// the delimiter of the file.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    ///
    /// let config = deser_csv::DeserializerConfig::new().sep_line(true);
    /// let rows: Vec<BTreeMap<String, u32>> =
    ///     config.from_str("sep=;\na;b\n1;2\n").unwrap();
    /// assert_eq!(rows[0]["b"], 2);
    /// ```
    pub const fn sep_line(mut self, yes: bool) -> DeserializerConfig {
        self.sep_line = yes;
        self
    }

    /// Sets the maximum length of a record in a stream in bytes (64 MiB by
    /// default).
    ///
    /// A record in a stream is buffered until it's complete.  A longer
    /// record is an error which ends the stream, which protects from
    /// streams that never end a record (like a quoted field that is never
    /// closed).  Inputs in memory are not limited.
    pub const fn max_record_len(mut self, len: usize) -> DeserializerConfig {
        self.max_record_len = len;
        self
    }

    /// Sets how fields are decoded into bytes.
    ///
    /// Types that expect bytes (like `Vec<u8>`) decode fields as base64 by
    /// default.  Fields which are not UTF-8 are passed on as bytes (see
    /// [bytes](deser_core::adapters#bytes)).
    pub const fn bytes(mut self, format: BytesFormat) -> DeserializerConfig {
        self.bytes = format;
        self
    }

    /// Enables or disables location tracking.
    ///
    /// The byte range of every field is always published into the state
    /// (see [`State::input_range`](deser_core::State::input_range)).  When
    /// enabled additionally the input is set as source (see
    /// [`Source`](deser_core::Source)).  This copies the input.
    pub const fn track_locations(mut self, yes: bool) -> DeserializerConfig {
        self.track_locations = yes;
        self
    }

    /// Deserializes the records of a string.
    ///
    /// See [`from_str`](crate::from_str).
    pub fn from_str<'de, T: Deserialize<'de>>(&self, s: &'de str) -> Result<T, Error> {
        Deserializer::from_str_with_config(s, self).deserialize()
    }

    /// Deserializes the records of a byte slice.
    ///
    /// See [`from_slice`](crate::from_slice).
    pub fn from_slice<'de, T: Deserialize<'de>>(&self, bytes: &'de [u8]) -> Result<T, Error> {
        Deserializer::from_slice_with_config(bytes, self).deserialize()
    }

    /// Returns how records are scanned.
    fn options(&self, header: bool) -> Options {
        Options {
            trim: match self.trim {
                Trim::None => false,
                Trim::Headers => header,
                Trim::Fields => !header,
                Trim::All => true,
            },
            skip_blank_lines: self.skip_blank_lines,
            lenient_quotes: self.lenient_quotes,
            max_record_len: self.max_record_len,
        }
    }

    fn dialect(&self, delimiter: u8) -> Result<Dialect, Error> {
        Dialect::new(
            delimiter,
            self.quote,
            self.double_quote,
            self.escape,
            self.terminator,
            self.comment,
        )
    }
}

/// The state of a stream of records.
///
/// This holds the names of the columns and what is needed to split the
/// records.
#[derive(Debug, Default)]
pub(crate) struct StreamState {
    // `None` before the start of the stream (BOM and `sep=` line) was read
    dialect: Option<Dialect>,
    scanner: Scanner,
    names: Option<Vec<String>>,
    // the names were read (or given, or there are none)
    has_names: bool,
    // the number of fields of the first record (without names)
    expected_len: Option<usize>,
    // decoded fields and names
    scratch: Vec<u8>,
}

impl StreamState {
    /// Creates the state of a stream which continues with the given names
    /// of the columns.
    pub(crate) fn with_headers(names: Vec<String>) -> StreamState {
        StreamState {
            names: Some(names),
            has_names: true,
            ..StreamState::default()
        }
    }

    /// Returns the names of the columns.
    pub(crate) fn headers(&self) -> Option<&[String]> {
        self.names.as_deref()
    }

    /// Finds the next record.
    ///
    /// This works like
    /// [`StreamDeserializer::frame`](deser_core::de::StreamDeserializer::frame).
    /// Blank lines, comments and names are consumed without returning a
    /// record.  The fields of the record are kept in the scanner.
    pub(crate) fn frame(
        &mut self,
        config: &DeserializerConfig,
        input: &[u8],
        eof: bool,
    ) -> Result<Frame, Error> {
        if self.dialect.is_none() {
            match self.start(config, input, eof)? {
                Frame::Incomplete { consumed: 0 } if self.dialect.is_some() => {}
                frame => return Ok(frame),
            }
        }
        let dialect = self.dialect.as_ref().unwrap();
        let header = !self.has_names;
        let options = config.options(header);
        match self.scanner.scan(dialect, input, eof, options)? {
            Scan::Record { end, consumed } => {
                if header {
                    self.read_names(input)?;
                    return Ok(Frame::Incomplete { consumed });
                }
                if self.names.is_none() && self.expected_len.is_none() {
                    self.expected_len = Some(self.scanner.fields.len());
                }
                Ok(Frame::Value {
                    start: 0,
                    end,
                    consumed,
                })
            }
            Scan::Skip { consumed } => Ok(Frame::Incomplete { consumed }),
            Scan::Incomplete => Ok(Frame::Incomplete { consumed: 0 }),
            Scan::End => Ok(Frame::End),
        }
    }

    /// Reads the start of the stream: the byte order mark and the `sep=`
    /// line.
    fn start(
        &mut self,
        config: &DeserializerConfig,
        input: &[u8],
        eof: bool,
    ) -> Result<Frame, Error> {
        const BOM: &[u8] = b"\xef\xbb\xbf";
        const SEP: &[u8] = b"sep=";
        if input.len() < BOM.len() && BOM.starts_with(input) && !eof {
            return Ok(Frame::Incomplete { consumed: 0 });
        }
        if input.starts_with(b"\xff\xfe") || input.starts_with(b"\xfe\xff") {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "input is UTF-16, only UTF-8 is supported",
            )
            .with_offset(0));
        }
        let bom = if input.starts_with(BOM) { BOM.len() } else { 0 };
        let rest = &input[bom..];
        let prefix = rest.len().min(SEP.len());
        if !config.sep_line || !rest[..prefix].eq_ignore_ascii_case(&SEP[..prefix]) {
            return self.started(config, config.delimiter, bom);
        }
        // wait for the whole line (and the LF after a CR)
        let end = match rest.iter().position(|&b| b == b'\n' || b == b'\r') {
            Some(end) if rest[end] == b'\r' && end + 1 == rest.len() && !eof => {
                return Ok(Frame::Incomplete { consumed: 0 });
            }
            Some(end) => end,
            None if eof => rest.len(),
            None => return Ok(Frame::Incomplete { consumed: 0 }),
        };
        if end != SEP.len() + 1 {
            return self.started(config, config.delimiter, bom);
        }
        let mut consumed = bom + end + 1;
        if rest.get(end) == Some(&b'\r') && rest.get(end + 1) == Some(&b'\n') {
            consumed += 1;
        }
        self.started(config, rest[SEP.len()], consumed.min(input.len()))
    }

    fn started(
        &mut self,
        config: &DeserializerConfig,
        delimiter: u8,
        consumed: usize,
    ) -> Result<Frame, Error> {
        self.dialect = Some(config.dialect(delimiter)?);
        if !self.has_names {
            match config.headers {
                Headers::First | Headers::Skip => {}
                Headers::None => self.has_names = true,
                Headers::Given(names) => {
                    self.names = Some(names.iter().map(|name| name.to_string()).collect());
                    self.has_names = true;
                }
            }
        }
        Ok(Frame::Incomplete { consumed })
    }

    /// Takes the names of the columns from the record that was scanned.
    fn read_names(&mut self, record: &[u8]) -> Result<(), Error> {
        if let Some((offset, msg)) = self.scanner.error {
            return Err(Error::new(ErrorKind::Unexpected, msg).with_offset(offset));
        }
        let dialect = self.dialect.as_ref().unwrap();
        let mut names = Vec::with_capacity(self.scanner.fields.len());
        for field in &self.scanner.fields {
            let text = &record[field.start..field.end];
            let text = if field.flags & UNESCAPE != 0 {
                unescape(dialect, text, field.flags & QUOTED != 0, &mut self.scratch);
                &self.scratch[..]
            } else {
                text
            };
            match core::str::from_utf8(text) {
                Ok(name) => names.push(name.to_string()),
                Err(_) => {
                    return Err(Error::new(ErrorKind::Unexpected, "name is not valid UTF-8")
                        .with_offset(field.span_start));
                }
            }
        }
        self.names = Some(names);
        self.has_names = true;
        Ok(())
    }

    /// Emits the record that was found last.
    ///
    /// `record` holds the bytes of the record, `base` is its offset in the
    /// input for the input ranges.
    pub(crate) fn emit_record<'de>(
        &mut self,
        config: &DeserializerConfig,
        record: &'de [u8],
        base: usize,
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        let StreamState {
            ref dialect,
            ref scanner,
            ref names,
            expected_len,
            ref mut scratch,
            ..
        } = *self;
        let self_names_len = names.as_ref().map(Vec::len);
        let dialect = dialect
            .as_ref()
            .expect("records are emitted after the start");
        if let Some((offset, msg)) = scanner.error {
            return Err(Error::new(ErrorKind::Unexpected, msg).with_offset(base + offset));
        }
        if config.bytes != BytesFormat::BASE64 {
            *driver.state_mut().get_mut::<BytesFormat>() = config.bytes;
        }
        // everything in a CSV file is text, like in a query string
        LexicalRules::LENIENT.set(driver.state_mut());
        let fields = &scanner.fields[..];
        // with `Headers::Skip` the names are known but not used
        let names = match config.headers {
            Headers::Skip => None,
            _ => names.as_ref(),
        };
        let expected = match (names, self_names_len) {
            (Some(names), _) => names.len(),
            (None, Some(len)) => len,
            (None, None) => expected_len.unwrap_or(fields.len()),
        };
        if fields.len() != expected && !config.flexible {
            return Err(Error::new(
                ErrorKind::Unexpected,
                format!(
                    "record has {} field{}, expected {}",
                    fields.len(),
                    if fields.len() == 1 { "" } else { "s" },
                    expected
                ),
            )
            .with_offset(base));
        }

        let shape = ContainerShape::new().with_len(fields.len());
        let emitter = FieldEmitter {
            dialect,
            nulls: config.nulls,
            record,
            // the special characters are ASCII, so if the record is UTF-8
            // all of its fields are
            record_is_utf8: record.is_ascii() || core::str::from_utf8(record).is_ok(),
            base,
        };
        // the start and end of the record are at its start and end (for
        // errors like missing fields)
        let end = base + record.len();
        driver.state_mut().set_input_range(base, base);
        match names {
            Some(names) => {
                // header names can repeat: records are multimaps
                driver.emit(Event::MapStart(shape.with_multimap(true)))?;
                for (index, field) in fields.iter().enumerate() {
                    emitter.set_range(driver, field);
                    match names.get(index) {
                        // the names are only valid for this call, sinks
                        // that keep them copy them
                        Some(name) => driver.emit(Atom::Lexical(Text::borrowed(name.as_str())))?,
                        None => driver.emit(Atom::Lexical(Text::owned(index.to_string())))?,
                    }
                    emitter.emit(driver, field, scratch)?;
                }
                driver.state_mut().set_input_range(end, end);
                driver.emit(Event::MapEnd)
            }
            None => {
                driver.emit(Event::SeqStart(shape))?;
                for field in fields {
                    emitter.emit(driver, field, scratch)?;
                }
                driver.state_mut().set_input_range(end, end);
                driver.emit(Event::SeqEnd)
            }
        }
    }
}

/// Emits the fields of a record.
struct FieldEmitter<'a, 'de> {
    dialect: &'a Dialect,
    nulls: Nulls,
    record: &'de [u8],
    record_is_utf8: bool,
    base: usize,
}

impl<'de> FieldEmitter<'_, 'de> {
    #[inline]
    fn set_range(&self, driver: &mut DeserializeDriver<'_, 'de>, field: &Field) {
        driver
            .state_mut()
            .set_input_range(self.base + field.span_start, self.base + field.span_end);
    }

    #[inline]
    fn emit(
        &self,
        driver: &mut DeserializeDriver<'_, 'de>,
        field: &Field,
        scratch: &mut Vec<u8>,
    ) -> Result<(), Error> {
        self.set_range(driver, field);
        let text = &self.record[field.start..field.end];
        if field.flags & QUOTED == 0 {
            match self.nulls {
                Nulls::Empty if text.is_empty() => return driver.emit(Atom::Null),
                Nulls::Text(null) if text == null.as_bytes() => return driver.emit(Atom::Null),
                _ => {}
            }
        }
        if field.flags & UNESCAPE != 0 {
            unescape(self.dialect, text, field.flags & QUOTED != 0, scratch);
            // the decoded text is only valid for the call
            match core::str::from_utf8(scratch) {
                Ok(text) => driver.emit(Atom::Lexical(Text::borrowed(text))),
                Err(_) => driver.emit(Atom::Bytes(Bytes::borrowed(scratch))),
            }
        } else if self.record_is_utf8 {
            // SAFETY: the record is UTF-8 and fields start and end at ASCII
            // characters (or the start and end of the record)
            let text = unsafe { core::str::from_utf8_unchecked(text) };
            driver.emit_borrowed(Atom::Lexical(Text::borrowed(text)))
        } else {
            match core::str::from_utf8(text) {
                Ok(text) => driver.emit_borrowed(Atom::Lexical(Text::borrowed(text))),
                Err(_) => driver.emit_borrowed(Atom::Bytes(Bytes::borrowed(text))),
            }
        }
    }
}

/// Deserializes delimited text.
///
/// Most of the time the [`from_str`](crate::from_str) and
/// [`from_slice`](crate::from_slice) functions (or the methods of the same
/// name on [`DeserializerConfig`]) are all that is needed: they deserialize
/// all records as a sequence.  The deserializer can also read one record
/// at a time (see [`records`](Self::records)) and configure the driver, for
/// instance to add layers:
///
/// ```
/// use deser_path::{Path, PathLayer};
/// use deser_csv::Deserializer;
///
/// #[derive(Debug, deser::Deserialize)]
/// struct Row {
///     name: String,
///     age: u32,
/// }
///
/// let err = Deserializer::from_str("name,age\njane,42\njohn,x\n")
///     .deserialize_with::<Vec<Row>, _>(|driver| {
///         driver.push_layer(PathLayer::new())
///     })
///     .unwrap_err();
/// assert_eq!(err.message(), "invalid value \"x\", expected u32");
/// assert_eq!(err.attachment::<Path>().unwrap().to_string(), "[1].age");
/// assert_eq!((err.line(), err.column()), (Some(3), Some(6)));
/// ```
pub struct Deserializer<'a> {
    input: &'a [u8],
    pos: usize,
    config: DeserializerConfig,
    state: StreamState,
    failed: bool,
    // the input as source (with `track_locations`)
    source: Option<Arc<str>>,
}

impl<'a> Deserializer<'a> {
    /// Creates a new deserializer for a string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(input: &'a str) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input.as_bytes(), &DeserializerConfig::new())
    }

    /// Creates a new deserializer for a string with the given configuration.
    pub fn from_str_with_config(input: &'a str, config: &DeserializerConfig) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input.as_bytes(), config)
    }

    /// Creates a new deserializer for a byte slice.
    ///
    /// Fields which are not UTF-8 are passed on as bytes.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input, &DeserializerConfig::new())
    }

    /// Creates a new deserializer for a byte slice with the given
    /// configuration.
    pub fn from_slice_with_config(
        input: &'a [u8],
        config: &DeserializerConfig,
    ) -> Deserializer<'a> {
        Deserializer {
            input,
            pos: 0,
            config: config.clone(),
            state: StreamState::default(),
            failed: false,
            source: None,
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Returns the names of the columns.
    ///
    /// They are read with the first record.
    pub fn headers(&self) -> Option<&[String]> {
        self.state.headers()
    }

    /// Returns `true` if there are no more records.
    ///
    /// This is also the case after an error that the input cannot recover
    /// from.  Blank lines and comments are not records, but they are only
    /// skipped when the next record is read.
    pub fn is_end(&self) -> bool {
        self.failed || self.pos == self.input.len()
    }

    /// Deserializes all records as a sequence.
    ///
    /// To configure the deserialization (for instance to add layers) use
    /// [`deserialize_with`](Self::deserialize_with).
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        de::Deserializer::deserialize(self)
    }

    /// Deserializes all records as a sequence with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// deserialized, for instance to add [`Layer`](deser_core::de::Layer)s.
    pub fn deserialize_with<T, F>(&mut self, setup: F) -> Result<T, Error>
    where
        T: Deserialize<'a>,
        F: FnOnce(&mut DeserializeDriver<'_, 'a>),
    {
        de::Deserializer::deserialize_with(self, setup)
    }

    /// Deserializes the next record.
    ///
    /// Returns `None` if there are no more records.  Errors of a record
    /// (like a field that does not fit the type) only discard the record,
    /// the next call continues with the next record.
    ///
    /// ```
    /// #[derive(deser::Deserialize)]
    /// struct Row {
    ///     name: String,
    ///     age: u32,
    /// }
    ///
    /// let mut de = deser_csv::Deserializer::from_str(
    ///     "name,age\njane,42\njohn,x\nmax,7\n",
    /// );
    /// assert_eq!(de.deserialize_record::<Row>().unwrap().unwrap().age, 42);
    /// assert!(de.deserialize_record::<Row>().is_err());
    /// assert_eq!(de.deserialize_record::<Row>().unwrap().unwrap().age, 7);
    /// assert!(de.deserialize_record::<Row>().unwrap().is_none());
    /// ```
    pub fn deserialize_record<T: Deserialize<'a>>(&mut self) -> Result<Option<T>, Error> {
        self.deserialize_record_with(|_| {})
    }

    /// Deserializes the next record with a configured driver.
    ///
    /// See [`deserialize_record`](Self::deserialize_record).
    pub fn deserialize_record_with<T, F>(&mut self, setup: F) -> Result<Option<T>, Error>
    where
        T: Deserialize<'a>,
        F: FnOnce(&mut DeserializeDriver<'_, 'a>),
    {
        let mut out = None;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            setup(&mut driver);
            if !self.drive_record(&mut driver)? {
                return Ok(None);
            }
        }
        out.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty record"))
            .map(Some)
    }

    /// Returns an iterator over the remaining records.
    ///
    /// Errors of records are returned and the iteration continues with the
    /// next record (see [`deserialize_record`](Self::deserialize_record)).
    pub fn records<T: Deserialize<'a>>(&mut self) -> Records<'_, 'a, T> {
        Records {
            de: self,
            _marker: PhantomData,
        }
    }

    /// Feeds the events of the next record into the given driver.
    ///
    /// Returns `false` if there are no more records.
    pub fn drive_record(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<bool, Error> {
        let rv = self.drive_record_impl(driver);
        rv.map_err(|err| err.resolve_position(self.input))
    }

    fn drive_record_impl(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<bool, Error> {
        let Some((start, end)) = self.next_record()? else {
            return Ok(false);
        };
        let input = self.input;
        self.set_source(driver);
        self.state
            .emit_record(&self.config, &input[start..end], start, driver)?;
        Ok(true)
    }

    /// Sets the input as source (with `track_locations`).
    fn set_source(&mut self, driver: &mut DeserializeDriver<'_, 'a>) {
        if self.config.track_locations {
            let input = self.input;
            let source = self
                .source
                .get_or_insert_with(|| String::from_utf8_lossy(input).into());
            Source::set(driver.state_mut(), source.clone());
        }
    }

    /// Finds the next record and returns its range.
    fn next_record(&mut self) -> Result<Option<(usize, usize)>, Error> {
        if self.failed {
            return Ok(None);
        }
        loop {
            let input = &self.input[self.pos..];
            let frame = match self.state.frame(&self.config, input, true) {
                Ok(frame) => frame,
                Err(err) => {
                    // errors of the structure end the input
                    self.failed = true;
                    return Err(err.shift_offset(self.pos));
                }
            };
            match frame {
                Frame::Value {
                    start,
                    end,
                    consumed,
                } => {
                    let range = (self.pos + start, self.pos + end);
                    self.pos += consumed;
                    return Ok(Some(range));
                }
                Frame::Incomplete { consumed } => self.pos += consumed,
                Frame::End => return Ok(None),
            }
        }
    }

    /// Parses the input and feeds all records as a sequence into the given
    /// driver.
    ///
    /// Fields that do not need to be decoded are passed on borrowed from
    /// the input (see
    /// [`emit_borrowed`](DeserializeDriver::emit_borrowed)).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        let rv = self.drive_impl(driver);
        rv.map_err(|err| err.resolve_position(self.input))
    }

    fn drive_impl(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        self.set_source(driver);
        let input = self.input;
        driver.emit(Event::seq_start())?;
        while let Some((start, end)) = self.next_record()? {
            self.state
                .emit_record(&self.config, &input[start..end], start, driver)?;
        }
        driver.emit(Event::SeqEnd)
    }
}

impl<'a> de::Deserializer<'a> for Deserializer<'a> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        Deserializer::drive(self, driver)
    }
}

/// An iterator over the records of a [`Deserializer`].
///
/// See [`Deserializer::records`].
pub struct Records<'d, 'a, T> {
    de: &'d mut Deserializer<'a>,
    _marker: PhantomData<fn() -> T>,
}

impl<'a, T: Deserialize<'a>> Iterator for Records<'_, 'a, T> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.de.deserialize_record().transpose()
    }
}

/// Moves the offset of an error.
trait ShiftOffset {
    fn shift_offset(self, base: usize) -> Self;
}

impl ShiftOffset for Error {
    fn shift_offset(self, base: usize) -> Error {
        match self.offset() {
            Some(offset) => self.with_offset(base + offset),
            None => self,
        }
    }
}
