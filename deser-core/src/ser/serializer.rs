use crate::error::Error;
use crate::ser::{Serialize, SerializeDriver};

/// Serializes values into an output.
///
/// This is implemented by the serializers of the data formats (for instance
/// `deser_json::Serializer`) and by other destinations of values (like the
/// value type of `deser-value`).  A serializer holds its output, every call
/// serializes a value into it.  Serializers implement
/// [`drive`](Self::drive) which receives the events of a value from a
/// driver.  The provided methods create the driver:
///
/// * [`serialize`](Self::serialize) serializes a value.
/// * [`serialize_with`](Self::serialize_with) serializes a value and allows
///   configuring the driver first, for instance to add
///   [`Layer`](crate::ser::Layer)s.
///
/// ```
/// use deser::ser::{SerializeDriver, Serializer};
/// use deser::{Atom, Error, ErrorKind, Event};
///
/// /// A format which writes sequences of numbers comma separated.
/// struct Numbers(String);
///
/// impl Serializer for Numbers {
///     fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
///         driver.drive(|event, _state| {
///             match event {
///                 Event::Atom(Atom::U64(value)) => {
///                     if !self.0.is_empty() {
///                         self.0.push_str(", ");
///                     }
///                     self.0.push_str(&value.to_string());
///                 }
///                 Event::SeqStart(_) | Event::SeqEnd => {}
///                 _ => return Err(Error::new(ErrorKind::UnsupportedType, "not a number")),
///             }
///             Ok(())
///         })
///     }
/// }
///
/// let mut numbers = Numbers(String::new());
/// numbers.serialize(&vec![1u64, 2, 3]).unwrap();
/// assert_eq!(numbers.0, "1, 2, 3");
/// ```
pub trait Serializer {
    /// Receives the events of a value from the driver and writes them into
    /// the output.
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error>;

    /// Serializes a value.
    fn serialize(&mut self, value: &dyn Serialize) -> Result<(), Error> {
        self.drive(&mut SerializeDriver::new(value))
    }

    /// Serializes a value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](crate::ser::Layer)s.
    fn serialize_with<F>(&mut self, value: &dyn Serialize, setup: F) -> Result<(), Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
        Self: Sized,
    {
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        self.drive(&mut driver)
    }
}

impl<S: Serializer + ?Sized> Serializer for &mut S {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        (**self).drive(driver)
    }
}
