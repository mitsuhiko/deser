//! Globals, references and the information passed on out of band.
use alloc::borrow::Cow;
use alloc::string::String;
use core::fmt;

use deser_core::State;
use deser_core::de::{Deserialize, OwnedSink, Sink, SinkHandle, Slot, default_atom};
use deser_core::ext::{ExtValue, Extension};
use deser_core::ser::{Describe, Emit, Serialize};
use deser_core::{Atom, ContainerShape, Error, Text};

/// A global: a class or function referred to by its module and name.
///
/// Pickles refer to classes and functions by name (with the `GLOBAL` and
/// `STACK_GLOBAL` opcodes), they are never imported.  A global is the
/// class of an [object](crate#objects) (see [`take_class`]) and globals
/// that are values themselves (a pickled class) are passed on as extension
/// atoms of this type.  The fallback of the extension is the dotted path
/// (`module.name`) as string:
///
/// ```
/// use deser_pickle::Global;
///
/// // `pickle.dumps(collections.OrderedDict, 4)`
/// let input = b"\x80\x04\x95\x1f\x00\x00\x00\x00\x00\x00\x00\x8c\x0bcollections\x94\x8c\x0bOrderedDict\x94\x93\x94.";
/// let global: Global = deser_pickle::from_slice(input).unwrap();
/// assert_eq!(global.module(), "collections");
/// assert_eq!(global.name(), "OrderedDict");
/// let path: String = deser_pickle::from_slice(input).unwrap();
/// assert_eq!(path, "collections.OrderedDict");
/// ```
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Global {
    /// `module.name`
    path: String,
    /// The length of the module in `path`.
    split: usize,
}

impl Global {
    /// Creates a global from its module and name.
    pub fn new(module: &str, name: &str) -> Global {
        let mut path = String::with_capacity(module.len() + name.len() + 1);
        path.push_str(module);
        path.push('.');
        path.push_str(name);
        Global {
            path,
            split: module.len(),
        }
    }

    /// Returns the module (like `collections`).
    pub fn module(&self) -> &str {
        &self.path[..self.split]
    }

    /// Returns the name in the module (like `OrderedDict`).
    ///
    /// Names of nested classes contain dots (`Outer.Inner`).
    pub fn name(&self) -> &str {
        &self.path[self.split + 1..]
    }

    /// Returns the dotted path (`module.name`).
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl fmt::Debug for Global {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Global({})", self.path)
    }
}

impl fmt::Display for Global {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.path)
    }
}

impl Extension for Global {
    fn name(&self) -> &str {
        "python global"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::Str(Text::borrowed(&self.path))
    }
}

impl Serialize for Global {
    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::Ext(ExtValue::borrowed(value))))
    }
}

impl<'de> Deserialize<'de> for Global {
    fn deserialize_atom(slot: &mut Slot<Self>, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Ext(ref ext) => match ext.downcast_ref::<Global>() {
                Some(value) => {
                    slot.set(value.clone());
                    Ok(())
                }
                None => default_atom(slot, atom, state),
            },
            other => default_atom(slot, other, state),
        }
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("python global")
    }
}

/// A reference to a value that contains it (a cycle).
///
/// Pickles are graphs: a value can be reached more than once and can
/// contain itself.  Values that are reached more than once are emitted at
/// every place (see [References](crate#references)), with an id as event
/// data (see [`take_shared_id`]).  Where a value is reached again from
/// within itself it cannot be emitted again, a reference with the id of the
/// value is emitted instead.
///
/// The fallback of the extension is `null`: types that do not understand
/// references see them as missing values (an `Option` is `None`).
///
/// ```
/// use deser_pickle::Reference;
///
/// // `x = []; x.append(x)`
/// let input = b"\x80\x04\x95\x06\x00\x00\x00\x00\x00\x00\x00]\x94h\x00a.";
/// let value: Vec<Reference> = deser_pickle::from_slice(input).unwrap();
/// assert_eq!(value.len(), 1);
/// let value: Vec<Option<u32>> = deser_pickle::from_slice(input).unwrap();
/// assert_eq!(value, [None]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reference {
    id: u64,
}

impl Reference {
    /// Creates a reference to the value with the given id.
    pub const fn new(id: u64) -> Reference {
        Reference { id }
    }

    /// Returns the id of the value it refers to (see [`take_shared_id`]).
    pub const fn id(self) -> u64 {
        self.id
    }
}

impl Extension for Reference {
    fn name(&self) -> &str {
        "pickle reference"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::Null
    }
}

impl Serialize for Reference {
    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::Ext(ExtValue::borrowed(value))))
    }
}

impl<'de> Deserialize<'de> for Reference {
    fn deserialize_atom(slot: &mut Slot<Self>, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Ext(ref ext) => match ext.downcast_ref::<Reference>() {
                Some(&value) => {
                    slot.set(value);
                    Ok(())
                }
                None => default_atom(slot, atom, state),
            },
            other => default_atom(slot, other, state),
        }
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("pickle reference")
    }
}

