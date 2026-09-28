use std::io::Write;

use crate::error::{Error, ErrorKind};
use crate::ser::{Serialize, SerializeDriver};

/// The result of [`Encoder::encode_incremental`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoded {
    /// The value is complete, the output holds the rest of it.
    Done,
    /// The value is not complete.
    ///
    /// The output holds a part of it which the caller writes (and removes
    /// from the output) before it calls the encoder again to continue.
    Partial,
}

/// A data format that serializes values into bytes.
///
/// This is implemented by the serializer configurations of the data formats
/// (for instance `deser_json::SerializerConfig`), which makes them usable in
/// generic code.  An encoder serializes values into vectors (see
/// [`to_vec`](Self::to_vec)) and into streams (see
/// [`to_writer`](Self::to_writer) and [`io`](crate::io)).  Encoders write
/// everything that separates the values of a stream, for instance the line
/// breaks of JSON Lines or the markers between YAML documents.
///
/// Like with [`Decoder`](crate::io::Decoder), the encoder is the
/// configuration and everything a stream needs to remember is kept in its
/// [`State`](Self::State), for instance the number of values written or the
/// columns of a CSV file.
///
/// Formats which can write the output of a value before the value is
/// complete additionally implement
/// [`encode_incremental`](Self::encode_incremental).  Writers use it to
/// write large values in pieces, so the memory used does not depend on the
/// size of the values.
pub trait Encoder {
    /// The state of a stream.
    ///
    /// Every stream starts with the default state, writers can also start
    /// with a given state (see
    /// [`Writer::with_state`](crate::io::Writer::with_state)).  Encoders
    /// that encode values incrementally also keep the progress of the value
    /// that is being written here.
    type State: Default;

    /// Serializes a value and appends its bytes to the output.
    ///
    /// The value is serialized by driving the driver, which might have been
    /// configured before (for instance with layers).  If this fails, the
    /// output can contain a partial value (the writers discard it) but the
    /// state must be unchanged: the next value is written as if the failed
    /// one was never attempted.
    fn encode(
        &self,
        state: &mut Self::State,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), Error>;

    /// Returns `true` if the encoder implements
    /// [`encode_incremental`](Self::encode_incremental).
    ///
    /// This can depend on the configuration.
    fn supports_incremental(&self) -> bool {
        false
    }

    /// Serializes a part of a value and appends its bytes to the output.
    ///
    /// This is only invoked if
    /// [`supports_incremental`](Self::supports_incremental) returns `true`.
    /// It works like [`encode`](Self::encode) but the encoder can stop
    /// once the output holds at least `limit` bytes that are final (it can
    /// hold more) and return [`Encoded::Partial`].  The caller then writes
    /// the output, removes it and calls again with the same driver until
    /// the value is complete ([`Encoded::Done`]).  The encoder keeps the
    /// progress of the value in the state.  Output that can still change
    /// (for instance the header of a container whose length is not known
    /// yet) is kept by the encoder, the output only holds final bytes.
    ///
    /// With a limit of `usize::MAX` the value is always completed in a
    /// single call, which allows encoders to use faster paths (see
    /// [`SerializeDriver::drive_until`]).
    ///
    /// If this fails, the value is abandoned: the encoder has to discard its
    /// progress and the state has to be as if the value was never
    /// attempted.  The bytes that were written before stay written, which
    /// is why the writers do not write more values after that.  The
    /// writers also do not continue a stream if the value was abandoned for
    /// other reasons (like a failed write), so the encoder does not need to
    /// handle a new value being started while one is in progress.
    fn encode_incremental(
        &self,
        state: &mut Self::State,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
        limit: usize,
    ) -> Result<Encoded, Error> {
        let _ = (state, driver, out, limit);
        Err(Error::new(
            ErrorKind::Unexpected,
            "the encoder cannot serialize incrementally",
        ))
    }

    /// Serializes a value into a vector.
    fn to_vec(&self, value: &dyn Serialize) -> Result<Vec<u8>, Error> {
        self.to_vec_with(value, |_| {})
    }

    /// Serializes a value into a vector with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](crate::ser::Layer)s.
    fn to_vec_with<F>(&self, value: &dyn Serialize, setup: F) -> Result<Vec<u8>, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut out = Vec::new();
        crate::io::encode(self, &mut Self::State::default(), value, setup, &mut out)?;
        Ok(out)
    }

    /// Serializes a value into a writer.
    ///
    /// If the encoder supports it, large values are written in pieces while
    /// they are serialized (see [`Writer`](crate::io::Writer)).  The writer
    /// does not need to be buffered.  To write more than one value use a
    /// [`Writer`](crate::io::Writer).
    fn to_writer<W: Write>(&self, writer: W, value: &dyn Serialize) -> Result<(), Error> {
        crate::io::to_writer(writer, self, value)
    }
}

impl<E: Encoder + ?Sized> Encoder for &E {
    type State = E::State;

    fn encode(
        &self,
        state: &mut Self::State,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        (**self).encode(state, driver, out)
    }

    fn supports_incremental(&self) -> bool {
        (**self).supports_incremental()
    }

    fn encode_incremental(
        &self,
        state: &mut Self::State,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
        limit: usize,
    ) -> Result<Encoded, Error> {
        (**self).encode_incremental(state, driver, out, limit)
    }
}
