use std::marker::PhantomData;

use deser::Error;
use deser::de::{Deserialize, DeserializeDriver, Format, Limits};

use crate::parser::{Borrowing, Parser, Progress, syntax_error};

/// Configures how CBOR is deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The method
/// [`from_slice`](Self::from_slice) works like the function of the same
/// name.  To read multiple data items with the configuration create a
/// [`Deserializer`] with [`Deserializer::from_slice_with_config`].
///
/// ```
/// use deser_cbor::DeserializerConfig;
///
/// const CONFIG: DeserializerConfig = DeserializerConfig::new().max_depth(1);
/// assert!(CONFIG.from_slice::<Vec<u32>>(&[0x81, 0x01]).is_ok());
/// assert!(CONFIG.from_slice::<Vec<Vec<u32>>>(&[0x81, 0x81, 0x01]).is_err());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeserializerConfig {
    max_depth: Option<usize>,
}

impl DeserializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> DeserializerConfig {
        DeserializerConfig { max_depth: None }
    }

    /// Limits the nesting depth of arrays and maps.
    ///
    /// Deser does not use the stack to process nested data so arbitrarily
    /// deep structures do not overflow the stack.  Still it can be useful to
    /// limit the depth of untrusted inputs.  By default the depth is not
    /// limited.  This adds a [`Limits`] layer to the driver, which can also
    /// limit other aspects of the input.
    pub const fn max_depth(mut self, depth: usize) -> DeserializerConfig {
        self.max_depth = Some(depth);
        self
    }

    /// Returns the maximum depth.
    pub(crate) fn max_depth_limit(&self) -> Option<usize> {
        self.max_depth
    }

    /// Deserializes a value from CBOR.
    ///
    /// See [`from_slice`](crate::from_slice).
    pub fn from_slice<'de, T: Deserialize<'de>>(&self, input: &'de [u8]) -> Result<T, Error> {
        let mut de = Deserializer::from_slice_with_config(input, self);
        let rv = de.deserialize()?;
        de.end()?;
        Ok(rv)
    }
}

/// Deserializes a deserializable from CBOR.
///
/// A deserializer reads data items from a slice.  Because CBOR sequences
/// (RFC 8742) are just data items following each other, a deserializer can
/// be used to read more than one item:
///
/// ```
/// use deser_cbor::Deserializer;
///
/// let mut de = Deserializer::from_slice(&[0x01, 0x62, b'h', b'i']);
/// assert_eq!(de.deserialize::<u32>().unwrap(), 1);
/// assert_eq!(de.deserialize::<String>().unwrap(), "hi");
/// assert!(de.is_end());
/// ```
///
/// To deserialize a single data item, use [`from_slice`](crate::from_slice)
/// (or the method of the same name on [`DeserializerConfig`]).
pub struct Deserializer<'a> {
    input: &'a [u8],
    pos: usize,
    config: DeserializerConfig,
    parser: Parser,
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
            pos: 0,
            config: config.clone(),
            parser: Parser::default(),
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Returns the current offset in the input.
    pub fn offset(&self) -> usize {
        self.pos
    }

    /// Returns `true` if the entire input was consumed.
    pub fn is_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    /// Fails if the input was not consumed entirely.
    pub fn end(&self) -> Result<(), Error> {
        if self.is_end() {
            Ok(())
        } else {
            Err(syntax_error(self.pos, "trailing data after item"))
        }
    }

    /// Deserializes the next data item.
    ///
    /// This does not check if there is more data after the item.  Use
    /// [`end`](Self::end) for this or [`from_slice`] which does it
    /// automatically.
    ///
    /// To configure the deserialization (for instance to add layers) use
    /// [`Format::deserialize_with`].
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        Format::deserialize(self)
    }

    /// Returns an iterator over the remaining data items.
    ///
    /// This is useful to read CBOR sequences.  The iterator stops after
    /// the first error.
    ///
    /// ```
    /// let mut de = deser_cbor::Deserializer::from_slice(&[0x01, 0x02, 0x03]);
    /// let items = de.iter::<u32>().collect::<Result<Vec<_>, _>>().unwrap();
    /// assert_eq!(items, [1, 2, 3]);
    /// ```
    pub fn iter<T: Deserialize<'a>>(&mut self) -> Iter<'_, 'a, T> {
        Iter {
            de: self,
            failed: false,
            _marker: PhantomData,
        }
    }

    /// Parses the next data item and feeds the events into the given driver.
    ///
    /// This is useful to deserialize into a custom
    /// [`Sink`](deser::de::Sink) or to wrap the sink of a value.
    ///
    /// Definite length strings and byte strings are passed on borrowed from
    /// the input (see
    /// [`emit_borrowed`](DeserializeDriver::emit_borrowed)).  The byte
    /// ranges of the data items are published as input ranges (see
    /// [`State::input_range`](deser::State::input_range)) and errors carry
    /// the offset in the input (see [`Error::offset`]).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if let Some(max_depth) = self.config.max_depth {
            driver.push_layer(Limits::new().max_depth(max_depth));
        }
        self.drive_impl(driver)
    }

    fn drive_impl(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        match self
            .parser
            .parse(self.input, self.pos, true, 0, &mut Borrowing(driver))
        {
            Ok(Progress::Done(pos)) => {
                self.pos = pos;
                Ok(())
            }
            Ok(Progress::NeedMore(_)) => unreachable!("the input is complete"),
            Err(err) => {
                // the next item is read from where the parser stopped
                self.pos = self.parser.position();
                self.parser.reset();
                Err(err)
            }
        }
    }
}

/// An iterator over the data items of a CBOR sequence.
///
/// See [`Deserializer::iter`].
pub struct Iter<'b, 'a, T> {
    de: &'b mut Deserializer<'a>,
    failed: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<'b, 'a, T: Deserialize<'a>> Iterator for Iter<'b, 'a, T> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.de.is_end() {
            return None;
        }
        let rv = self.de.deserialize();
        self.failed = rv.is_err();
        Some(rv)
    }
}

impl<'a> Format<'a> for Deserializer<'a> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        Deserializer::drive(self, driver)
    }
}

/// Deserializes a value from CBOR.
///
/// The input must contain exactly one data item.  This uses the default
/// [`DeserializerConfig`].
pub fn from_slice<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T, Error> {
    DeserializerConfig::new().from_slice(input)
}
