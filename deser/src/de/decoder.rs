use std::io::Read;

use crate::de::{Deserialize, DeserializeDriver, DeserializeOwned};
use crate::error::{Error, ErrorKind};

/// The result of [`Decoder::frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frame {
    /// The next value is complete.
    ///
    /// Its bytes are `input[start..end]`.  Afterwards the first `consumed`
    /// bytes of the input (at least up to `end`) are discarded, the bytes
    /// before `start` are skipped.
    Value {
        start: usize,
        end: usize,
        consumed: usize,
    },
    /// The input does not contain a complete value.
    ///
    /// The first `consumed` bytes of the input are discarded, for instance
    /// whitespace before the next value.  If bytes were consumed, the
    /// decoder is invoked again right away (as a value might follow them),
    /// otherwise once more input was read.  At the end of the input the
    /// decoder must not return this without consuming bytes.
    Incomplete { consumed: usize },
    /// There are no more values.
    ///
    /// This must only be returned at the end of the input.
    End,
}

/// The result of [`Decoder::feed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Progress {
    /// The value is complete, it used the first `consumed` bytes of the
    /// input.
    Done { consumed: usize },
    /// The value needs more input.  The first `consumed` bytes of the input
    /// were used and are discarded, the next call continues with the input
    /// after them (followed by the new data).
    NeedMore { consumed: usize },
    /// There are no more values.
    ///
    /// This must only be returned at the end of the input.
    End,
}

/// A data format that deserializes values from bytes.
///
/// This is implemented by the deserializer configurations of the data
/// formats (for instance `deser_json::DeserializerConfig`), which makes them
/// usable in generic code.  A decoder deserializes values from slices (see
/// [`from_slice`](Self::from_slice)) and from streams (see
/// [`from_reader`](Self::from_reader) and [`io`](crate::io)).
///
/// To read streams, a decoder splits the input into the frames of values
/// with [`frame`](Self::frame) and deserializes a value once its frame is
/// complete with [`drive`](Self::drive).  Formats which can deserialize a
/// value while its input arrives additionally implement
/// [`feed`](Self::feed), which only needs to buffer incomplete tokens.
///
/// Types which produce the events of a value from something else than bytes
/// implement [`Format`](crate::de::Format) instead.
pub trait Decoder {
    /// The state of a stream.
    ///
    /// This holds the progress of reading a stream, for instance how far
    /// the input was scanned.  Every stream starts with the default state.
    type State: Default;

    /// Finds the next value in the input.
    ///
    /// The input holds the data that was read so far (minus the data that
    /// was discarded).  If it does not contain a complete value yet,
    /// [`Frame::Incomplete`] is returned and the method is invoked again
    /// once more data was read: the input then starts after the bytes that
    /// were consumed and continues with the new data.  This allows decoders
    /// to keep the progress of their scan in the state so they do not have
    /// to scan the input again.  `eof` is `true` if no more data follows the
    /// input.
    ///
    /// Once a value is complete, [`Frame::Value`] is returned and the value
    /// is deserialized with [`drive`](Self::drive).  The next call starts a
    /// new value, again after the consumed bytes.  Offsets of errors refer
    /// to the input.
    fn frame(&self, state: &mut Self::State, input: &[u8], eof: bool) -> Result<Frame, Error>;

    /// Deserializes a value from its frame.
    ///
    /// The frame holds the bytes of a value found by
    /// [`frame`](Self::frame).  Offsets of errors refer to the frame.
    fn drive<'de>(
        &self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error>;

    /// Returns `true` if the format is text.
    ///
    /// For text formats the positions of errors are resolved into lines and
    /// columns.  This is `false` by default.
    fn is_text(&self) -> bool {
        false
    }

    /// Returns `true` if the decoder implements [`feed`](Self::feed).
    ///
    /// This can depend on the configuration, for instance JSON Lines are
    /// read line by line.
    fn supports_feed(&self) -> bool {
        false
    }

