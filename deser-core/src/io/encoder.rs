use std::io::Write;

use crate::error::Error;
use crate::ser::{Serialize, SerializeDriver};

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
pub trait Encoder {
    /// The state of a stream.
    ///
    /// Every stream starts with the default state, writers can also start
    /// with a given state (see
    /// [`Writer::with_state`](crate::io::Writer::with_state)).
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
    /// The value is written with a single write.  To write more than one
    /// value use a [`Writer`](crate::io::Writer).
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
}
