use std::borrow::Cow;

use deser::de::{Deserialize, Sink, SinkHandle};
use deser::{Atom, Error, ErrorKind, EventData, State};

use crate::map::Map;
use crate::seq::Seq;
use crate::value::{Kind, Meta, Span, Value, owned_bytes};

/// Containers are not preallocated beyond this number of elements as the
/// length comes from the input.
const MAX_PREALLOC: usize = 1024;

/// Where a [`ValueSink`] places the value.
enum Out<'a> {
    Value(&'a mut Option<Value>),
    Seq(&'a mut Option<Seq>),
    Map(&'a mut Option<Map>),
}

/// The container a [`ValueSink`] is building.
enum Building {
    None,
    Seq(Seq),
    Map(Map),
}

/// Deserializes values.
struct ValueSink<'a> {
    out: Out<'a>,
    building: Building,
    meta: Option<Box<Meta>>,
    // the key of the entry whose value is deserialized.
    key: Option<Value>,
    // the key or value that is deserialized.
    slot: Option<Value>,
}

impl<'a> ValueSink<'a> {
    fn new(out: Out<'a>) -> ValueSink<'a> {
        ValueSink {
            out,
            building: Building::None,
            meta: None,
            key: None,
            slot: None,
        }
    }

    /// Adds the key or value that was deserialized last to the container.
    fn flush(&mut self) {
        if let Some(value) = self.slot.take() {
            match self.building {
                Building::Seq(ref mut seq) => seq.items.push(value),
                Building::Map(ref mut map) => {
                    if let Some(key) = self.key.take() {
                        map.inner.entries.insert(key, value);
                    }
                }
                Building::None => {}
            }
        }
    }

    /// Prepares for the next value in the container.
    fn begin_value(&mut self) -> Result<(), Error> {
        match self.building {
            Building::Map(ref map) => {
                let key = self
                    .slot
                    .take()
                    .ok_or_else(|| Error::new(ErrorKind::Unexpected, "missing map key"))?;
                if map.contains_key(&key) {
                    return Err(duplicate_key(&key));
                }
                self.key = Some(key);
            }
            _ => self.flush(),
        }
        Ok(())
    }
}

#[cold]
fn duplicate_key(key: &Value) -> Error {
    let err = Error::new(
        ErrorKind::Unexpected,
        format!("duplicate map key {:?}", key),
    );
    // point at the key if its location is known
    match key.span() {
        Some(span) => err.with_offset(span.range().start),
        None => err,
    }
}

impl<'a, 'de> Sink<'de> for ValueSink<'a> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match self.out {
            Out::Value(ref mut out) => {
                **out = Some(atom_value(atom, state)?);
                Ok(())
            }
            _ => self.unexpected_atom(atom, state),
        }
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        if let Out::Seq(_) = self.out {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "unexpected map, expected sequence",
            ));
        }
        let shape = state.container_shape();
        self.meta = capture_meta(state);
        self.building = Building::Map(
            Map::with_capacity(shape.len().unwrap_or(0).min(MAX_PREALLOC))
                .with_order(shape.order()),
        );
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        if let Out::Map(_) = self.out {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "unexpected sequence, expected map",
            ));
        }
        let shape = state.container_shape();
        self.meta = capture_meta(state);
        self.building = Building::Seq(
            Seq::with_capacity(shape.len().unwrap_or(0).min(MAX_PREALLOC))
                .with_order(shape.order()),
        );
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.flush();
        Ok(Value::deserialize_into(&mut self.slot))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.begin_value()?;
        Ok(Value::deserialize_into(&mut self.slot))
    }

    fn key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.flush();
        self.slot = Some(atom_value(atom, state)?);
        Ok(())
    }

    fn value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.begin_value()?;
        self.slot = Some(atom_value(atom, state)?);
        Ok(())
    }

    fn borrowed_key_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.key_atom(atom, state)
    }

    fn borrowed_value_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.value_atom(atom, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.flush();
        let kind = match std::mem::replace(&mut self.building, Building::None) {
            Building::None => return Ok(()),
            Building::Seq(seq) => Kind::Seq(seq),
            Building::Map(map) => Kind::Map(map),
        };
        if let Some(ref mut meta) = self.meta
            && let Some(span) = meta.span_mut()
            && let Some(range) = state.input_range()
        {
            span.set_end(range.start, range.end);
        }
        match (&mut self.out, kind) {
            (Out::Value(out), kind) => {
                **out = Some(Value {
                    kind,
                    meta: self.meta.take(),
                })
            }
            (Out::Seq(out), Kind::Seq(seq)) => **out = Some(seq),
            (Out::Map(out), Kind::Map(map)) => **out = Some(map),
            _ => unreachable!(),
        }
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(match self.out {
            Out::Value(_) => "any value",
            Out::Seq(_) => "sequence",
            Out::Map(_) => "map",
        })
    }
}

/// Captures the meta data of the current event.
fn capture_meta(state: &State) -> Option<Box<Meta>> {
    let span = match (state.input_range(), state.source()) {
        (Some(range), Some(source)) => Some(Span::new(range, source.clone())),
        _ => None,
    };
    if span.is_none() && !state.has_event_data() {
        return None;
    }
    let event_data = if state.has_event_data() {
        state.capture_event_data()
    } else {
        EventData::new()
    };
    Some(Box::new(Meta::from_parts(event_data, span)))
}

/// Converts an atom into a value.
fn atom_value(atom: Atom, state: &State) -> Result<Value, Error> {
    let kind = match atom {
        Atom::Null => Kind::Null,
        Atom::Bool(value) => Kind::Bool(value),
        Atom::Str(value) => Kind::Str(value.into_owned()),
        Atom::Bytes(value) => Kind::Bytes(owned_bytes(value)),
        Atom::Char(value) => Kind::Char(value),
        Atom::U64(value) => Kind::U64(value),
        Atom::I64(value) => Kind::from_i64(value),
        Atom::F32(value) => Kind::F32(value),
        Atom::F64(value) => Kind::F64(value),
        Atom::Ext(value) => Kind::from_ext(value),
        other => return Err(other.unexpected_error("any value")),
    };
    Ok(Value {
        kind,
        meta: capture_meta(state),
    })
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(ValueSink::new(Out::Value(out)))
    }

    #[doc(hidden)]
    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        *out = Some(atom_value(atom, state)?);
        Ok(())
    }

    #[doc(hidden)]
    fn __private_borrowed_atom_into(
        out: &mut Option<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        *out = Some(atom_value(atom, state)?);
        Ok(())
    }
}

impl<'de> Deserialize<'de> for Seq {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(ValueSink::new(Out::Seq(out)))
    }
}

impl<'de> Deserialize<'de> for Map {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(ValueSink::new(Out::Map(out)))
    }
}
