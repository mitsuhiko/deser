//! Test helpers shared by the integration tests.
#![allow(dead_code)]

use std::collections::HashMap;

use deser::State;
use deser::de::{Deserialize, Sink, SinkHandle, default_atom};
use deser::ext::{BigInt, ExtValue};
use deser::ser::{Emit, MapEmitter, SeqEmitter, Serialize, SerializeHandle};
use deser::{Atom, Error};
use deser_pickle::{
    Form, Global, Kind, Reference, set_class, set_form, set_kind, set_shared_id, take_class,
    take_form, take_kind, take_shared_id,
};

/// Decodes a hex string.
pub fn hex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex string {:?}", s);
    (0..s.len())
        .step_by(2)
        .map(|idx| u8::from_str_radix(&s[idx..idx + 2], 16).unwrap())
        .collect()
}

/// Encodes bytes as hex.
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// A dynamic Python value.
///
/// This exists only for the tests: it captures everything the deserializer
/// emits (including classes, kinds and ids) and serializes it back.
#[derive(Debug, Clone)]
pub enum Py {
    Null,
    Bool(bool),
    /// An integer as decimal text.
    Int(String),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    ByteArray(Vec<u8>),
    Seq(Option<Kind>, Vec<Py>),
    Map(Vec<(Py, Py)>),
    Global(Global),
    Ref(u64),
    /// A value with an id (reached more than once).
    Shared(u64, Box<Py>),
    /// A value with a class (and the form of the object).
    Object(Global, Option<Form>, Box<Py>),
}

impl PartialEq for Py {
    fn eq(&self, other: &Py) -> bool {
        match (self, other) {
            (Py::Float(a), Py::Float(b)) => a.to_bits() == b.to_bits() || a.is_nan() && b.is_nan(),
            (Py::Null, Py::Null) => true,
            (Py::Bool(a), Py::Bool(b)) => a == b,
            (Py::Int(a), Py::Int(b)) => a == b,
            (Py::Str(a), Py::Str(b)) => a == b,
            (Py::Bytes(a), Py::Bytes(b)) => a == b,
            (Py::ByteArray(a), Py::ByteArray(b)) => a == b,
            (Py::Seq(a, b), Py::Seq(c, d)) => a == c && b == d,
            (Py::Map(a), Py::Map(b)) => a == b,
            (Py::Global(a), Py::Global(b)) => a == b,
            (Py::Ref(a), Py::Ref(b)) => a == b,
            (Py::Shared(a, b), Py::Shared(c, d)) => a == c && b == d,
            (Py::Object(a, b, c), Py::Object(d, e, f)) => a == d && b == e && c == f,
            _ => false,
        }
    }
}

impl Py {
    /// Renumbers the ids in the order they appear.
    pub fn normalized(&self) -> Py {
        fn walk(value: &Py, ids: &mut HashMap<u64, u64>) -> Py {
            let mut id = |x: u64| {
                let next = ids.len() as u64;
                *ids.entry(x).or_insert(next)
            };
            match value {
                Py::Ref(x) => Py::Ref(id(*x)),
                Py::Shared(x, inner) => {
                    let x = id(*x);
                    Py::Shared(x, Box::new(walk(inner, ids)))
                }
                Py::Object(class, form, inner) => {
                    Py::Object(class.clone(), *form, Box::new(walk(inner, ids)))
                }
                Py::Seq(kind, items) => {
                    Py::Seq(*kind, items.iter().map(|x| walk(x, ids)).collect())
                }
                Py::Map(entries) => Py::Map(
                    entries
                        .iter()
                        .map(|(k, v)| (walk(k, ids), walk(v, ids)))
                        .collect(),
                ),
                other => other.clone(),
            }
        }
        walk(self, &mut HashMap::new())
    }

    /// Returns `true` if the value contains itself through a value that the
    /// serializer can only refer to once it's complete (like a tuple).
    pub fn has_late_cycle(&self, protocol: u8) -> bool {
        fn is_late(value: &Py, protocol: u8) -> bool {
            match value {
                Py::Seq(Some(Kind::Tuple | Kind::FrozenSet), _) => true,
                Py::Seq(Some(Kind::Set), _) => protocol < 4,
                Py::Object(_, form, inner) => match form {
                    Some(Form::State | Form::Slots | Form::Items) => false,
                    Some(Form::Arguments) => !matches!(**inner, Py::Map(_)) || protocol >= 4,
                    Some(Form::Argument) => true,
                    None => !matches!(**inner, Py::Map(_) | Py::Seq(None, _)),
                },
                _ => false,
            }
        }
        fn walk(value: &Py, open: &mut Vec<(u64, bool)>, protocol: u8) -> bool {
            match value {
                Py::Ref(id) => open.iter().any(|&(x, late)| x == *id && late),
                Py::Shared(id, inner) => {
                    open.push((*id, is_late(inner, protocol)));
                    let rv = walk(inner, open, protocol);
                    open.pop();
                    rv
                }
                Py::Object(_, _, inner) => walk(inner, open, protocol),
                Py::Seq(_, items) => items.iter().any(|x| walk(x, open, protocol)),
                Py::Map(entries) => entries
                    .iter()
                    .any(|(k, v)| walk(k, open, protocol) || walk(v, open, protocol)),
                _ => false,
            }
        }
        walk(self, &mut Vec::new(), protocol)
    }
}

