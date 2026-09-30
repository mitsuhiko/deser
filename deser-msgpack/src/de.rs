use core::marker::PhantomData;

use deser_core::Error;
use deser_core::de::{self, Deserialize, DeserializeDriver, deserialize_value};

use crate::parser::{Borrowing, Parser, Progress, syntax_error};

/// Configures how MessagePack is deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The method
/// [`from_slice`](Self::from_slice) works like the function of the same
/// name.  To read multiple items with the configuration create a
/// [`Deserializer`] with [`Deserializer::from_slice_with_config`].
///
/// ```
/// use deser_msgpack::DeserializerConfig;
///
/// const CONFIG: DeserializerConfig = DeserializerConfig::new();
/// assert_eq!(CONFIG.from_slice::<Vec<u32>>(&[0x92, 0x01, 0x02]).unwrap(), [1, 2]);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeserializerConfig {
    // there are no options yet
    _private: (),
}

impl DeserializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> DeserializerConfig {
        DeserializerConfig { _private: () }
    }

    /// Deserializes a value from MessagePack.
    ///
    /// See [`from_slice`](crate::from_slice).
    pub fn from_slice<'de, T: Deserialize<'de>>(&self, input: &'de [u8]) -> Result<T, Error> {
        deserialize_value(|driver| self.drive_slice(input, driver))
    }

    /// The part of [`from_slice`](Self::from_slice) that does not depend on
    /// the type of the value, it exists once.
    fn drive_slice<'de>(
        &self,
        input: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        let mut deserializer = Deserializer::from_slice_with_config(input, self);
        de::Deserializer::drive(&mut deserializer, driver)?;
        deserializer.end()
    }
}

/// Deserializes a deserializable from MessagePack.
///
/// A deserializer reads items from a slice.  Because MessagePack streams are
/// just items following each other, a deserializer can be used to read more
/// than one item:
///
/// ```
/// use deser_msgpack::Deserializer;
///
/// let mut de = Deserializer::from_slice(&[0x01, 0xa2, b'h', b'i']);
/// assert_eq!(de.deserialize::<u32>().unwrap(), 1);
/// assert_eq!(de.deserialize::<String>().unwrap(), "hi");
/// assert!(de.is_end());
/// ```
///
/// To deserialize a single item, use [`from_slice`](crate::from_slice)
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

    /// Deserializes the next item.
    ///
    /// This does not check if there is more data after the item.  Use
    /// [`end`](Self::end) for this or [`from_slice`] which does it
    /// automatically.
    ///
    /// To configure the deserialization (for instance to add layers) use
    /// [`deserialize_with`](Self::deserialize_with).
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        de::Deserializer::deserialize(self)
    }

    /// Deserializes the next value with a configured driver.
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

    /// Returns an iterator over the remaining items.
    ///
    /// This is useful to read streams of MessagePack items.  The iterator stops after
    /// the first error.
    ///
    /// ```
    /// let mut de = deser_msgpack::Deserializer::from_slice(&[0x01, 0x02, 0x03]);
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

    /// Parses the next item and feeds the events into the given driver.
    ///
    /// This is useful to deserialize into a custom
    /// [`Sink`](deser_core::de::Sink) or to wrap the sink of a value.
    ///
    /// Strings and binary data are passed on borrowed from the input (see
    /// [`emit_borrowed`](DeserializeDriver::emit_borrowed)).  The byte
    /// ranges of the items are published as input ranges (see
    /// [`State::input_range`](deser_core::State::input_range)) and errors carry
    /// the offset in the input (see [`Error::offset`]).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
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

/// An iterator over the items of a MessagePack stream.
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

impl<'a> de::Deserializer<'a> for Deserializer<'a> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        Deserializer::drive(self, driver)
    }
}

/// Deserializes a value from MessagePack.
///
/// The input must contain exactly one item.  This uses the default
/// [`DeserializerConfig`].
pub fn from_slice<'de, T: Deserialize<'de>>(input: &'de [u8]) -> Result<T, Error> {
    DeserializerConfig::new().from_slice(input)
}