/// The Python type of a value where the data model does not tell.
///
/// Tuples, sets and frozensets are sequences and bytearrays are bytes.
/// The deserializer attaches the kind to their first event (see
/// [`take_kind`]), the serializer writes these types if it's set (see
/// [`set_kind`]).  Lists, dicts and bytes have no kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// A `tuple` (a sequence).
    Tuple,
    /// A `set` (a sequence).
    Set,
    /// A `frozenset` (a sequence).
    FrozenSet,
    /// A `bytearray` (bytes).
    ByteArray,
}

/// How an object is created from the value it's emitted as.
///
/// The deserializer attaches the form to the first event of an
/// [object](crate#objects) together with its class (see [`take_form`]).
/// The serializer creates objects in this form (see [`set_form`]) which
/// makes them read back as they were.  Without a form, objects that are
/// maps are created from their state, lists from their items, tuples from
/// their arguments and other values are the argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Form {
    /// The value is the state: `cls.__new__(cls)` and `__setstate__` (or
    /// the attributes for maps).  This is how Python pickles instances of
    /// classes (including dataclasses).
    State,
    /// The value is the state of an object with slots: `cls.__new__(cls)`
    /// and the entries of the map are set as attributes.
    Slots,
    /// The value is the items: `cls.__new__(cls)` and `__setitem__` for
    /// maps, `extend` (or `append`) for sequences.  This is how Python
    /// pickles subclasses of `dict` and `list`.
    Items,
    /// The value is the arguments: `cls(*value)` for sequences and
    /// `cls.__new__(cls, **value)` for maps (which requires protocol 4).
    Arguments,
    /// The value is the only argument: `cls(value)`.
    Argument,
}

/// The class of a value, attached as event data to its first event.
#[derive(Debug, Default, Clone)]
pub(crate) struct ClassData(pub(crate) Option<Global>);

/// The form of an object, attached as event data to its first event.
#[derive(Debug, Default, Clone)]
pub(crate) struct FormData(pub(crate) Option<Form>);

/// The kind of a value, attached as event data to its first event.
#[derive(Debug, Default, Clone)]
pub(crate) struct KindData(pub(crate) Option<Kind>);

/// The id of a value that is reached more than once.
#[derive(Debug, Default, Clone)]
pub(crate) struct SharedIdData(pub(crate) Option<u64>);

/// Takes the class of the current value from the state.
///
/// This is intended to be called by sinks from within [`Sink::atom`],
/// [`Sink::map`] or [`Sink::seq`].  Returns `None` if the value is not an
/// [object](crate#objects) or the data format is not pickle.
pub fn take_class(state: &mut State) -> Option<Global> {
    state.take_event::<ClassData>().and_then(|class| class.0)
}

/// Sets the class of the value that is serialized.
///
/// It must be called from [`Serialize::serialize`] and applies to the
/// value serialized from that call (see [`State::event`]).  The value is
/// written as an object of the class (see
/// [Serialization](crate#serialization)).  Serializers of other formats
/// ignore it.
pub fn set_class(state: &mut State, class: Global) {
    state.event_mut::<ClassData>().0 = Some(class);
}

/// Takes the form of the current object from the state.
///
/// Returns `None` for values that are not objects.  See [`Form`].
pub fn take_form(state: &mut State) -> Option<Form> {
    state.take_event::<FormData>().and_then(|form| form.0)
}

/// Sets the form of the object that is serialized.
///
/// It must be called from [`Serialize::serialize`] together with
/// [`set_class`].  See [`Form`].
pub fn set_form(state: &mut State, form: Form) {
    state.event_mut::<FormData>().0 = Some(form);
}

/// Takes the kind of the current value from the state.
///
/// Returns `None` for values that have no kind (see [`Kind`]).
pub fn take_kind(state: &mut State) -> Option<Kind> {
    state.take_event::<KindData>().and_then(|kind| kind.0)
}

/// Sets the kind of the value that is serialized.
///
/// It must be called from [`Serialize::serialize`].  Sequences are written
/// as tuples, sets or frozensets and bytes as bytearrays.
pub fn set_kind(state: &mut State, kind: Kind) {
    state.event_mut::<KindData>().0 = Some(kind);
}

/// Takes the id of the current value if it's reached more than once.
///
/// Values that are reached more than once (shared values) are emitted at
/// every place with the same id.  Returns `None` for values that are only
/// reached once.  See [References](crate#references).
pub fn take_shared_id(state: &mut State) -> Option<u64> {
    state.take_event::<SharedIdData>().and_then(|id| id.0)
}

/// Sets the id of the value that is serialized.
///
/// The serializer writes a value with an id once: other values with the
/// same id are written as references to the first and a [`Reference`] with
/// the id refers to it.  It must be called from [`Serialize::serialize`].
pub fn set_shared_id(state: &mut State, id: u64) {
    state.event_mut::<SharedIdData>().0 = Some(id);
}