impl Serialize for Py {
    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(match *this {
            Py::Null => Atom::Null,
            Py::Bool(value) => Atom::Bool(value),
            Py::Int(ref value) => match value.parse::<i64>() {
                Ok(value) => Atom::I64(value),
                Err(_) => value.parse::<BigInt>().unwrap().into_atom(),
            },
            Py::Float(value) => Atom::F64(value),
            Py::Str(ref value) => Atom::Str(value.as_str().into()),
            Py::Bytes(ref value) => Atom::Bytes(value.as_slice().into()),
            Py::ByteArray(ref value) => {
                set_kind(state, Kind::ByteArray);
                Atom::Bytes(value.as_slice().into())
            }
            Py::Global(ref value) => Atom::Ext(ExtValue::borrowed(value)),
            Py::Ref(id) => Atom::Ext(ExtValue::owned(Reference::new(id))),
            Py::Seq(kind, ref items) => {
                let emit = Emit::seq(ListEmitter(items.iter()), state);
                if let Some(kind) = kind {
                    set_kind(state, kind);
                }
                return Ok(emit);
            }
            Py::Map(ref entries) => {
                return Ok(Emit::map(EntryEmitter(entries.iter(), None), state));
            }
            Py::Shared(id, ref inner) => {
                let emit = Py::serialize(inner, state)?;
                set_shared_id(state, id);
                return Ok(emit);
            }
            Py::Object(ref class, form, ref inner) => {
                let emit = Py::serialize(inner, state)?;
                set_class(state, class.clone());
                if let Some(form) = form {
                    set_form(state, form);
                }
                return Ok(emit);
            }
        }))
    }
}

struct ListEmitter<'a>(std::slice::Iter<'a, Py>);

impl SeqEmitter for ListEmitter<'_> {
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        Ok(self.0.next().map(SerializeHandle::to))
    }
}

struct EntryEmitter<'a>(std::slice::Iter<'a, (Py, Py)>, Option<&'a Py>);

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
fn atom_value(atom: &Atom) -> Option<Py> {
    Some(match *atom {
        Atom::Null => Py::Null,
        Atom::Bool(value) => Py::Bool(value),
        Atom::Str(ref value) => Py::Str(value.as_str().to_string()),
        Atom::Bytes(ref value) => Py::Bytes(value.data().to_vec()),
        Atom::U64(value) => Py::Int(value.to_string()),
        Atom::I64(value) => Py::Int(value.to_string()),
        Atom::F64(value) => Py::Float(value),
        Atom::Ext(ref ext) => {
            if let Some(value) = ext.downcast_ref::<Reference>() {
                Py::Ref(value.id())
            } else if let Some(value) = ext.downcast_ref::<Global>() {
                Py::Global(value.clone())
            } else if let Some(value) = ext.downcast_ref::<u128>() {
                Py::Int(value.to_string())
            } else if let Some(value) = ext.downcast_ref::<i128>() {
                Py::Int(value.to_string())
            } else {
                Py::Int(ext.downcast_ref::<BigInt>()?.to_string())
            }
        }
        _ => return None,
    })
}

/// The data of the first event of a value.
#[derive(Default)]
struct Data {
    class: Option<Global>,
    form: Option<Form>,
    kind: Option<Kind>,
    shared: Option<u64>,
}

impl Data {
    fn take(state: &mut State) -> Data {
        Data {
            class: take_class(state),
            form: take_form(state),
            kind: take_kind(state),
            shared: take_shared_id(state),
        }
    }

    fn wrap(self, mut value: Py) -> Py {
        if let Some(class) = self.class {
            value = Py::Object(class, self.form, Box::new(value));
        }
        if let Some(id) = self.shared {
            value = Py::Shared(id, Box::new(value));
        }
        value
    }
}

impl<'de> Deserialize<'de> for Py {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            PySink {
                out,
                compound: None,
                data: Data::default(),
                key: None,
                value: None,
            },
            state,
        )
    }
}

enum Compound {
    Seq(Vec<Py>),
    Map(Vec<(Py, Py)>),
}

struct PySink<'a> {
    out: &'a mut Option<Py>,
    compound: Option<Compound>,
    data: Data,
    key: Option<Py>,
    value: Option<Py>,
}

impl PySink<'_> {
    fn flush(&mut self) {
        match self.compound {
            Some(Compound::Seq(ref mut items)) => items.extend(self.value.take()),
            Some(Compound::Map(ref mut entries)) => {
                if let (Some(key), Some(value)) = (self.key.take(), self.value.take()) {
                    entries.push((key, value));
                }
            }
            None => {}
        }
    }
}

impl<'de> Sink<'de> for PySink<'_> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let data = Data::take(state);
        let mut value = match atom_value(&atom) {
            Some(value) => value,
            None => return default_atom(self, atom, state),
        };
        if data.kind == Some(Kind::ByteArray)
            && let Py::Bytes(bytes) = value
        {
            value = Py::ByteArray(bytes);
        }
        *self.out = Some(data.wrap(value));
        Ok(())
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.data = Data::take(state);
        self.compound = Some(Compound::Map(Vec::new()));
        Ok(())
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.data = Data::take(state);
        self.compound = Some(Compound::Seq(Vec::new()));
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.flush();
        Ok(Py::deserialize_into(&mut self.key, state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        if let Some(Compound::Seq(_)) = self.compound {
            self.flush();
        }
        Ok(Py::deserialize_into(&mut self.value, state))
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        self.flush();
        let data = std::mem::take(&mut self.data);
        let value = match self.compound.take() {
            Some(Compound::Seq(items)) => Py::Seq(data.kind, items),
            Some(Compound::Map(entries)) => Py::Map(entries),
            None => return Ok(()),
        };
        *self.out = Some(data.wrap(value));
        Ok(())
    }
}
