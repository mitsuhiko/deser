use deser_core::Text;
use deser_core::ser::{Emit, MapEmitter, SeqEmitter, Serialize, SerializeHandle};
use deser_core::{Atom, ContainerShape, Error, ErrorKind, State};

use crate::map::Map;
use crate::seq::Seq;
use crate::value::{Kind, Value};

impl Serialize for Value {
    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        if let Some(ref meta) = value.meta
            && !meta.event_data().is_empty()
        {
            state.attach_event_data(meta.event_data());
        }
        Kind::serialize(&value.kind, state)
    }

    fn container_shape(value: &Self) -> ContainerShape {
        Kind::container_shape(&value.kind)
    }

    fn is_optional(value: &Self) -> bool {
        matches!(value.kind, Kind::Null)
    }
}

impl Serialize for Kind {
    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(match this {
            Kind::Null => Emit::Atom(Atom::Null),
            Kind::Bool(value) => Emit::Atom(Atom::Bool(*value)),
            Kind::U64(value) => Emit::Atom(Atom::U64(*value)),
            Kind::I64(value) => Emit::Atom(Atom::I64(*value)),
            Kind::F32(value) => Emit::Atom(Atom::F32(*value)),
            Kind::F64(value) => Emit::Atom(Atom::F64(*value)),
            Kind::Char(value) => Emit::Atom(Atom::Char(*value)),
            Kind::Str(value) => Emit::Atom(Atom::Str(Text::borrowed(value))),
            Kind::Lexical(value) => Emit::Atom(Atom::Lexical(Text::borrowed(value))),
            Kind::Bytes(value) => Emit::Atom(Atom::Bytes(value.as_borrowed())),
            Kind::Ext(value) => Emit::Atom(Atom::Ext(value.as_borrowed())),
            Kind::Implicit(value) => Emit::Atom(Atom::Implicit(value.as_borrowed())),
            Kind::Seq(seq) => return Seq::serialize(seq, state),
            Kind::Map(map) => return Map::serialize(map, state),
        })
    }

    fn container_shape(value: &Self) -> ContainerShape {
        match value {
            Kind::Seq(seq) => Seq::container_shape(seq),
            Kind::Map(map) => Map::container_shape(map),
            _ => ContainerShape::new(),
        }
    }

    fn is_optional(value: &Self) -> bool {
        matches!(value, Kind::Null)
    }
}

impl Serialize for Seq {
    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::seq(SeqIter(value.items.iter()), state))
    }

    fn container_shape(value: &Self) -> ContainerShape {
        {
            let mut shape = ContainerShape::with_len(value.len());
            shape.set_order(value.order());
            shape
        }
    }
}

struct SeqIter<'a>(std::slice::Iter<'a, Value>);

impl SeqEmitter for SeqIter<'_> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(SerializeHandle::to))
    }
}

impl Serialize for Map {
    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::map(
            MapIter {
                iter: this.inner.entries.iter(),
                value: None,
            },
            state,
        ))
    }

    fn container_shape(value: &Self) -> ContainerShape {
        {
            let mut shape = ContainerShape::with_len(value.len());
            shape.set_order(value.order());
            shape.set_multimap(value.is_multimap());
            shape
        }
    }
}

struct MapIter<'a> {
    iter: indexmap::map::Iter<'a, Value, Value>,
    value: Option<&'a Value>,
}

impl MapEmitter for MapIter<'_> {
    fn next_key(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.iter.next().map(|(key, value)| {
            self.value = Some(value);
            SerializeHandle::to(key)
        }))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SerializeHandle<'_>, Error> {
        match self.value.take() {
            Some(value) => Ok(SerializeHandle::to(value)),
            None => Err(Error::new(
                ErrorKind::InvalidState,
                "next_value called before next_key",
            )),
        }
    }
}
