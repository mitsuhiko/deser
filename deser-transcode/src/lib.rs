//! Converts values from one data format to another.
//!
//! Every [`Deserializer`] can be transcoded into every [`Serializer`],
//! without types in between and without code that is specific to the
//! formats:
//!
//! ```rust
//! let mut de = deser_json::Deserializer::from_str(r#"{"name": "deser", "tags": ["a", "b"]}"#);
//! let mut ser = deser_yaml::Serializer::new();
//! deser_transcode::transcode(&mut de, &mut ser).unwrap();
//! assert_eq!(ser.finish(), "name: deser\ntags:\n  - a\n  - b\n");
//! ```
//!
//! # How it Works
//!
//! A deserializer parses a value and pushes its events into a
//! [`DeserializeDriver`], a serializer receives the events of a value
//! from a [`SerializeDriver`].  Both are in control of their loop, so a
//! value is recorded into a [`RecordBuf`] and then serialized from it.
//! Only one value is held at a time, and strings which the deserializer
//! lends from the input (like the strings without escape sequences in
//! JSON) are not copied.  Recording also makes the length of every map and
//! sequence known before it's serialized, even if the input format did not
//! say, so formats that write lengths upfront (such as CBOR and
//! MessagePack) do not have to fix them up afterwards.
//!
//! The events are passed on as the deserializer emitted them, together
//! with the data attached to them (such as CBOR tags).  Each serializer
//! then does with them what it does with any value, there are no rules
//! specific to transcoding:
//!
//! * values whose type the input format infers from their text (like
//!   `true` or `1.0` in YAML) are written as the type they were inferred
//!   as, text that is text in the input format (like the values of query
//!   strings) stays text.
//! * keys that are not strings are written the way the target format
//!   writes such keys (JSON writes `1` as `"1"`), keys that the target
//!   format cannot express (such as sequences in JSON) are an error.
//! * values that the target format cannot express use its fallback (bytes
//!   are base64 in text formats, TOML skips map entries that are null) or
//!   are an error (TOML documents must be tables).
//! * duplicate keys are passed on, it's up to the target format what it
//!   does with them.
//!
//! # Streams
//!
//! Every call transcodes a single value.  What follows the value in the
//! input depends on the configuration of the deserializer, for a stream of
//! values (like JSON Lines or YAML documents) transcode until the
//! deserializer is at its end.  A [`Transcoder`] reuses its buffer for all
//! values:
//!
//! ```rust
//! use deser_json::{DeserializerConfig, Trailing};
//! use deser_transcode::Transcoder;
//!
//! let config = DeserializerConfig::new().trailing(Trailing::Newline);
//! let mut de = deser_json::Deserializer::from_str_with_config(
//!     "{\"a\": 1}\n{\"a\": 2}\n",
//!     &config,
//! );
//! let mut ser = deser_yaml::Serializer::new();
//! let mut transcoder = Transcoder::new();
//! while !de.is_end() {
//!     transcoder.transcode(&mut de, &mut ser).unwrap();
//! }
//! assert_eq!(ser.finish(), "a: 1\n---\na: 2\n");
//! ```
//!
//! Readers and writers of `deser::io` are deserializers and serializers
//! too, so values can be transcoded from one stream to another (for
//! instance from a file to standard output) without holding more than one
//! value in memory:
//!
//! ```rust
//! use deser_json::{DeserializerConfig, Trailing};
//! use deser_transcode::Transcoder;
//!
//! let config = DeserializerConfig::new().trailing(Trailing::Newline);
//! let input = &b"{\"a\": 1}\n{\"a\": 2}\n"[..];
//! let mut de = config.reader(input);
//! let mut ser = deser_yaml::SerializerConfig::new().writer(Vec::new());
//! let mut transcoder = Transcoder::new();
//! while !de.is_end().unwrap() {
//!     transcoder.transcode(&mut de, &mut ser).unwrap();
//! }
//! assert_eq!(ser.into_inner(), b"a: 1\n---\na: 2\n");
//! ```
//!
//! Values that are read from streams cannot borrow from the input, strings
//! are copied into the buffer of the transcoder.
//!
//! # Layers
//!
//! Layers can be added to both sides (see [`transcode_with`]), for
//! instance to limit what is accepted from the input.  Errors of the
//! deserializer carry the location in the input as usual.
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]
#![no_std]

use deser_core::de::{DeserializeDriver, Deserializer, RecordBuf};
use deser_core::ser::{SerializeDriver, Serializer};
use deser_core::{Error, ErrorKind};

