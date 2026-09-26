use std::borrow::Cow;

use deser::ser::{Chunk, MapEmitter, SeqEmitter, Serialize, SerializeHandle};
use deser::{Atom, ContainerShape, Error, ErrorKind, State};

use crate::map::Map;
use crate::seq::Seq;
use crate::value::{Kind, Value};

impl Serialize for Value {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        if let Some(ref meta) = self.meta
            && !meta.event_data().is_empty()
        {
            state.attach_event_data(meta.event_data());
        }
        self.kind.serialize(state)
    }

    fn container_shape(&self) -> ContainerShape {
        self.kind.container_shape()
    }

    fn is_optional(&self) -> bool {
        matches!(self.kind, Kind::Null)
    }
}

impl Serialize for Kind {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(match self {
            Kind::Null => Chunk::Atom(Atom::Null),
            Kind::Bool(value) => Chunk::Atom(Atom::Bool(*value)),
            Kind::U64(value) => Chunk::Atom(Atom::U64(*value)),
            Kind::I64(value) => Chunk::Atom(Atom::I64(*value)),
            Kind::F64(value) => Chunk::Atom(Atom::F64(*value)),
            Kind::Char(value) => Chunk::Atom(Atom::Char(*value)),
            Kind::Str(value) => Chunk::Atom(Atom::Str(Cow::Borrowed(value))),
            Kind::Bytes(value) => Chunk::Atom(Atom::Bytes(value.as_borrowed())),
            Kind::Ext(value) => Chunk::Atom(Atom::Ext(value.as_borrowed())),
            Kind::Seq(seq) => return seq.serialize(state),
            Kind::Map(map) => return map.serialize(state),
        })
    }

    fn container_shape(&self) -> ContainerShape {
        match self {
            Kind::Seq(seq) => seq.container_shape(),
            Kind::Map(map) => map.container_shape(),
            _ => ContainerShape::new(),
        }
    }

    fn is_optional(&self) -> bool {
        matches!(self, Kind::Null)
    }
}

impl Serialize for Seq {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Seq(Box::new(SeqIter(self.items.iter()))))
    }

    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new()
            .with_len(self.len())
            .with_order(self.order())
    }
}

struct SeqIter<'a>(std::slice::Iter<'a, Value>);

impl SeqEmitter for SeqIter<'_> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(SerializeHandle::to))
    }
}

impl Serialize for Map {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Map(Box::new(MapIter {
            iter: self.inner.entries.iter(),
            value: None,
        })))
    }

    fn container_shape(&self) -> ContainerShape {
        ContainerShape::new()
            .with_len(self.len())
            .with_order(self.order())
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
                ErrorKind::Unexpected,
                "next_value called before next_key",
            )),
        }
    }
}
