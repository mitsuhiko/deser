//! Test helpers shared by the integration tests.
#![allow(dead_code)]

use deser::State;
use deser::de::{Deserialize, Sink, SinkHandle, default_atom};
use deser::ext::ExtValue;
use deser::ser::{Emit, MapEmitter, SeqEmitter, Serialize, SerializeHandle};
use deser::{Atom, Error, ImplicitValue};
use deser_php::{Reference, Visibility, set_class, set_visibility, take_class, take_visibility};

/// Decodes a hex string.
pub fn hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex string {:?}", s);
    (0..s.len())
        .step_by(2)
        .map(|idx| u8::from_str_radix(&s[idx..idx + 2], 16).unwrap())
        .collect()
}

/// A dynamic PHP value.
///
/// This exists only for the tests: it captures everything the deserializer
/// emits (including classes and the visibility of properties) and
/// serializes it back.
#[derive(Debug, Clone)]
pub enum Php {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    List(Vec<Php>),
    /// An array or object (with its class).
    Map(Option<String>, Vec<(Key, Php)>),
    /// An enum case (a string) or custom serialized object (bytes).
    Classed(String, Box<Php>),
    Ref(Reference),
}

/// A key of an array or a property name with its visibility.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub name: Php,
    pub visibility: Option<Visibility>,
}

impl PartialEq for Php {
    fn eq(&self, other: &Php) -> bool {
        match (self, other) {
            // floats are the same if their bits are (`-0` is not `0`)
            (Php::Float(a), Php::Float(b)) => {
                a.to_bits() == b.to_bits() || a.is_nan() && b.is_nan()
            }
            (Php::Null, Php::Null) => true,
            (Php::Bool(a), Php::Bool(b)) => a == b,
            (Php::Int(a), Php::Int(b)) => a == b,
            (Php::Str(a), Php::Str(b)) => a == b,
            (Php::Bytes(a), Php::Bytes(b)) => a == b,
            (Php::List(a), Php::List(b)) => a == b,
            (Php::Map(a, b), Php::Map(c, d)) => a == c && b == d,
            (Php::Classed(a, b), Php::Classed(c, d)) => a == c && b == d,
            (Php::Ref(a), Php::Ref(b)) => a == b,
            _ => false,
        }
    }
}

impl Serialize for Php {
    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(match *this {
            Php::Null => Atom::Null,
            Php::Bool(value) => Atom::Bool(value),
            Php::Int(value) => Atom::I64(value),
            Php::Float(value) => Atom::F64(value),
            Php::Str(ref value) => Atom::Str(value.as_str().into()),
            Php::Bytes(ref value) => Atom::Bytes(value.as_slice().into()),
            Php::Ref(ref value) => Atom::Ext(ExtValue::borrowed(value)),
            Php::List(ref items) => return Ok(Emit::seq(ListEmitter(items.iter()), state)),
            Php::Map(ref class, ref entries) => {
                let emit = Emit::map(EntryEmitter(entries.iter(), None), state);
                if let Some(class) = class {
                    set_class(state, class.as_str());
                }
                return Ok(emit);
            }
            Php::Classed(ref class, ref value) => {
                let emit = Php::serialize(value, state)?;
                set_class(state, class.as_str());
                return Ok(emit);
            }
        }))
    }
}

impl Serialize for Key {
    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        let emit = Php::serialize(&this.name, state)?;
        if let Some(ref visibility) = this.visibility {
            set_visibility(state, visibility.clone());
        }
        Ok(emit)
    }
}

struct ListEmitter<'a>(std::slice::Iter<'a, Php>);

impl SeqEmitter for ListEmitter<'_> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(SerializeHandle::to))
    }
}

struct EntryEmitter<'a>(std::slice::Iter<'a, (Key, Php)>, Option<&'a Php>);

impl MapEmitter for EntryEmitter<'_> {
    fn next_key(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(|(key, value)| {
            self.1 = Some(value);
            SerializeHandle::to(key)
        }))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SerializeHandle<'_>, Error> {
        Ok(SerializeHandle::to(self.1.unwrap()))
    }
}

/// Converts an atom.
fn atom_value(atom: Atom) -> Option<Php> {
    Some(match atom {
        Atom::Null => Php::Null,
        Atom::Bool(value) => Php::Bool(value),
        Atom::Str(value) => Php::Str(value.into_owned()),
        Atom::Bytes(value) => Php::Bytes(value.into_owned()),
        Atom::U64(value) => Php::Int(value.try_into().unwrap()),
        Atom::I64(value) => Php::Int(value),
        Atom::F64(value) => Php::Float(value),
        Atom::Implicit(value) => match value.value() {
            ImplicitValue::U64(value) => Php::Int(value.try_into().unwrap()),
            ImplicitValue::I64(value) => Php::Int(value),
            other => panic!("unexpected implicit value {:?}", other),
        },
        Atom::Ext(ref ext) => Php::Ref(*ext.downcast_ref::<Reference>()?),
        _ => return None,
    })
}

impl<'de> Deserialize<'de> for Php {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            PhpSink {
                out,
                compound: None,
                class: None,
                key: None,
                value: None,
            },
            state,
        )
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(KeySink { out }, state)
    }
}

struct KeySink<'a> {
    out: &'a mut Option<Key>,
}

impl<'de> Sink<'de> for KeySink<'_> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let visibility = take_visibility(state);
        let name = match atom_value(atom.clone()) {
            Some(name) => name,
            None => return default_atom(self, atom, state),
        };
        *self.out = Some(Key { name, visibility });
        Ok(())
    }
}

enum Compound {
    List(Vec<Php>),
    Map(Vec<(Key, Php)>),
}

struct PhpSink<'a> {
    out: &'a mut Option<Php>,
    compound: Option<Compound>,
    class: Option<String>,
    key: Option<Key>,
    value: Option<Php>,
}

impl PhpSink<'_> {
    fn flush(&mut self) {
        match self.compound {
            Some(Compound::List(ref mut items)) => items.extend(self.value.take()),
            Some(Compound::Map(ref mut entries)) => {
                if let (Some(key), Some(value)) = (self.key.take(), self.value.take()) {
                    entries.push((key, value));
                }
            }
            None => {}
        }
    }
}

impl<'de> Sink<'de> for PhpSink<'_> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let class = take_class(state);
        let value = match atom_value(atom.clone()) {
            Some(value) => value,
            None => return default_atom(self, atom, state),
        };
        *self.out = Some(match class {
            Some(class) => Php::Classed(class, Box::new(value)),
            None => value,
        });
        Ok(())
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.class = take_class(state);
        self.compound = Some(Compound::Map(Vec::new()));
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.class = take_class(state);
        assert!(self.class.is_none(), "sequence with class");
        self.compound = Some(Compound::List(Vec::new()));
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.flush();
        Ok(Key::deserialize_into(&mut self.key, state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        if let Some(Compound::List(_)) = self.compound {
            self.flush();
        }
        Ok(Php::deserialize_into(&mut self.value, state))
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        self.flush();
        match self.compound.take() {
            Some(Compound::List(items)) => *self.out = Some(Php::List(items)),
            Some(Compound::Map(entries)) => *self.out = Some(Php::Map(self.class.take(), entries)),
            None => {}
        }
        Ok(())
    }
}
