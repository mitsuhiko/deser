use std::borrow::Cow;

/// An event represents an atomic serialization and deserialization event.
///
/// ## Serialization
///
/// [`Event`] and [`Chunk`](crate::ser::Chunk) are two close relatives.  A chunk
/// is stateful whereas [`Event`] represents a single event from a chunk.
/// Atomic chunks directly create an event whereas compound chunks keep emitting
/// more chunks which again can produce events.  To go from chunks to events use
/// the [`for_each_event`](crate::ser::for_each_event) method.
///
/// ## Deserialization
///
/// During deserialization events are passed to a [`Driver`](crate::de::Driver)
/// to drive the deserialization.
#[derive(Debug, PartialEq)]
pub enum Event<'a> {
    Null,
    Bool(bool),
    Str(Cow<'a, str>),
    Bytes(Cow<'a, [u8]>),
    U64(u64),
    I64(i64),
    F64(f64),
    MapStart,
    MapEnd,
    SeqStart,
    SeqEnd,
}

impl<'a> Event<'a> {
    /// Makes a static clone of the event decoupling the lifetimes.
    pub fn to_static(&self) -> Event<'static> {
        match *self {
            Event::Null => Event::Null,
            Event::Bool(v) => Event::Bool(v),
            Event::Str(ref v) => Event::Str(Cow::Owned(v.to_string())),
            Event::Bytes(ref v) => Event::Bytes(Cow::Owned(v.to_vec())),
            Event::U64(v) => Event::U64(v),
            Event::I64(v) => Event::I64(v),
            Event::F64(v) => Event::F64(v),
            Event::MapStart => Event::MapStart,
            Event::MapEnd => Event::MapEnd,
            Event::SeqStart => Event::SeqStart,
            Event::SeqEnd => Event::SeqEnd,
        }
    }
}