    /// Deserializes a value while its input arrives.
    ///
    /// This is only invoked if [`supports_feed`](Self::supports_feed)
    /// returns `true`.  It's used instead of [`frame`](Self::frame) and
    /// [`drive`](Self::drive) for values which do not borrow from the input.
    /// The decoder emits the events of the parts of the value in the input
    /// into the driver and returns how much of the input it used
    /// ([`Progress::NeedMore`]) until the value is complete
    /// ([`Progress::Done`]).  Only incomplete tokens need to be kept.  The
    /// driver is the same for all calls for a value, the first call for a
    /// value starts where the previous value ended.  At the end of the
    /// input (`eof`) the value has to be completed (or fail).
    ///
    /// As the input does not live beyond the call, the events cannot
    /// borrow from it.  `offset` is the offset of the input in the stream:
    /// the input ranges of the events and the offsets of errors refer to
    /// positions in the stream.  After an error the value is abandoned, the
    /// decoder decides if the stream can continue with the next value (for
    /// instance by skipping the rest of the value if a sink failed) or if
    /// further calls fail.
    fn feed(
        &self,
        state: &mut Self::State,
        input: &[u8],
        offset: usize,
        eof: bool,
        driver: &mut DeserializeDriver<'_, '_>,
    ) -> Result<Progress, Error> {
        let _ = (state, input, offset, eof, driver);
        Err(Error::new(
            ErrorKind::Unexpected,
            "the decoder cannot deserialize incrementally",
        ))
    }

    /// Deserializes a value from a slice.
    ///
    /// The slice holds a single value, what may follow it depends on the
    /// format and its configuration.  Types can borrow from the slice.
    // the names match the methods of the configurations of the formats
    #[allow(clippy::wrong_self_convention)]
    fn from_slice<'de, T: Deserialize<'de>>(&self, input: &'de [u8]) -> Result<T, Error> {
        self.from_slice_with(input, |_| {})
    }

    /// Deserializes a value from a slice with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// deserialized, for instance to add [`Layer`](crate::de::Layer)s.
    ///
    /// The provided implementation finds the value with
    /// [`frame`](Self::frame) and deserializes it with
    /// [`drive`](Self::drive).  Formats typically parse slices directly.
    #[allow(clippy::wrong_self_convention)]
    fn from_slice_with<'de, T, F>(&self, input: &'de [u8], setup: F) -> Result<T, Error>
    where
        T: Deserialize<'de>,
        F: FnOnce(&mut DeserializeDriver<'_, 'de>),
    {
        crate::io::decode_slice(self, input, setup)
    }

    /// Deserializes a value from a reader.
    ///
    /// The stream holds a single value, a value after it is an error.  The
    /// reader does not need to be buffered.  To read more than one value
    /// use a [`Reader`](crate::io::Reader).
    #[allow(clippy::wrong_self_convention)]
    fn from_reader<T: DeserializeOwned, R: Read>(&self, reader: R) -> Result<T, Error> {
        crate::io::from_reader(reader, self)
    }
}

impl<D: Decoder + ?Sized> Decoder for &D {
    type State = D::State;

    fn frame(&self, state: &mut Self::State, input: &[u8], eof: bool) -> Result<Frame, Error> {
        (**self).frame(state, input, eof)
    }

    fn drive<'de>(
        &self,
        frame: &'de [u8],
        driver: &mut DeserializeDriver<'_, 'de>,
    ) -> Result<(), Error> {
        (**self).drive(frame, driver)
    }

    fn is_text(&self) -> bool {
        (**self).is_text()
    }

    fn supports_feed(&self) -> bool {
        (**self).supports_feed()
    }

    fn feed(
        &self,
        state: &mut Self::State,
        input: &[u8],
        offset: usize,
        eof: bool,
        driver: &mut DeserializeDriver<'_, '_>,
    ) -> Result<Progress, Error> {
        (**self).feed(state, input, offset, eof, driver)
    }

    fn from_slice_with<'de, T, F>(&self, input: &'de [u8], setup: F) -> Result<T, Error>
    where
        T: Deserialize<'de>,
        F: FnOnce(&mut DeserializeDriver<'_, 'de>),
    {
        (**self).from_slice_with(input, setup)
    }
}
