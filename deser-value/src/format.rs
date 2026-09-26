use std::borrow::Cow;
use std::sync::Arc;

use deser::de::{self, Deserialize, DeserializeDriver};
use deser::ser::{self, Serialize, SerializeDriver};
use deser::{Atom, ContainerShape, Error, ErrorKind, Event};

use crate::value::{Kind, Value};

/// Deserializes types from a [`Value`].
///
/// This is a [`Deserializer`](deser::de::Deserializer) which emits the
/// events of a value.  It's what
/// [`from_value`] uses, use it directly to configure the deserialization,
/// for instance to add layers:
///
/// ```
/// use deser_path::{Path, PathLayer};
/// use deser_value::{value, Deserializer};
///
/// let value = value!({"items": [1, "two"]});
/// let err = Deserializer::new(&value)
///     .deserialize_with::<std::collections::BTreeMap<String, Vec<u32>>, _>(|driver| {
///         driver.push_layer(PathLayer::new());
///     })
///     .unwrap_err();
/// assert_eq!(err.attachment::<Path>().unwrap().to_string(), "items[1]");
/// ```
///
/// Strings and bytes are passed on borrowed from the value, which means
/// that types like `&str` can be deserialized.  The [meta data](crate::Meta)
/// of the values is restored for every event: event data is attached and if
/// the value has spans, the input ranges and the source are published.
/// This means that errors refer to the location in the input the value was
/// deserialized from.
pub struct Deserializer<'a> {
    value: &'a Value,
}

impl<'a> Deserializer<'a> {
    /// Creates a deserializer for a value.
    pub fn new(value: &'a Value) -> Deserializer<'a> {
        Deserializer { value }
    }

    /// Deserializes the value.
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        de::Deserializer::deserialize(self)
    }

    /// Deserializes the value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// deserialized, for instance to add [`Layer`](deser::de::Layer)s.
    pub fn deserialize_with<T, F>(&mut self, setup: F) -> Result<T, Error>
    where
        T: Deserialize<'a>,
        F: FnOnce(&mut DeserializeDriver<'_, 'a>),
    {
        de::Deserializer::deserialize_with(self, setup)
    }
}

impl<'de> de::Deserializer<'de> for Deserializer<'de> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
        let mut source = None;
        drive(self.value, driver, &mut source).map_err(|err| match source {
            Some(source) => err.resolve_position(source.as_bytes()),
            None => err,
        })
    }
}

/// A map or sequence whose values are emitted.
enum Frame<'a> {
    Seq(std::slice::Iter<'a, Value>, &'a Value),
    Map(
        indexmap::map::Iter<'a, Value, Value>,
        Option<&'a Value>,
        &'a Value,
    ),
}

/// Publishes the input range and the source of an event.
fn set_range<'de>(
    driver: &mut DeserializeDriver<'_, 'de>,
    current: &mut Option<&'de Arc<str>>,
    new: &'de Arc<str>,
    (start, end): (usize, usize),
) {
    if !current.is_some_and(|current| Arc::ptr_eq(current, new)) {
        driver.state_mut().set_source(new.clone());
        *current = Some(new);
    }
    driver.state_mut().set_input_range(start, end);
}

fn drive<'de>(
    root: &'de Value,
    driver: &mut DeserializeDriver<'_, 'de>,
    source: &mut Option<&'de Arc<str>>,
) -> Result<(), Error> {
    let mut stack: Vec<Frame<'de>> = Vec::new();
    let mut next = Some(root);
    loop {
        if let Some(value) = next.take() {
            if let Some(ref meta) = value.meta {
                if !meta.event_data().is_empty() {
                    driver.state_mut().attach_event_data(meta.event_data());
                }
                if let Some(span) = meta.span() {
                    set_range(driver, source, span.source(), span.start_range());
                }
            }
            match value.kind {
                Kind::Seq(ref seq) => {
                    driver.emit(Event::SeqStart(shape(seq.len(), seq.order())))?;
                    stack.push(Frame::Seq(seq.iter(), value));
                }
                Kind::Map(ref map) => {
                    driver.emit(Event::MapStart(shape(map.len(), map.order())))?;
                    stack.push(Frame::Map(map.inner.entries.iter(), None, value));
                }
                ref leaf => driver.emit_borrowed(leaf_atom(leaf))?,
            }
        }

        let Some(frame) = stack.last_mut() else {
            return Ok(());
        };
        next = match frame {
            Frame::Seq(iter, _) => iter.next(),
            Frame::Map(iter, pending, _) => pending.take().or_else(|| {
                let (key, value) = iter.next()?;
                *pending = Some(value);
                Some(key)
            }),
        };
        if next.is_none() {
            let (container, event) = match stack.pop() {
                Some(Frame::Seq(_, container)) => (container, Event::SeqEnd),
                Some(Frame::Map(_, _, container)) => (container, Event::MapEnd),
                None => unreachable!(),
            };
            if let Some(span) = container.span()
                && let Some(range) = span.end_range()
            {
                set_range(driver, source, span.source(), range);
            }
            driver.emit(event)?;
        }
    }
}

