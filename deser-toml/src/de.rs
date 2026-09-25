use std::borrow::Cow;

use deser::de::{Deserialize, DeserializeDriver};
use deser::ext::ExtValue;
use deser::{Atom, Error, ErrorKind, Event};

use crate::document::{Document, Item, Span, Value};
use crate::parser::{parse, ROOT};

/// Deserializes TOML.
///
/// ```
/// use deser_toml::Deserializer;
/// use std::collections::BTreeMap;
///
/// let mut de = Deserializer::new("a = 1\nb = 2");
/// let value: BTreeMap<String, u32> = de.deserialize().unwrap();
/// assert_eq!(value["b"], 2);
/// ```
pub struct Deserializer<'a> {
    input: &'a str,
    /// An error that is reported instead of parsing (invalid UTF-8).
    error: Option<Error>,
    #[cfg(feature = "locations")]
    track_locations: bool,
}

impl<'a> Deserializer<'a> {
    /// Creates a new deserializer.
    pub fn new(input: &'a str) -> Deserializer<'a> {
        Deserializer {
            input,
            error: None,
            #[cfg(feature = "locations")]
            track_locations: false,
        }
    }

    /// Creates a new deserializer for a byte slice.
    ///
    /// The input must be UTF-8, otherwise deserializing fails.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        match str_from_utf8(input) {
            Ok(input) => Deserializer::new(input),
            Err(err) => {
                let mut rv = Deserializer::new("");
                rv.error = Some(err);
                rv
            }
        }
    }

    /// Enables or disables location tracking.
    ///
    /// The byte range of every event is always published into the state
    /// (see [`State::input_range`](deser::State::input_range)).  When
    /// enabled additionally a source map is installed as
    /// [`Locations`](deser_location::Locations) which resolves the ranges
    /// into lines and columns.  Types like
    /// [`Spanned`](deser_location::Spanned) can then pick them up.
    ///
    /// Tables report the location of the header that defines them (the
    /// whole document for the root table), tables created by dotted keys
    /// report the location of the key.  Arrays of tables report the
    /// location of their first header.
    #[cfg(feature = "locations")]
    pub fn track_locations(mut self, yes: bool) -> Deserializer<'a> {
        self.track_locations = yes;
        self
    }

    /// Deserializes the document.
    pub fn deserialize<T: Deserialize>(&mut self) -> Result<T, Error> {
        let mut out = None;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            self.drive(&mut driver)?;
        }
        out.take()
            .ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))
    }

    /// Parses the input and feeds the events into the given driver.
    ///
    /// The whole document is parsed before the first event is emitted, so
    /// syntax errors are reported before any value is deserialized.
    pub fn drive(&mut self, driver: &mut DeserializeDriver) -> Result<(), Error> {
        if let Some(err) = self.error.take() {
            return Err(err);
        }
        let doc = parse(self.input)?;

        #[cfg(feature = "locations")]
        if self.track_locations {
            deser_location::Locations::set_source_map(
                driver.state_mut(),
                std::sync::Arc::new(deser_location::SourceMap::new(self.input)),
            );
        }
        emit(&doc, driver)
    }
}

/// Emits an event with the byte range of its span.
#[inline(always)]
fn emit_at<'e, E: Into<Event<'e>>>(
    driver: &mut DeserializeDriver,
    event: E,
    span: Span,
) -> Result<(), Error> {
    driver.emit_at(event, span.start, span.end)
}

/// A container whose events are emitted, with the index of the next child.
enum Frame {
    Table(usize, usize),
    Array(usize, usize),
}

/// Emits the events of a document.
fn emit(doc: &Document, driver: &mut DeserializeDriver) -> Result<(), Error> {
    let mut stack = vec![Frame::Table(ROOT, 0)];
    emit_at(driver, Event::MapStart, doc.tables[ROOT].span)?;

    while let Some(frame) = stack.last_mut() {
        let item: &Item = match *frame {
            Frame::Table(id, ref mut index) => {
                let table = &doc.tables[id];
                match table.entries.get(*index) {
                    Some(entry) => {
                        *index += 1;
                        emit_at(driver, Atom::Str(Cow::Borrowed(&entry.key)), entry.key_span)?;
                        &entry.item
                    }
                    None => {
                        stack.pop();
                        emit_at(driver, Event::MapEnd, table.span)?;
                        continue;
                    }
                }
            }
            Frame::Array(id, ref mut index) => {
                let array = &doc.arrays[id];
                match array.items.get(*index) {
                    Some(item) => {
                        *index += 1;
                        item
                    }
                    None => {
                        stack.pop();
                        emit_at(driver, Event::SeqEnd, array.span)?;
                        continue;
                    }
                }
            }
        };

        match item.value {
            Value::Table(id) => {
                emit_at(driver, Event::MapStart, doc.tables[id].span)?;
                stack.push(Frame::Table(id, 0));
            }
            Value::Array(id) => {
                emit_at(driver, Event::SeqStart, doc.arrays[id].span)?;
                stack.push(Frame::Array(id, 0));
            }
            ref scalar => {
                let atom = match *scalar {
                    Value::Str(ref value) => Atom::Str(Cow::Borrowed(value)),
                    Value::Int(value) if value >= 0 => Atom::U64(value as u64),
                    Value::Int(value) => Atom::I64(value),
                    Value::UInt(value) => Atom::U64(value),
                    Value::Float(value) => Atom::F64(value),
                    Value::Bool(value) => Atom::Bool(value),
                    Value::Datetime(ref value) => Atom::Ext(ExtValue::borrowed(value)),
                    Value::Table(_) | Value::Array(_) => unreachable!(),
                };
                emit_at(driver, atom, item.span)?;
            }
        }
    }

    Ok(())
}

fn str_from_utf8(bytes: &[u8]) -> Result<&str, Error> {
    #[cfg(feature = "simdutf8")]
    {
        if simdutf8::basic::from_utf8(bytes).is_ok() {
            // SAFETY: validated above
            return Ok(unsafe { std::str::from_utf8_unchecked(bytes) });
        }
    }
    std::str::from_utf8(bytes).map_err(|err| {
        Error::new(
            ErrorKind::Unexpected,
            format!("invalid UTF-8 at offset {}", err.valid_up_to()),
        )
    })
}

/// Deserializes a value from TOML.
///
/// A TOML document is a table, so the value has to be deserializable from
/// a map (such as a struct or a map type).
pub fn from_str<T: Deserialize>(s: &str) -> Result<T, Error> {
    Deserializer::new(s).deserialize()
}

/// Deserializes a value from TOML in a byte slice.
///
/// The input must be UTF-8.  Otherwise this works like [`from_str`].
pub fn from_slice<T: Deserialize>(bytes: &[u8]) -> Result<T, Error> {
    Deserializer::from_slice(bytes).deserialize()
}
