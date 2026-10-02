use std::borrow::Cow;

use deser_core::de::{Deserialize, DuplicateKeys, Sink, SinkHandle, default_atom};
use deser_core::{Atom, Error, ErrorKind, Source, State};

use crate::map::Map;
use crate::seq::Seq;
use crate::value::{Kind, Meta, Span, Value, owned_bytes};

/// Where a [`ValueSink`] places the value.
enum Out<'a> {
    Value(&'a mut Option<Value>),
    Seq(&'a mut Option<Seq>),
    Map(&'a mut Option<Map>),
    /// Updates a value: maps are merged into maps, everything else replaces
    /// the value.
    UpdateValue(&'a mut Value),
    /// Merges a map into a map.
    UpdateMap(&'a mut Map),
}

/// The container a [`ValueSink`] is building.
enum Building {
    None,
    Seq(Seq),
    Map(Map),
}

/// What happens to the value of an entry of a map.
#[derive(Default)]
enum Entry {
    /// The entry is inserted, it replaces the value of a key that was given
    /// before.
    #[default]
    Insert,
    /// The value is added to the values of a key of a multimap that was
    /// given before.
    Repeat,
    /// The value is dropped as the key was given before and the first value
    /// is used.
    Ignore,
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
    // what happens to the value of the current entry
    entry: Entry,
}

impl<'a> ValueSink<'a> {
    fn new(out: Out<'a>) -> ValueSink<'a> {
        ValueSink {
            out,
            building: Building::None,
            meta: None,
            key: None,
            slot: None,
            entry: Entry::Insert,
        }
    }

    /// Adds the key or value that was deserialized last to the container.
    fn flush(&mut self) {
        if let Some(value) = self.slot.take() {
            match self.building {
                Building::Seq(ref mut seq) => seq.items.push(value),
                Building::Map(ref mut map) => {
                    if let Some(key) = self.key.take() {
                        match std::mem::take(&mut self.entry) {
                            Entry::Insert => {
                                map.inner.entries.insert(key, value);
                            }
                            Entry::Repeat => add_repeated(map, &key, value),
                            Entry::Ignore => {}
                        }
                    }
                }
                Building::None => {}
            }
        }
    }

    /// Prepares for the next value in the container.
    ///
    /// A key that was given before collects the values in a multimap,
    /// otherwise the [`DuplicateKeys`] policy decides.
    fn begin_value(&mut self, state: &State) -> Result<(), Error> {
        match self.building {
            Building::Map(ref map) => {
                let key = self
                    .slot
                    .take()
                    .ok_or_else(|| Error::new(ErrorKind::InvalidState, "missing map key"))?;
                if map.contains_key(&key) {
                    self.entry = if map.is_multimap() {
                        Entry::Repeat
                    } else {
                        match DuplicateKeys::of(state) {
                            DuplicateKeys::Last => Entry::Insert,
                            DuplicateKeys::First => Entry::Ignore,
                            _ => return Err(duplicate_key(&key)),
                        }
                    };
                }
                self.key = Some(key);
            }
            _ => self.flush(),
        }
        Ok(())
    }
}

/// Merges the entries of a map into another map.
///
/// The values of keys that exist are replaced (not merged), the entries keep
/// their position.  New entries are added at the end.  Keys in `map` are
/// unique, so the entries it adds cannot collide with each other.
fn merge_map(target: &mut Map, mut map: Map) {
    if target.is_empty() {
        *target = map;
        return;
    }
    let entries = &mut target.inner.entries;
    entries.reserve(map.len());
    for (key, value) in std::mem::take(&mut map.inner.entries) {
        entries.insert(key, value);
    }
}

/// Adds the value of a key of a multimap that was given before.
///
/// The values of the key become a sequence that is marked as repeated.
#[cold]
fn add_repeated(map: &mut Map, key: &Value, value: Value) {
    let Some(existing) = map.inner.entries.get_mut(key) else {
        return;
    };
    match existing.kind {
        Kind::Seq(ref mut seq) if seq.is_repeated() => seq.items.push(value),
        _ => {
            let first = std::mem::replace(existing, Value::from(()));
            let mut seq = Seq::from(vec![first, value]);
            seq.set_repeated(true);
            *existing = Value::from(seq);
        }
    }
}

#[cold]
fn duplicate_key(key: &Value) -> Error {
    let mut err = Error::new(
        ErrorKind::DuplicateKey,
        format!("duplicate map key {:?}", key),
    );
    // point at the key if its location is known
    if let Some(span) = key.span() {
        err.set_offset(span.range().start);
    }
    err
}

impl<'a, 'de> Sink<'de> for ValueSink<'a> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match self.out {
            Out::Value(ref mut out) => {
                **out = Some(atom_value(atom, state)?);
                Ok(())
            }
            Out::UpdateValue(ref mut out) => {
                **out = atom_value(atom, state)?;
                Ok(())
            }
            _ => default_atom(self, atom, state),
        }
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        if let Out::Seq(_) = self.out {
            return Err(Error::new(
                ErrorKind::InvalidType,
                "unexpected map, expected sequence",
            ));
        }
        let shape = state.container_shape();
        self.meta = capture_meta(state);
        let mut map = Map::with_capacity(shape.cautious_capacity::<(Value, Value)>());
        map.set_order(shape.order());
        map.set_multimap(shape.is_multimap());
        self.building = Building::Map(map);
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        if let Out::Map(_) | Out::UpdateMap(_) = self.out {
            return Err(Error::new(
                ErrorKind::InvalidType,
                "unexpected sequence, expected map",
            ));
        }
        let shape = state.container_shape();
        self.meta = capture_meta(state);
        let mut seq = Seq::with_capacity(shape.cautious_capacity::<Value>());
        seq.set_order(shape.order());
        self.building = Building::Seq(seq);
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.flush();
        Ok(Value::deserialize_into(&mut self.slot, state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.begin_value(state)?;
        Ok(Value::deserialize_into(&mut self.slot, state))
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.flush();
        self.slot = Some(atom_value(atom, state)?);
        Ok(())
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.begin_value(state)?;
        self.slot = Some(atom_value(atom, state)?);
        Ok(())
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.__private_key_atom(atom, state)
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.__private_value_atom(atom, state)
    }

    /// Takes all keys when the value is flattened into a struct.
    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        match self.building {
            Building::Map(_) => self.flush(),
            Building::None if !matches!(self.out, Out::Seq(_)) => {
                let mut map = Map::new();
                map.set_multimap(state.is_multimap());
                self.building = Building::Map(map);
            }
            _ => return Ok(None),
        }
        self.slot = Some(Value::from(key));
        self.begin_value(state)?;
        Ok(Some(Value::deserialize_into(&mut self.slot, state)))
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.flush();
        let kind = match std::mem::replace(&mut self.building, Building::None) {
            // a value that is flattened into a struct is an empty map if no
            // key was left for it
            Building::None => match self.out {
                Out::Value(ref mut out @ None) => {
                    **out = Some(Value::from(Map::new()));
                    return Ok(());
                }
                Out::Map(ref mut out @ None) => {
                    **out = Some(Map::new());
                    return Ok(());
                }
                // updates keep the value
                _ => return Ok(()),
            },
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
            // maps are merged into maps (the value keeps its meta data),
            // everything else is replaced
            (Out::UpdateValue(out), kind) => match (&mut out.kind, kind) {
                (Kind::Map(target), Kind::Map(map)) => merge_map(target, map),
                (_, kind) => {
                    **out = Value {
                        kind,
                        meta: self.meta.take(),
                    }
                }
            },
            (Out::UpdateMap(out), Kind::Map(map)) => merge_map(out, map),
            _ => unreachable!(),
        }
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(match self.out {
            Out::Value(_) | Out::UpdateValue(_) => "any value",
            Out::Seq(_) => "sequence",
            Out::Map(_) | Out::UpdateMap(_) => "map",
        })
    }
}

/// Captures the meta data of the current event.
fn capture_meta(state: &State) -> Option<Box<Meta>> {
    let span = match (state.input_range(), state.get::<Source>()) {
        (Some(range), Some(source)) => Some(Span::new(range, source.0.clone())),
        _ => None,
    };
    let event_data = state.capture_event_data();
    if span.is_none() && event_data.is_empty() {
        return None;
    }
    Some(Box::new(Meta::from_parts(event_data, span)))
}

/// Converts an atom into a value.
fn atom_value(atom: Atom, state: &State) -> Result<Value, Error> {
    let kind = match atom {
        Atom::Null => Kind::Null,
        Atom::Bool(value) => Kind::Bool(value),
        Atom::Str(value) => Kind::Str(value.into_owned()),
        Atom::Lexical(value) => Kind::Lexical(value.into_owned()),
        Atom::Bytes(value) => Kind::Bytes(owned_bytes(value)),
        Atom::Char(value) => Kind::Char(value),
        Atom::U64(value) => Kind::U64(value),
        Atom::I64(value) => Kind::from_i64(value),
        Atom::F32(value) => Kind::F32(value),
        Atom::F64(value) => Kind::F64(value),
        Atom::Ext(value) => Kind::from_ext(value),
        Atom::Implicit(value) => Kind::Implicit(value.to_static()),
        other => return Err(other.unexpected_error("any value")),
    };
    Ok(Value {
        kind,
        meta: capture_meta(state),
    })
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(ValueSink::new(Out::Value(out)), state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("any value")
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

    /// Updates the value: a map merges the data into it if it's a map (the
    /// values of keys that exist are replaced), all other values are
    /// replaced.
    fn deserialize_update<'out>(value: &'out mut Self, state: &mut State) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(ValueSink::new(Out::UpdateValue(value)), state)
    }
}

impl<'de> Deserialize<'de> for Seq {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(ValueSink::new(Out::Seq(out)), state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("sequence")
    }
}

impl<'de> Deserialize<'de> for Map {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(ValueSink::new(Out::Map(out)), state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("map")
    }

    /// Merges the data into the map, the values of keys that exist are
    /// replaced.
    fn deserialize_update<'out>(value: &'out mut Self, state: &mut State) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(ValueSink::new(Out::UpdateMap(value)), state)
    }
}
