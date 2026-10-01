use alloc::sync::Arc;

use deser_core::de::{self, Deserialize, DeserializeDriver, LexicalRules, deserialize_value};
use deser_core::{Error, ErrorKind, Source};

use crate::common::{Borrowing, Copying, Out, decode_utf16_text, syntax_error};
use crate::format::Format;
use crate::{read_ascii, read_binary, read_xml};

/// Configures how property lists are deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The method
/// [`from_slice`](Self::from_slice) works like the function of the same
/// name.
///
/// ```
/// use deser_plist::DeserializerConfig;
///
/// const CONFIG: DeserializerConfig = DeserializerConfig::new().track_locations(true);
/// assert_eq!(CONFIG.from_slice::<Vec<u32>>(b"(1, 2)").unwrap(), [1, 2]);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeserializerConfig {
    track_locations: bool,
}

impl DeserializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> DeserializerConfig {
        DeserializerConfig {
            track_locations: false,
        }
    }

    /// Enables or disables location tracking for the text formats.
    ///
    /// The byte range of every event is always published into the state
    /// (see [`State::input_range`](deser_core::State::input_range)).  When
    /// enabled additionally the input of XML and OpenStep property lists
    /// is set as source (see [`Source`]) which allows resolving the ranges
    /// into lines and columns, for instance with the `Spanned` type of
    /// [`deser-location`](https://docs.rs/deser-location).  This copies
    /// the input.
    pub const fn track_locations(mut self, yes: bool) -> DeserializerConfig {
        self.track_locations = yes;
        self
    }

    /// Deserializes a value from a property list.
    ///
    /// See [`from_slice`].
    pub fn from_slice<'de, T: Deserialize<'de>>(&self, input: &'de [u8]) -> Result<T, Error> {
        deserialize_value(|driver| self.drive_slice(input, driver))
    }

    /// The part of [`from_slice`](Self::from_slice) that does not depend on the type
    /// of the value, it exists once.
    fn drive_slice<'de>(
        &self,
        input: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        de::Deserializer::drive(
            &mut Deserializer::from_slice_with_config(input, self),
            driver,
        )
    }
}

/// Deserializes a property list.
///
/// The format of the input (binary, XML or OpenStep) is detected
/// automatically (see [`Format::detect`]).  Most of the time the
/// [`from_slice`] function (or the method of the same
/// name on [`DeserializerConfig`]) is all that is needed.  The
/// deserializer is useful to [`drive`](Self::drive) a custom sink.
///
/// ```
/// use deser_plist::{Deserializer, Format};
///
/// let mut de = Deserializer::from_slice(b"<plist><integer>42</integer></plist>");
/// assert_eq!(de.format(), Format::Xml);
/// assert_eq!(de.deserialize::<u32>().unwrap(), 42);
/// ```
pub struct Deserializer<'a> {
    input: &'a [u8],
    format: Format,
    config: DeserializerConfig,
}

impl<'a> Deserializer<'a> {
    /// Creates a new deserializer for a byte slice.
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
            format: Format::detect(input),
            config: config.clone(),
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Returns the detected format of the input.
    pub fn format(&self) -> Format {
        self.format
    }

    /// Deserializes the property list.
    ///
    /// To configure the deserialization (for instance to add layers) use
    /// [`deserialize_with`](Self::deserialize_with).
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        de::Deserializer::deserialize(self)
    }

    /// Deserializes the property list with a configured driver.
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

    /// Parses the input and feeds the events into the given driver.
    ///
    /// Strings and data are passed on borrowed from the input where
    /// possible (see [`emit_borrowed`](DeserializeDriver::emit_borrowed)).
    /// The byte ranges of the values are published as input ranges (see
    /// [`State::input_range`](deser_core::State::input_range)).  Errors
    /// carry the offset in the input (see [`Error::offset`]), for the text
    /// formats also the line and column.
    ///
    /// Text in UTF-16 (with a byte order mark) is converted first.  Its
    /// values are not borrowed and the offsets refer to the converted
    /// UTF-8 text.
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if self.format == Format::Binary {
            return read_binary::parse(self.input, &mut Borrowing(driver));
        }

        if let Some(text) = decode_utf16_text(self.input) {
            let text = text.map_err(|offset| syntax_error(offset, "invalid UTF-16"))?;
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            return self.drive_text(text, &mut Copying(driver));
        }
        let input = self
            .input
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(self.input);
        let text = core::str::from_utf8(input).map_err(|err| {
            let offset = self.input.len() - input.len() + err.valid_up_to();
            Error::new(ErrorKind::Unexpected, "invalid UTF-8").with_offset(offset)
        })?;
        self.drive_text(text, &mut Borrowing(driver))
    }

    fn drive_text<'i, O: Out<'i>>(&self, text: &'i str, out: &mut O) -> Result<(), Error> {
        if self.config.track_locations {
            Source(Arc::<str>::from(text)).set(out.state_mut());
        }
        let rv = if self.format == Format::Xml {
            read_xml::parse(text, out)
        } else {
            // everything is text, `YES` and `NO` are booleans
            LexicalRules::STRICT
                .with_lenient_bools(true)
                .set(out.state_mut());
            read_ascii::parse(text, out)
        };
        rv.map_err(|err| err.resolve_position(text.as_bytes()))
    }
}

impl<'a> de::Deserializer<'a> for Deserializer<'a> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        Deserializer::drive(self, driver)
    }
}

/// Deserializes a value from a property list.
///
/// The format is detected automatically: binary, XML and OpenStep
/// property lists are supported.  This uses the default
/// [`DeserializerConfig`].
///
/// ```
/// use std::collections::BTreeMap;
///
/// let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
/// <plist version="1.0">
/// <dict>
///     <key>answer</key>
///     <integer>42</integer>
/// </dict>
/// </plist>"#;
/// let value: BTreeMap<String, u32> = deser_plist::from_slice(xml).unwrap();
/// assert_eq!(value["answer"], 42);
///
/// let value: BTreeMap<String, u32> =
///     deser_plist::from_slice(b"{ answer = 42; }").unwrap();
/// assert_eq!(value["answer"], 42);
/// ```
pub fn from_slice<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T, Error> {
    DeserializerConfig::new().from_slice(input)
}
