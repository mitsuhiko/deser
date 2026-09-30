use crate::State;
use crate::de::{Deserialize, DeserializeDriver, SinkHandle};
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
///     fn drive(
///         &mut self,
///         driver: &mut DeserializeDriver<'_, 'de>,
///     ) -> Result<(), Error> {
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
        // only creating the sink and taking the value depend on the type,
        // the driver is created and run by a function that exists once.
        let mut setup = Some(setup);
        deserialize_value(|make_sink| {
            drive_new(self, make_sink, &mut |driver| {
                if let Some(setup) = setup.take() {
                    setup(driver);
                }
            })
        })
    }

    /// Updates an existing value with the next value.
    ///
    /// See [`Deserialize::deserialize_update`].  If this fails, the value
    /// might be partially updated.
    fn update<T: Deserialize<'de>>(&mut self, value: &mut T) -> Result<(), Error>
    where
        Self: Sized,
    {
        self.update_with(value, |_| {})
    }

    /// Updates an existing value with the next value after setting up the
    /// driver.
    ///
    /// This is like [`update`](Self::update) but the callback is invoked
    /// with the driver first, like with
    /// [`deserialize_with`](Self::deserialize_with).
    fn update_with<T, F>(&mut self, value: &mut T, setup: F) -> Result<(), Error>
    where
        T: Deserialize<'de>,
        F: FnOnce(&mut DeserializeDriver<'_, 'de>),
        Self: Sized,
    {
        let mut driver = DeserializeDriver::update(value);
        setup(&mut driver);
        self.drive(&mut driver)
    }
}

/// Creates the sink of the value that is deserialized.
///
/// This is passed from [`deserialize_value`] to the function that drives
/// the deserializer, which passes it on to [`drive_value`].  It must be
/// called once, with the state of the driver.
pub type MakeSink<'m, 'out, 'de> = dyn FnMut(&mut State) -> SinkHandle<'out, 'de> + 'm;

/// Deserializes a value with a function that drives a deserializer into
/// its sink.
///
/// This is how formats implement functions like `from_str`.  It does the
/// same as [`Deserializer::deserialize`], but only the part that depends
/// on the type of the value is generic: `drive` receives a function that
/// creates the sink of the value and passes it to [`drive_value`] together
/// with the deserializer.  If `drive` is a function that is not generic
/// over the value, the code that creates and runs the deserializer exists
/// once rather than once per type.
///
/// If no value was deserialized, an [`EndOfFile`](ErrorKind::EndOfFile)
/// error is returned.
///
/// ```
/// use deser::de::{
///     Deserialize, DeserializeDriver, Deserializer, MakeSink, deserialize_value, drive_value,
/// };
/// use deser::{Error, ErrorKind, Event};
///
/// /// A format which reads comma separated numbers as a sequence.
/// struct Numbers<'a>(&'a str);
///
/// impl<'de> Deserializer<'de> for Numbers<'de> {
///     fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
///         driver.emit(Event::seq_start())?;
///         for item in self.0.split(',') {
///             let value: u64 = item
///                 .trim()
///                 .parse()
///                 .map_err(|_| Error::new(ErrorKind::Unexpected, "invalid number"))?;
///             driver.emit(value)?;
///         }
///         driver.emit(Event::SeqEnd)
///     }
/// }
///
/// /// Deserializes a value from comma separated numbers.
/// pub fn from_str<'de, T: Deserialize<'de>>(s: &'de str) -> Result<T, Error> {
///     deserialize_value(|make_sink| drive_str(s, make_sink))
/// }
///
/// /// The part of `from_str` that does not depend on the type of the value.
/// fn drive_str<'de>(s: &'de str, make_sink: &mut MakeSink<'_, '_, 'de>) -> Result<(), Error> {
///     drive_value(&mut Numbers(s), make_sink)
/// }
///
/// let value: Vec<u32> = from_str("1, 2, 3").unwrap();
/// assert_eq!(value, [1, 2, 3]);
/// ```
#[inline]
pub fn deserialize_value<'de, T: Deserialize<'de>>(
    drive: impl for<'out> FnOnce(&mut MakeSink<'_, 'out, 'de>) -> Result<(), Error>,
) -> Result<T, Error> {
    let mut out = None;
    {
        let mut slot = Some(&mut out);
        drive(&mut |state| match slot.take() {
            Some(slot) => {
                // the top-level value is requested before it starts
                state.raw_requested = T::__private_raw();
                T::deserialize_into(slot, state)
            }
            None => panic!("the sink of a value was created twice"),
        })?;
    }
    out.ok_or_else(empty_input)
}

/// Drives a deserializer into the sink of a value.
///
/// This creates the state and the driver, creates the sink with
/// `make_sink` and calls [`Deserializer::drive`].  See
/// [`deserialize_value`] for how it's used.
#[inline(never)]
pub fn drive_value<'out, 'de>(
    de: &mut dyn Deserializer<'de>,
    make_sink: &mut MakeSink<'_, 'out, 'de>,
) -> Result<(), Error> {
    drive_new(de, make_sink, &mut |_| {})
}

/// Creates the state and the sink and drives a deserializer into it.
#[inline(never)]
fn drive_new<'out, 'de>(
    de: &mut dyn Deserializer<'de>,
    make_sink: &mut MakeSink<'_, 'out, 'de>,
    setup: &mut dyn FnMut(&mut DeserializeDriver<'_, 'de>),
) -> Result<(), Error> {
    let mut state = State::new();
    let sink = make_sink(&mut state);
    let mut driver = DeserializeDriver::from_state(state, sink);
    setup(&mut driver);
    de.drive(&mut driver)
}

#[cold]
fn empty_input() -> Error {
    Error::new(ErrorKind::EndOfFile, "empty input")
}