/// A value with the class of a Python object.
///
/// When deserialized the class of the value (if there is one) is captured,
/// when serialized the value is written as an object of the class:
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_pickle::{Global, Object};
///
/// // a `Point(x=1, y=2)` of a module `geometry`
/// let input = b"\x80\x04\x95*\x00\x00\x00\x00\x00\x00\x00\x8c\x08geometry\x94\x8c\x05Point\x94\x93\x94)\x81\x94}\x94(\x8c\x01x\x94K\x01\x8c\x01y\x94K\x02ub.";
/// let point: Object<BTreeMap<String, i32>> = deser_pickle::from_slice(input).unwrap();
/// assert_eq!(point.class, Some(Global::new("geometry", "Point")));
/// assert_eq!(point.value["y"], 2);
/// ```
///
/// Other formats do not have classes.  When deserializing from such
/// formats, the class is `None`, when serializing it's ignored.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Object<T> {
    /// The class of the value.
    pub class: Option<Global>,
    /// How the object is created from the value (see [`Form`]).
    pub form: Option<Form>,
    /// The value.
    pub value: T,
}

impl<T> Object<T> {
    /// Creates a value with a class.
    pub fn new(class: Global, value: T) -> Object<T> {
        Object {
            class: Some(class),
            form: None,
            value,
        }
    }

    /// Creates a value with a class and the form the object is created in.
    pub fn with_form(class: Global, form: Form, value: T) -> Object<T> {
        Object {
            class: Some(class),
            form: Some(form),
            value,
        }
    }

    /// Returns the inner value.
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: fmt::Debug> fmt::Debug for Object<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.class {
            Some(ref class) => {
                write!(f, "{} ", class)?;
                fmt::Debug::fmt(&self.value, f)
            }
            None => fmt::Debug::fmt(&self.value, f),
        }
    }
}

impl<T: Serialize> Serialize for Object<T> {
    fn serialize<'a>(this: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        // the class is set after the value attached its data, it replaces it
        let emit = T::serialize(&this.value, state)?;
        if let Some(ref class) = this.class {
            set_class(state, class.clone());
            if let Some(form) = this.form {
                set_form(state, form);
            }
        }
        Ok(emit)
    }

    fn finish(this: &Self, state: &mut State) -> Result<(), Error> {
        T::finish(&this.value, state)
    }

    fn is_optional(this: &Self) -> bool {
        T::is_optional(&this.value)
    }

    fn container_shape(this: &Self) -> ContainerShape {
        T::container_shape(&this.value)
    }

    fn describe(this: &Self, d: &mut dyn Describe) {
        T::describe(&this.value, d)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            ObjectSink {
                out,
                slot: None,
                compound: None,
                class: None,
                form: None,
            },
            state,
        )
    }

    fn expecting() -> Cow<'static, str> {
        T::expecting()
    }
}

struct ObjectSink<'a, 'de, T> {
    out: &'a mut Option<Object<T>>,
    // atoms are deserialized directly into this slot, maps and sequences
    // need a sink that lives across calls
    slot: Option<T>,
    compound: Option<OwnedSink<'de, T>>,
    class: Option<Global>,
    form: Option<Form>,
}

impl<'a, 'de, T: Deserialize<'de>> ObjectSink<'a, 'de, T> {
    /// Takes the class and form of the value.
    fn take(&mut self, state: &mut State) {
        self.class = take_class(state);
        self.form = take_form(state);
    }

    fn compound(&mut self, state: &mut State) -> &mut dyn Sink<'de> {
        self.compound
            .get_or_insert_with(|| OwnedSink::deserialize(state))
            .get_mut()
    }
}

impl<'a, 'de, T: Deserialize<'de>> Sink<'de> for ObjectSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.take(state);
        let mut sink = T::deserialize_into(&mut self.slot, state);
        sink.atom(atom, state)?;
        sink.finish(state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.take(state);
        let mut sink = T::deserialize_into(&mut self.slot, state);
        sink.borrowed_atom(atom, state)?;
        sink.finish(state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.take(state);
        self.compound(state).map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.take(state);
        self.compound(state).seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.compound(state).next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.compound(state).next_value(state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.compound(state).value_for_key(key, state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        match self.compound {
            Some(ref mut compound) => compound.get_mut().recover(err, state),
            None => Err(err),
        }
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let value = match self.compound {
            Some(ref mut compound) => {
                compound.get_mut().finish(state)?;
                compound.take()
            }
            None => self.slot.take(),
        };
        let class = self.class.take();
        let form = self.form.take();
        *self.out = value.map(|value| Object { class, form, value });
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        if let Some(ref compound) = self.compound {
            return compound.get().expecting();
        }
        T::expecting()
    }
}
