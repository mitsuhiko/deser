use crate::de::{Deserialize, DeserializeDriver};
use crate::error::{Error, ErrorKind};

/// Deserializes values from an input.
///
/// This is implemented by the deserializers of the data formats (for
/// instance `deser_json::Deserializer`) and by other sources of values
/// (like the value type of `deser-value`).  A deserializer holds its input,
/// every call deserializes the next value.  Deserializers implement
/// [`drive`](Self::drive) which parses the input and feeds the events of a
/// value into a driver.  The provided methods create the driver:
///
/// * [`deserialize`](Self::deserialize) deserializes a value.
/// * [`deserialize_with`](Self::deserialize_with) deserializes a value and
///   allows configuring the driver first, for instance to add
///   [`Layer`](crate::de::Layer)s or to wrap the sink of the value.
///
/// ```
/// use deser::de::{DeserializeDriver, Deserializer, Limits};
/// use deser::{Error, Event};
///
/// /// A format which reads comma separated numbers as a sequence.
/// struct Numbers<'a>(&'a str);
///
/// impl<'de> Deserializer<'de> for Numbers<'de> {
///     fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
///         driver.emit(Event::seq_start())?;
///         for item in self.0.split(',') {
///             let value: u64 = item.trim().parse().map_err(|_| {
///                 Error::new(deser::ErrorKind::Unexpected, "invalid number")
///             })?;
///             driver.emit(value)?;
///         }
///         driver.emit(Event::SeqEnd)
///     }
/// }
///
/// let value: Vec<u32> = Numbers("1, 2, 3").deserialize().unwrap();
/// assert_eq!(value, [1, 2, 3]);
///
/// let rv = Numbers("1, 2, 3").deserialize_with::<Vec<u32>, _>(|driver| {
///     driver.push_layer(Limits::new().max_items(2));
/// });
/// assert_eq!(rv.unwrap_err().to_string(), "Unexpected: too many items");
/// ```
pub trait Deserializer<'de> {
    /// Parses the input and feeds the events of a value into the driver.
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error>;

    /// Deserializes a value.
    fn deserialize<T: Deserialize<'de>>(&mut self) -> Result<T, Error>
    where
        Self: Sized,
    {
        self.deserialize_with(|_| {})
    }

    /// Deserializes a value with a configured driver.
    ///
    /// The callback is invoked with the driver before the first event is
    /// emitted.
    fn deserialize_with<T, F>(&mut self, setup: F) -> Result<T, Error>
    where
        T: Deserialize<'de>,
        F: FnOnce(&mut DeserializeDriver<'_, 'de>),
        Self: Sized,
    {
        let mut out = None;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            setup(&mut driver);
            self.drive(&mut driver)?;
        }
        out.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))
    }
}