fn shape(len: usize, order: deser::Order) -> ContainerShape {
    ContainerShape::new().with_len(len).with_order(order)
}

/// Returns the atom of a value without children.
fn leaf_atom(kind: &Kind) -> Atom<'_> {
    match kind {
        Kind::Null => Atom::Null,
        Kind::Bool(value) => Atom::Bool(*value),
        Kind::U64(value) => Atom::U64(*value),
        Kind::I64(value) => Atom::I64(*value),
        Kind::F64(value) => Atom::F64(*value),
        Kind::Char(value) => Atom::Char(*value),
        Kind::Str(value) => Atom::Str(Cow::Borrowed(value)),
        Kind::Bytes(value) => Atom::Bytes(value.as_borrowed()),
        Kind::Ext(value) => Atom::Ext(value.as_borrowed()),
        Kind::Seq(_) | Kind::Map(_) => unreachable!("containers are not atoms"),
    }
}

/// Deserializes a type from a value.
///
/// Types can borrow strings and bytes from the value.
///
/// ```
/// use deser::Deserialize;
/// use deser_value::{from_value, value};
///
/// #[derive(Debug, Deserialize)]
/// struct User<'a> {
///     name: &'a str,
///     id: u64,
/// }
///
/// let value = value!({"name": "Jane", "id": 42});
/// let user: User = from_value(&value).unwrap();
/// assert_eq!(user.name, "Jane");
/// ```
///
/// To configure the deserialization use the [`Deserializer`].
pub fn from_value<'de, T: Deserialize<'de>>(value: &'de Value) -> Result<T, Error> {
    Deserializer::new(value).deserialize()
}

/// Serializes a value into a [`Value`].
///
/// The [event data](deser::State::event) of the serialized values (such
/// as formatting hints) is captured in the [meta data](crate::Meta) of the
/// values.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser::Order;
/// use deser_value::{to_value, value};
///
/// let map = BTreeMap::from([("b", 2), ("a", 1)]);
/// let value = to_value(&map).unwrap();
/// assert_eq!(value, value!({"a": 1, "b": 2}));
/// assert_eq!(value.as_map().unwrap().order(), Order::Sorted);
/// ```
///
/// To configure the serialization use the [`Serializer`].
pub fn to_value<T: Serialize>(value: &T) -> Result<Value, Error> {
    let mut serializer = Serializer::new();
    serializer.serialize(value)?;
    Ok(serializer.finish().pop().expect("a value was serialized"))
}

/// Serializes values into [`Value`]s.
///
/// This is a [`Serializer`](deser::ser::Serializer) which builds a value
/// from the events of a serialized value.  It's what [`to_value`] uses, use
/// it directly to configure the serialization, for instance to add layers.
/// Every call to [`serialize`](Self::serialize) adds a value:
///
/// ```
/// use deser::ser::{Layer, Next};
/// use deser::{Atom, Error, Event};
/// use deser_value::{Serializer, value};
///
/// /// Writes all numbers as strings.
/// struct NumbersAsStrings;
///
/// impl Layer for NumbersAsStrings {
///     fn event(&mut self, event: Event<'_>, next: &mut Next<'_>) -> Result<(), Error> {
///         match event {
///             Event::Atom(Atom::U64(value)) => next.emit(value.to_string().into()),
///             event => next.emit(event),
///         }
///     }
/// }
///
/// let mut serializer = Serializer::new();
/// serializer.serialize(&true).unwrap();
/// serializer
///     .serialize_with(&vec![1u64, 2], |driver| driver.push_layer(NumbersAsStrings))
///     .unwrap();
/// assert_eq!(serializer.finish(), [value!(true), value!(["1", "2"])]);
/// ```
///
/// The [event data](deser::State::event) of the serialized values (such as
/// formatting hints) is captured in the [meta data](crate::Meta) of the
/// values.
#[derive(Debug, Default, Clone)]
pub struct Serializer {
    values: Vec<Value>,
}

impl Serializer {
    /// Creates a serializer.
    pub fn new() -> Serializer {
        Serializer::default()
    }

    /// Serializes a value.
    ///
    /// If the value fails to serialize, nothing is added.
    pub fn serialize(&mut self, value: &dyn Serialize) -> Result<(), Error> {
        ser::Serializer::serialize(self, value)
    }

    /// Serializes a value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](deser::ser::Layer)s.
    pub fn serialize_with<F>(&mut self, value: &dyn Serialize, setup: F) -> Result<(), Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        ser::Serializer::serialize_with(self, value, setup)
    }

    /// Returns the values serialized so far.
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// Returns the values.
    pub fn finish(self) -> Vec<Value> {
        self.values
    }
}

impl ser::Serializer for Serializer {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        let mut out = None;
        {
            let mut de = DeserializeDriver::new(&mut out);
            driver.drive(|event, state| {
                if state.has_event_data() {
                    de.state_mut()
                        .attach_event_data(&state.capture_event_data());
                }
                de.emit(event)
            })?;
        }
        let value =
            out.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "no value was serialized"))?;
        self.values.push(value);
        Ok(())
    }
}