/// Transcodes a value from a deserializer into a serializer.
///
/// This reads the next value of the deserializer and serializes it.  To
/// transcode many values, [`Transcoder`] reuses the buffer.
pub fn transcode<'de, D, S>(de: &mut D, ser: &mut S) -> Result<(), Error>
where
    D: Deserializer<'de> + ?Sized,
    S: Serializer + ?Sized,
{
    Transcoder::new().transcode(de, ser)
}

/// Transcodes a value with configured drivers.
///
/// The callbacks are invoked with the drivers before the value is
/// deserialized and serialized, for instance to add layers:
///
/// ```rust
/// use deser::de::Limits;
///
/// let mut de = deser_json::Deserializer::from_str("[[[1]]]");
/// let mut ser = deser_yaml::Serializer::new();
/// let err = deser_transcode::transcode_with(
///     &mut de,
///     &mut ser,
///     |driver| driver.push_layer(Limits::new().max_depth(2)),
///     |_driver| {},
/// )
/// .unwrap_err();
/// assert_eq!(
///     err.to_string(),
///     "LimitExceeded: recursion limit exceeded at line 1 column 3"
/// );
/// ```
pub fn transcode_with<'de, D, S, DF, SF>(
    de: &mut D,
    ser: &mut S,
    de_setup: DF,
    ser_setup: SF,
) -> Result<(), Error>
where
    D: Deserializer<'de> + ?Sized,
    S: Serializer + ?Sized,
    DF: FnOnce(&mut DeserializeDriver<'_, 'de>),
    SF: FnOnce(&mut SerializeDriver<'_>),
{
    Transcoder::new().transcode_with(de, ser, de_setup, ser_setup)
}

/// Transcodes values and reuses its buffer.
///
/// This is useful to transcode many values, see [`transcode`] and
/// [`transcode_with`] for what it does.  The transcoder can be used for all
/// values of an input, the buffer it holds borrows from it.
#[derive(Debug, Default)]
pub struct Transcoder<'de> {
    buf: RecordBuf<'de>,
}

impl<'de> Transcoder<'de> {
    /// Creates a new transcoder.
    pub fn new() -> Transcoder<'de> {
        Transcoder::default()
    }

    /// Transcodes a value from a deserializer into a serializer.
    pub fn transcode<D, S>(&mut self, de: &mut D, ser: &mut S) -> Result<(), Error>
    where
        D: Deserializer<'de> + ?Sized,
        S: Serializer + ?Sized,
    {
        self.transcode_with(de, ser, |_| {}, |_| {})
    }

    /// Transcodes a value with configured drivers.
    ///
    /// See [`transcode_with`].
    pub fn transcode_with<D, S, DF, SF>(
        &mut self,
        de: &mut D,
        ser: &mut S,
        de_setup: DF,
        ser_setup: SF,
    ) -> Result<(), Error>
    where
        D: Deserializer<'de> + ?Sized,
        S: Serializer + ?Sized,
        DF: FnOnce(&mut DeserializeDriver<'_, 'de>),
        SF: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut de_setup = Some(de_setup);
        self.record(de, &mut |driver| {
            if let Some(setup) = de_setup.take() {
                setup(driver);
            }
        })?;
        let mut ser_setup = Some(ser_setup);
        self.replay(ser, &mut |driver| {
            if let Some(setup) = ser_setup.take() {
                setup(driver);
            }
        })
    }

    /// Records the next value of the deserializer.
    ///
    /// This is not generic over the setup so that it exists once per
    /// deserializer.
    fn record<D>(
        &mut self,
        de: &mut D,
        setup: &mut dyn FnMut(&mut DeserializeDriver<'_, 'de>),
    ) -> Result<(), Error>
    where
        D: Deserializer<'de> + ?Sized,
    {
        {
            let mut driver = DeserializeDriver::from_fn(|state| self.buf.recorder(state));
            setup(&mut driver);
            de.drive(&mut driver)?;
        }
        if self.buf.is_empty() {
            return Err(Error::new(ErrorKind::EndOfFile, "empty input"));
        }
        Ok(())
    }

    /// Serializes the recorded value.
    fn replay<S>(
        &mut self,
        ser: &mut S,
        setup: &mut dyn FnMut(&mut SerializeDriver<'_>),
    ) -> Result<(), Error>
    where
        S: Serializer + ?Sized,
    {
        let mut driver = SerializeDriver::new(&self.buf);
        setup(&mut driver);
        ser.drive(&mut driver)
    }
}
