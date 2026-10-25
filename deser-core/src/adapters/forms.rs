//! Adapters that accept or write values in another form: [`OneOrMany`],
//! [`PickFirst`], [`MapAsPairs`] and [`PairsAsMap`].
use alloc::borrow::Cow;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(feature = "std")]
use core::hash::{BuildHasher, Hash};
use core::marker::PhantomData;
use core::mem::take;
#[cfg(feature = "std")]
use std::collections::{HashMap, HashSet};

use crate::State;
use crate::Text;
use crate::adapted::{self, Adapted};
use crate::adapters::Same;
use crate::de::impls::{MapOut, MapTarget, map_sink};
use crate::de::recording::{Capture, RecordBuf};
use crate::de::update::Collection;
use crate::de::{CollectedErrors, Deserialize, DuplicateKeys, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::{Atom, ContainerShape, Order};
use crate::extensions::EventData;
use crate::ser::impls::{MapIterEmitter, handle_as};
use crate::ser::{Emit, IndexedSeq, IndexedSeqEmitter, SeqEmitter, Serialize, SerializeHandle};

/// A collection that also accepts a single element.
///
/// When deserializing, a sequence is the collection and every other value
/// (an atom or a map) is a single element, which becomes a collection of
/// one.  The elements are deserialized with the adapter `A` (by default
/// [`Same`]).  This is for data that writes a list with one element as the
/// element itself (`"tags": "a"` and `"tags": ["a", "b"]`).  As a sequence
/// is always the collection, elements cannot be sequences themselves.
///
/// When serializing, `F` decides the form: [`PreferOne`] (the default)
/// writes a collection with exactly one element as that element and
/// [`PreferMany`] always writes a sequence.
///
/// Supported are `Vec<T>`, `VecDeque<T>`, `BTreeSet<T>` and `HashSet<T>`.
///
/// ```
/// use deser::adapters::{OneOrMany, PreferMany};
/// use deser::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize, Debug, PartialEq)]
/// pub struct Rule {
///     #[deser(as = OneOrMany)]
///     hosts: Vec<String>,
///     #[deser(as = OneOrMany<_, PreferMany>)]
///     ports: Vec<u16>,
/// }
///
/// let rule: Rule = deser_json::from_str(r#"{"hosts": "a", "ports": 80}"#).unwrap();
/// assert_eq!(rule.hosts, ["a"]);
/// assert_eq!(rule.ports, [80]);
/// assert_eq!(
///     deser_json::to_string(&rule).unwrap(),
///     r#"{"hosts":"a","ports":[80]}"#
/// );
/// ```
///
/// In formats where keys can be given more than once (like query strings,
/// see [`ContainerShape::set_multimap`]) the values of all occurrences of
/// the key are collected, as without the adapter.
pub struct OneOrMany<A = Same, F = PreferOne>(PhantomData<fn() -> (A, F)>);

/// Serializes a [`OneOrMany`] collection with one element as the element.
pub struct PreferOne;

/// Serializes a [`OneOrMany`] collection always as sequence.
pub struct PreferMany;

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::PreferOne {}
    impl Sealed for super::PreferMany {}
}

/// The forms of [`OneOrMany`]: [`PreferOne`] and [`PreferMany`].
///
/// This trait is sealed.
pub trait OneOrManyForm: sealed::Sealed + 'static {
    #[doc(hidden)]
    const PREFER_ONE: bool;
}

impl OneOrManyForm for PreferOne {
    const PREFER_ONE: bool = true;
}

impl OneOrManyForm for PreferMany {
    const PREFER_ONE: bool = false;
}

/// The sink of [`OneOrMany`].
///
/// The first event decides if the value is the collection or a single
/// element.
enum OneOrManySink<'a, 'de, C> {
    Pending(&'a mut Option<C>),
    Active(SinkHandle<'a, 'de>),
}

impl<'a, 'de, C: Send> OneOrManySink<'a, 'de, C> {
    /// Returns the sink of the value, creates it if needed.
    ///
    /// For a sequence that is the sink of the collection (with the adapter
    /// `M`), otherwise the sink of an element that is added to the
    /// collection.
    fn active<T, A, M>(&mut self, many: bool, state: &mut State) -> &mut SinkHandle<'a, 'de>
    where
        C: Collection<T>,
        T: Send + 'a,
        A: Deserialize<'de, T>,
        M: Deserialize<'de, C>,
    {
        if let OneOrManySink::Pending(..) = self
            && let OneOrManySink::Pending(out) =
                core::mem::replace(self, OneOrManySink::Active(SinkHandle::null()))
        {
            *self = OneOrManySink::Active(if many {
                M::deserialize_into(out, state)
            } else {
                crate::de::update::collect_into::<C, T, A>(out, state)
            });
        }
        match self {
            OneOrManySink::Active(sink) => sink,
            OneOrManySink::Pending(..) => unreachable!(),
        }
    }
}

/// Returns `true` if the atom is the collection itself.
///
/// This is the case for bytes if the elements are bytes (and strings,
/// which are bytes in formats without native bytes).
fn atom_is_many<'de, T: Send, A: Deserialize<'de, T>>(atom: &Atom) -> bool {
    A::__private_is_bytes() && matches!(atom, Atom::Bytes(_) | Atom::Str(_))
}

impl<'a, 'de, C, T, A, M> Sink<'de> for Adapted<(T, A, M), OneOrManySink<'a, 'de, C>>
where
    C: Collection<T>,
    T: Send + 'a,
    A: Deserialize<'de, T>,
    M: Deserialize<'de, C>,
{
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let many = atom_is_many::<T, A>(&atom);
        self.active::<T, A, M>(many, state).atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        let many = atom_is_many::<T, A>(&atom);
        self.active::<T, A, M>(many, state)
            .borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.active::<T, A, M>(false, state).map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.active::<T, A, M>(true, state).seq(state)
    }

    fn __private_seq(&mut self, state: &mut State) -> Result<bool, Error> {
        self.active::<T, A, M>(true, state).__private_seq(state)
    }

    fn __private_inline_atom(
        &mut self,
        index: usize,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        self.active::<T, A, M>(true, state)
            .__private_inline_atom(index, atom, state)
    }

    fn __private_inline_event(
        &mut self,
        event: crate::de::InlineEvent,
        state: &mut State,
    ) -> Result<(), Error> {
        self.active::<T, A, M>(true, state)
            .__private_inline_event(event, state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.active::<T, A, M>(false, state).next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.active::<T, A, M>(false, state).next_value(state)
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.active::<T, A, M>(false, state)
            .__private_key_atom(atom, state)
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.active::<T, A, M>(false, state)
            .__private_value_atom(atom, state)
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.active::<T, A, M>(false, state)
            .__private_borrowed_key_atom(atom, state)
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        self.active::<T, A, M>(false, state)
            .__private_borrowed_value_atom(atom, state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.active::<T, A, M>(false, state)
            .value_for_key(key, state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.active::<T, A, M>(false, state).recover(err, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.active::<T, A, M>(false, state).finish(state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        match &**self {
            OneOrManySink::Active(sink) => sink.expecting(),
            OneOrManySink::Pending(..) => M::expecting(),
        }
    }
}

/// Implements `OneOrMany` for collections.
///
/// The entries are `[bounds] [serialize bounds] Collection<T> =>
/// Collection<A>;` where the last part is the adapter for the collection
/// when it's a sequence.
macro_rules! one_or_many_impls {
    ($([$($bound:tt)*] [$($ser_bound:tt)*] $target:ty => $many:ty;)*) => {
        $(
            impl<'de, $($bound)*, A: Deserialize<'de, T>, F: OneOrManyForm>
                Deserialize<'de, $target> for OneOrMany<A, F>
            {
                fn deserialize_into<'out>(
                    out: &'out mut Option<$target>,
                    state: &mut State,
                ) -> SinkHandle<'out, 'de> {
                    adapted::arena_sink::<(T, A, $many), _>(OneOrManySink::Pending(out), state)
                }

                fn expecting() -> Cow<'static, str> {
                    <$many as Deserialize<'de, $target>>::expecting()
                }

                fn initial_value() -> Option<$target> {
                    <$many as Deserialize<'de, $target>>::initial_value()
                }

                #[inline]
                fn __private_atom_into(
                    out: &mut Option<$target>,
                    atom: Atom,
                    state: &mut State,
                ) -> Result<(), Error> {
                    if atom_is_many::<T, A>(&atom) {
                        return <$many as Deserialize<'de, $target>>::__private_atom_into(
                            out, atom, state,
                        );
                    }
                    let mut element = None;
                    A::__private_atom_into(&mut element, atom, state)?;
                    add_element(out, element)
                }

                #[inline]
                fn __private_borrowed_atom_into(
                    out: &mut Option<$target>,
                    atom: Atom<'de>,
                    state: &mut State,
                ) -> Result<(), Error> {
                    if atom_is_many::<T, A>(&atom) {
                        return <$many as Deserialize<'de, $target>>::__private_borrowed_atom_into(
                            out, atom, state,
                        );
                    }
                    let mut element = None;
                    A::__private_borrowed_atom_into(&mut element, atom, state)?;
                    add_element(out, element)
                }

                // the occurrences of a repeated key are collected like
                // without the adapter

                fn __private_collects() -> bool {
                    <$many as Deserialize<'de, $target>>::__private_collects()
                }

                fn __private_collect_into<'out>(
                    out: &'out mut Option<$target>,
                    state: &mut State,
                ) -> SinkHandle<'out, 'de> {
                    <$many as Deserialize<'de, $target>>::__private_collect_into(out, state)
                }

                fn __private_collect_update<'out>(
                    value: &'out mut $target,
                    first: bool,
                    state: &mut State,
                ) -> SinkHandle<'out, 'de> {
                    <$many as Deserialize<'de, $target>>::__private_collect_update(
                        value, first, state,
                    )
                }

                fn __private_collect_empty() -> Option<$target> {
                    <$many as Deserialize<'de, $target>>::__private_collect_empty()
                }
            }

            impl<$($ser_bound)*, A: Serialize<T>, F: OneOrManyForm> Serialize<$target>
                for OneOrMany<A, F>
            {
                fn serialize<'a>(
                    value: &'a $target,
                    _state: &mut State,
                ) -> Result<Emit<'a>, Error> {
                    let mut iter = value.iter();
                    Ok(Emit::Forward(match (iter.next(), iter.next()) {
                        (Some(element), None) if F::PREFER_ONE => handle_as::<A, T>(element),
                        _ => handle_as::<$many, $target>(value),
                    }))
                }

                fn describe(value: &$target, d: &mut dyn crate::ser::Describe) {
                    <$many as Serialize<$target>>::describe(value, d)
                }
            }
        )*
    };
}

/// Adds a single element to a new collection.
#[inline]
fn add_element<C: Collection<T>, T>(out: &mut Option<C>, element: Option<T>) -> Result<(), Error> {
    if let Some(element) = element {
        let mut collection = C::empty();
        collection.add(element)?;
        *out = Some(collection);
    }
    Ok(())
}

one_or_many_impls! {
    [T: Send] [T: Sync] Vec<T> => Vec<A>;
    [T: Send] [T: Sync] VecDeque<T> => VecDeque<A>;
    [T: Ord + Send] [T: Sync] BTreeSet<T> => BTreeSet<A>;
}

#[cfg(feature = "std")]
one_or_many_impls! {
    [T: Hash + Eq + Send, H: BuildHasher + Default + Send]
    [T: Sync, H: BuildHasher + Sync]
    HashSet<T, H> => HashSet<A>;
}

/// Tries adapters in order until one of them accepts the value.
///
/// The adapters are given as a tuple of two to four adapters, `_` stands
/// for [`Same`].  When deserializing, the value is delivered to the
/// adapters in order and the first one that accepts it wins.  Values other
/// than atoms are recorded and replayed for every adapter (like for
/// untagged enums, see [`Recording`](crate::de::Recording)), which keeps
/// their source locations.  If no adapter accepts the value, the error of
/// the first adapter is reported.  Missing values are handled by the first
/// adapter.  Serialization uses the first adapter.
///
/// The most common use is to accept a value in more than one form, like
/// numbers that are also sent as strings:
///
/// ```
/// use deser::adapters::{DisplayFromStr, PickFirst};
/// use deser::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize, Debug, PartialEq)]
/// pub struct Item {
///     #[deser(as = PickFirst<(_, DisplayFromStr)>)]
///     id: u64,
/// }
///
/// let a: Item = deser_json::from_str(r#"{"id": 42}"#).unwrap();
/// let b: Item = deser_json::from_str(r#"{"id": "42"}"#).unwrap();
/// assert_eq!(a, b);
/// assert_eq!(deser_json::to_string(&b).unwrap(), r#"{"id":42}"#);
/// ```
pub struct PickFirst<A>(PhantomData<fn() -> A>);

/// The value that is delivered to the adapters of [`PickFirst`].
enum PickInput<'t, 'de> {
    Atom(Atom<'t>),
    Borrowed(Atom<'de>),
    Recorded(&'t RecordBuf<'de>),
}

/// Delivers the value to one of the adapters of a [`PickFirst`].
type Attempt<'de, T> =
    for<'t> fn(&mut Option<T>, &PickInput<'t, 'de>, &EventData, &mut State) -> Result<(), Error>;

/// The adapters of a [`PickFirst`] (up to four).
struct Attempts<'de, T> {
    list: [Attempt<'de, T>; 4],
    len: usize,
}

impl<'de, T: Send> Attempts<'de, T> {
    fn new(list: &[Attempt<'de, T>]) -> Attempts<'de, T> {
        let mut rv = Attempts {
            list: [list[0]; 4],
            len: list.len(),
        };
        rv.list[..list.len()].copy_from_slice(list);
        rv
    }

    /// Delivers the value to the adapters until one of them accepts it.
    ///
    /// If none does, the value is delivered to the first adapter once more
    /// to report its error.
    fn pick(
        &self,
        out: &mut Option<T>,
        input: PickInput<'_, 'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        // sinks can take event data (like CBOR tags), every adapter gets
        // the data of the event.  Replaying a recording restores the event
        // data of the recorded events.
        let data = match input {
            PickInput::Recorded(_) => EventData::new(),
            _ => state.extensions().capture_event_data(),
        };
        for attempt in &self.list[..self.len] {
            let mut value = None;
            // only whether the adapter accepts the value matters
            let rv = state.discard_errors(|state| attempt(&mut value, &input, &data, state));
            if rv.is_ok() && value.is_some() {
                *out = value;
                return Ok(());
            }
        }
        (self.list[0])(out, &input, &data, state)
    }
}

/// Delivers the value to the adapter `A`.
fn pick_attempt<'de, T: Send, A: Deserialize<'de, T>>(
    out: &mut Option<T>,
    input: &PickInput<'_, 'de>,
    data: &EventData,
    state: &mut State,
) -> Result<(), Error> {
    if !data.is_empty() {
        state.extensions_mut().restore_event_data(data);
    }
    match *input {
        PickInput::Atom(ref atom) => A::__private_atom_into(out, atom.as_borrowed(), state),
        PickInput::Borrowed(ref atom) => A::__private_borrowed_atom_into(out, atom.clone(), state),
        PickInput::Recorded(buffer) => buffer.replay(A::deserialize_into(out, state), state),
    }
}

/// The sink of [`PickFirst`] passes the value on to this.
struct PickCapture<'a, 'de, T> {
    out: &'a mut Option<T>,
    attempts: Attempts<'de, T>,
    expecting: fn() -> Cow<'static, str>,
}

impl<'a, 'de, T: Send> Capture<'de, RecordBuf<'de>> for PickCapture<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.attempts.pick(self.out, PickInput::Atom(atom), state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.attempts
            .pick(self.out, PickInput::Borrowed(atom), state)
    }

    fn recorded(&mut self, buffer: RecordBuf<'de>, state: &mut State) -> Result<(), Error> {
        self.attempts
            .pick(self.out, PickInput::Recorded(&buffer), state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        (self.expecting)()
    }
}

macro_rules! pick_first_impls {
    ($(($first:ident $(, $adapter:ident)+);)*) => {
        $(
            impl<'de, T: Send, $first: Deserialize<'de, T>, $($adapter: Deserialize<'de, T>),+>
                Deserialize<'de, T> for PickFirst<($first, $($adapter),+)>
            {
                fn deserialize_into<'out>(
                    out: &'out mut Option<T>,
                    state: &mut State,
                ) -> SinkHandle<'out, 'de> {
                    RecordBuf::capture_with(
                        PickCapture {
                            out,
                            attempts: pick_first_impls!(@attempts $first $(, $adapter)+),
                            expecting: <Self as Deserialize<'de, T>>::expecting,
                        },
                        state,
                    )
                }

                fn expecting() -> Cow<'static, str> {
                    let mut rv = String::from($first::expecting());
                    $(
                        rv.push_str(" or ");
                        rv.push_str(&$adapter::expecting());
                    )+
                    Cow::Owned(rv)
                }

                fn initial_value() -> Option<T> {
                    $first::initial_value()
                }

                #[inline]
                fn __private_atom_into(
                    out: &mut Option<T>,
                    atom: Atom,
                    state: &mut State,
                ) -> Result<(), Error> {
                    pick_first_impls!(@attempts $first $(, $adapter)+)
                        .pick(out, PickInput::Atom(atom), state)
                }

                #[inline]
                fn __private_borrowed_atom_into(
                    out: &mut Option<T>,
                    atom: Atom<'de>,
                    state: &mut State,
                ) -> Result<(), Error> {
                    pick_first_impls!(@attempts $first $(, $adapter)+)
                        .pick(out, PickInput::Borrowed(atom), state)
                }
            }

            impl<T: ?Sized, $first: Serialize<T>, $($adapter),+> Serialize<T>
                for PickFirst<($first, $($adapter),+)>
            {
                fn serialize<'a>(value: &'a T, state: &mut State) -> Result<Emit<'a>, Error> {
                    $first::serialize(value, state)
                }

                fn finish(value: &T, state: &mut State) -> Result<(), Error> {
                    $first::finish(value, state)
                }

                fn is_optional(value: &T) -> bool {
                    $first::is_optional(value)
                }

                fn container_shape(value: &T) -> ContainerShape {
                    $first::container_shape(value)
                }

                fn describe(value: &T, d: &mut dyn crate::ser::Describe) {
                    $first::describe(value, d)
                }

                #[inline]
                fn __private_begin<'a>(
                    value: &'a T,
                    state: &mut State,
                ) -> Result<crate::ser::Begin<'a>, Error> {
                    $first::__private_begin(value, state)
                }

                fn __private_slice_as_bytes(val: &[T]) -> Option<Cow<'_, [u8]>>
                where
                    T: Sized,
                {
                    $first::__private_slice_as_bytes(val)
                }
            }
        )*
    };
    (@attempts $($adapter:ident),+) => {
        Attempts::new(&[$(pick_attempt::<T, $adapter> as Attempt<'de, T>),+])
    };
}

pick_first_impls! {
    (A1, A2);
    (A1, A2, A3);
    (A1, A2, A3, A4);
}

/// A map that is written as a sequence of key and value pairs.
///
/// When serializing, the entries are written as a sequence of pairs
/// (`[["a", 1], ["b", 2]]`).  This is useful for maps whose keys cannot be
/// keys in the format, like tuples or structs in JSON.  When deserializing,
/// a sequence of pairs and a map are accepted.  The keys are deserialized
/// with the adapter `KA` and the values with `VA` (both [`Same`] by
/// default).  Keys that are given more than once are handled like in maps
/// (see [`DuplicateKeys`]).
///
/// Supported are `BTreeMap<K, V>` and `HashMap<K, V>`.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser::adapters::MapAsPairs;
/// use deser::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Grid {
///     #[deser(as = MapAsPairs)]
///     cells: BTreeMap<(u32, u32), String>,
/// }
///
/// let grid: Grid = deser_json::from_str(r#"{"cells": [[[0, 1], "x"]]}"#).unwrap();
/// assert_eq!(grid.cells[&(0, 1)], "x");
/// assert_eq!(
///     deser_json::to_string(&grid).unwrap(),
///     r#"{"cells":[[[0,1],"x"]]}"#
/// );
/// ```
///
#[cfg_attr(
    not(feature = "std"),
    doc = "[`HashMap`]: https://doc.rust-lang.org/std/collections/struct.HashMap.html"
)]
pub struct MapAsPairs<KA = Same, VA = Same>(PhantomData<fn() -> (KA, VA)>);

/// A sequence of key and value pairs that is written as a map.
///
/// When serializing, the pairs are written as the entries of a map in
/// their order.  When deserializing, a map and a sequence of pairs are
/// accepted.  All entries are kept in the order of the data, also if a key
/// is given more than once.  The keys are deserialized with the adapter
/// `KA` and the values with `VA` (both [`Same`] by default).
///
/// This keeps the order of the entries and keys that are given more than
/// once.  Flattened into a struct, it receives the entries the struct does
/// not take in order.
///
/// Supported is `Vec<(K, V)>`.
///
/// ```
/// use deser::adapters::PairsAsMap;
/// use deser::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// pub struct Headers {
///     #[deser(as = PairsAsMap)]
///     headers: Vec<(String, String)>,
/// }
///
/// let input = r#"{"headers":{"b":"1","a":"2"}}"#;
/// let headers: Headers = deser_json::from_str(input).unwrap();
/// assert_eq!(headers.headers[0], ("b".into(), "1".into()));
/// assert_eq!(deser_json::to_string(&headers).unwrap(), input);
/// ```
pub struct PairsAsMap<KA = Same, VA = Same>(PhantomData<fn() -> (KA, VA)>);

/// The sink of [`MapAsPairs`].
enum MapAsPairsSink<'a, 'de, M, K, V> {
    Pending(&'a mut Option<M>),
    // the value is a map
    Map(SinkHandle<'a, 'de>),
    // the value is a sequence of pairs
    Pairs(PairsSink<'a, M, K, V>),
}

/// Collects the pairs of a [`MapAsPairs`] into the map.
struct PairsSink<'a, M, K, V> {
    out: &'a mut Option<M>,
    map: M,
    pair: Option<(K, V)>,
    duplicate_keys: DuplicateKeys,
    errors: CollectedErrors,
}

impl<'a, M: MapTarget<K, V>, K, V> PairsSink<'a, M, K, V> {
    /// Adds the previous pair before the next one starts.
    ///
    /// If it's a duplicate that is rejected, the error is collected here as
    /// it's not the error of the next pair.
    fn flush(&mut self, state: &mut State) -> Result<(), Error> {
        if let Some((key, value)) = self.pair.take() {
            let replace = self.duplicate_keys != DuplicateKeys::First;
            if self.map.insert_entry(key, value, replace)
                && let Err(err) = self
                    .duplicate_keys
                    .resolve(|| "duplicate key in map".into())
            {
                return self.errors.collect(err, state);
            }
        }
        Ok(())
    }
}

impl<'a, 'de, M, K, V> MapAsPairsSink<'a, 'de, M, K, V>
where
    M: MapTarget<K, V>,
    K: Send + 'a,
    V: Send + 'a,
{
    /// Returns the sink of the map, creates it if the value is a map.
    fn map_sink<KA, VA>(&mut self, state: &mut State) -> Result<&mut SinkHandle<'a, 'de>, Error>
    where
        KA: Deserialize<'de, K>,
        VA: Deserialize<'de, V>,
    {
        if let MapAsPairsSink::Pending(..) = self
            && let MapAsPairsSink::Pending(out) =
                core::mem::replace(self, MapAsPairsSink::Map(SinkHandle::null()))
        {
            *self = MapAsPairsSink::Map(map_sink::<M, K, V, KA, VA>(MapOut::Slot(out), state));
        }
        match self {
            MapAsPairsSink::Map(sink) => Ok(sink),
            _ => Err(Error::new(
                ErrorKind::InvalidState,
                "the value is not a map",
            )),
        }
    }
}

impl<'a, 'de, M, K, V, KA, VA> Sink<'de> for Adapted<(KA, VA), MapAsPairsSink<'a, 'de, M, K, V>>
where
    M: MapTarget<K, V>,
    K: Send + 'a,
    V: Send + 'a,
    KA: Deserialize<'de, K>,
    VA: Deserialize<'de, V>,
{
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed(M::NAME)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.map_sink::<KA, VA>(state)?.map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        if let MapAsPairsSink::Pending(..) = **self
            && let MapAsPairsSink::Pending(out) =
                core::mem::replace(&mut **self, MapAsPairsSink::Map(SinkHandle::null()))
        {
            let mut map = M::default();
            map.reserve_entries(state.container_shape().cautious_capacity::<(K, V)>());
            **self = MapAsPairsSink::Pairs(PairsSink {
                out,
                map,
                pair: None,
                duplicate_keys: DuplicateKeys::of(state),
                errors: CollectedErrors::new(),
            });
            return Ok(());
        }
        match **self {
            MapAsPairsSink::Map(ref mut sink) => sink.seq(state),
            _ => default_container_error(self, state),
        }
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        match **self {
            MapAsPairsSink::Map(ref mut sink) => sink.next_key(state),
            _ => Ok(SinkHandle::null()),
        }
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        match **self {
            MapAsPairsSink::Map(ref mut sink) => sink.next_value(state),
            MapAsPairsSink::Pairs(ref mut pairs) => {
                pairs.flush(state)?;
                Ok(<(KA, VA) as Deserialize<'de, (K, V)>>::deserialize_into(
                    &mut pairs.pair,
                    state,
                ))
            }
            MapAsPairsSink::Pending(..) => Ok(SinkHandle::null()),
        }
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match **self {
            MapAsPairsSink::Map(ref mut sink) => sink.__private_key_atom(atom, state),
            _ => crate::de::atom_into_handle(self.next_key(state)?, atom, state),
        }
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match **self {
            MapAsPairsSink::Map(ref mut sink) => sink.__private_value_atom(atom, state),
            _ => crate::de::atom_into_handle(self.next_value(state)?, atom, state),
        }
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match **self {
            MapAsPairsSink::Map(ref mut sink) => sink.__private_borrowed_key_atom(atom, state),
            _ => crate::de::borrowed_atom_into_handle(self.next_key(state)?, atom, state),
        }
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match **self {
            MapAsPairsSink::Map(ref mut sink) => sink.__private_borrowed_value_atom(atom, state),
            _ => crate::de::borrowed_atom_into_handle(self.next_value(state)?, atom, state),
        }
    }

    /// Takes all keys when the map is flattened into a struct, like a map.
    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        if let MapAsPairsSink::Pairs(..) = **self {
            return Ok(None);
        }
        self.map_sink::<KA, VA>(state)?.value_for_key(key, state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        match **self {
            MapAsPairsSink::Map(ref mut sink) => sink.recover(err, state),
            MapAsPairsSink::Pairs(ref mut pairs) => {
                pairs.pair = None;
                pairs.errors.collect(err, state)
            }
            MapAsPairsSink::Pending(..) => Err(err),
        }
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        match **self {
            MapAsPairsSink::Map(ref mut sink) => sink.finish(state),
            MapAsPairsSink::Pairs(ref mut pairs) => {
                pairs.flush(state)?;
                pairs.errors.finish()?;
                *pairs.out = Some(take(&mut pairs.map));
                Ok(())
            }
            // a flattened map that received no keys
            MapAsPairsSink::Pending(..) => self.map_sink::<KA, VA>(state)?.finish(state),
        }
    }
}

/// Returns the error for a sequence that is given to a sink that is not
/// pending anymore.
#[cold]
fn default_container_error(sink: &mut dyn Sink<'_>, state: &State) -> Result<(), Error> {
    crate::de::atoms::default_container(sink, "sequence", state)
}

/// Implements `MapAsPairs` for maps.
macro_rules! map_as_pairs_impls {
    ($([$($bound:tt)*] [$($ser_bound:tt)*] $target:ty, $order:ident;)*) => {
        $(
            impl<'de, $($bound)*, KA: Deserialize<'de, K>, VA: Deserialize<'de, V>>
                Deserialize<'de, $target> for MapAsPairs<KA, VA>
            {
                fn deserialize_into<'out>(
                    out: &'out mut Option<$target>,
                    state: &mut State,
                ) -> SinkHandle<'out, 'de> {
                    adapted::arena_sink::<(KA, VA), _>(
                        MapAsPairsSink::<_, K, V>::Pending(out),
                        state,
                    )
                }

                fn expecting() -> Cow<'static, str> {
                    Cow::Borrowed(<$target as MapTarget<K, V>>::NAME)
                }
            }

            impl<$($ser_bound)*, KA: Serialize<K>, VA: Serialize<V>> Serialize<$target>
                for MapAsPairs<KA, VA>
            {
                fn serialize<'a>(
                    value: &'a $target,
                    state: &mut State,
                ) -> Result<Emit<'a>, Error> {
                    Ok(adapted::seq_emitter::<(KA, VA), _>(
                        PairsEmitter {
                            iter: value.iter(),
                            current: None,
                        },
                        state,
                    ))
                }

                fn container_shape(value: &$target) -> ContainerShape {
                    let mut shape = ContainerShape::with_order(Order::$order);
                    shape.set_len(value.len());
                    shape
                }
            }
        )*
    };
}

map_as_pairs_impls! {
    [K: Ord + Send, V: Send] [K: Sync, V: Sync] BTreeMap<K, V>, Sorted;
}

#[cfg(feature = "std")]
map_as_pairs_impls! {
    [K: Hash + Eq + Send, V: Send, H: BuildHasher + Default + Send]
    [K: Sync, V: Sync, H: BuildHasher + Sync]
    HashMap<K, V, H>, Arbitrary;
}

/// Emits the entries of a map as pairs.
struct PairsEmitter<'a, I, K, V> {
    iter: I,
    // the entry that is emitted, the handle of the pair borrows it
    current: Option<(&'a K, &'a V)>,
}

impl<'a, I, K, V, KA, VA> SeqEmitter for Adapted<(KA, VA), PairsEmitter<'a, I, K, V>>
where
    I: Iterator<Item = (&'a K, &'a V)> + Send,
    K: Sync + 'a,
    V: Sync + 'a,
    KA: Serialize<K>,
    VA: Serialize<V>,
{
    fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
        let this = &mut **self;
        this.current = this.iter.next();
        Ok(this
            .current
            .as_ref()
            .map(handle_as::<Pair<KA, VA>, (&'a K, &'a V)>))
    }
}

/// Serializes a key and a value as a pair.
struct Pair<KA, VA>(PhantomData<fn() -> (KA, VA)>);

impl<'r, K, V, KA, VA> Serialize<(&'r K, &'r V)> for Pair<KA, VA>
where
    K: Sync,
    V: Sync,
    KA: Serialize<K>,
    VA: Serialize<V>,
{
    fn serialize<'a>(value: &'a (&'r K, &'r V), state: &mut State) -> Result<Emit<'a>, Error> {
        let seq = Adapted::<Self, _>::indexed(value);
        Ok(Emit::seq(IndexedSeqEmitter::new(seq), state))
    }

    fn container_shape(_value: &(&'r K, &'r V)) -> ContainerShape {
        ContainerShape::with_len(2)
    }
}

impl<'r, K, V, KA, VA> IndexedSeq for Adapted<Pair<KA, VA>, (&'r K, &'r V)>
where
    K: Sync,
    V: Sync,
    KA: Serialize<K>,
    VA: Serialize<V>,
{
    fn element(
        &self,
        index: usize,
        _state: &mut State,
    ) -> Result<Option<SerializeHandle<'_>>, Error> {
        let (key, value) = *self.get();
        Ok(match index {
            0 => Some(handle_as::<KA, K>(key)),
            1 => Some(handle_as::<VA, V>(value)),
            _ => None,
        })
    }
}

/// The sink of [`PairsAsMap`].
enum PairsAsMapSink<'a, 'de, K, V> {
    Pending(&'a mut Option<Vec<(K, V)>>),
    // the value is a sequence of pairs
    Seq(SinkHandle<'a, 'de>),
    // the value is a map
    Map(EntriesSink<'a, K, V>),
}

/// Collects the entries of a map for a [`PairsAsMap`].
struct EntriesSink<'a, K, V> {
    out: &'a mut Option<Vec<(K, V)>>,
    entries: Vec<(K, V)>,
    key: Option<K>,
    value: Option<V>,
    errors: CollectedErrors,
}

impl<'a, K, V> EntriesSink<'a, K, V> {
    /// Adds the previous entry before the next one starts.
    fn flush(&mut self) {
        if let (Some(key), Some(value)) = (self.key.take(), self.value.take()) {
            self.entries.push((key, value));
        }
    }
}

impl<'a, 'de, K: Send, V: Send> PairsAsMapSink<'a, 'de, K, V> {
    /// Returns the sink of the sequence, creates it if the value is a
    /// sequence.
    fn seq_sink<KA, VA>(&mut self, state: &mut State) -> Option<&mut SinkHandle<'a, 'de>>
    where
        KA: Deserialize<'de, K>,
        VA: Deserialize<'de, V>,
    {
        if let PairsAsMapSink::Pending(..) = self
            && let PairsAsMapSink::Pending(out) =
                core::mem::replace(self, PairsAsMapSink::Seq(SinkHandle::null()))
        {
            *self = PairsAsMapSink::Seq(
                <Vec<(KA, VA)> as Deserialize<'de, Vec<(K, V)>>>::deserialize_into(out, state),
            );
        }
        match self {
            PairsAsMapSink::Seq(sink) => Some(sink),
            _ => None,
        }
    }

    /// Returns the sink of the entries, creates it if the value is a map.
    fn entries(&mut self) -> Option<&mut EntriesSink<'a, K, V>> {
        if let PairsAsMapSink::Pending(..) = self
            && let PairsAsMapSink::Pending(out) =
                core::mem::replace(self, PairsAsMapSink::Seq(SinkHandle::null()))
        {
            *self = PairsAsMapSink::Map(EntriesSink {
                out,
                entries: Vec::new(),
                key: None,
                value: None,
                errors: CollectedErrors::new(),
            });
        }
        match self {
            PairsAsMapSink::Map(entries) => Some(entries),
            _ => None,
        }
    }
}

impl<'a, 'de, K, V, KA, VA> Sink<'de> for Adapted<(KA, VA), PairsAsMapSink<'a, 'de, K, V>>
where
    K: Send,
    V: Send,
    KA: Deserialize<'de, K>,
    VA: Deserialize<'de, V>,
{
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("map")
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        let capacity = state.container_shape().cautious_capacity::<(K, V)>();
        if let Some(entries) = self.entries() {
            entries.entries.reserve(capacity);
            return Ok(());
        }
        crate::de::atoms::default_container(self, "map", state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        match self.seq_sink::<KA, VA>(state) {
            Some(sink) => sink.seq(state),
            None => default_container_error(self, state),
        }
    }

    fn __private_seq(&mut self, state: &mut State) -> Result<bool, Error> {
        // pairs of atoms are built inline by the sink of the sequence
        match self.seq_sink::<KA, VA>(state) {
            Some(sink) => sink.__private_seq(state),
            None => default_container_error(self, state).map(|()| false),
        }
    }

    fn __private_inline_atom(
        &mut self,
        index: usize,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        match **self {
            PairsAsMapSink::Seq(ref mut sink) => sink.__private_inline_atom(index, atom, state),
            _ => unreachable!(),
        }
    }

    fn __private_inline_event(
        &mut self,
        event: crate::de::InlineEvent,
        state: &mut State,
    ) -> Result<(), Error> {
        match **self {
            PairsAsMapSink::Seq(ref mut sink) => sink.__private_inline_event(event, state),
            _ => unreachable!(),
        }
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        match **self {
            PairsAsMapSink::Map(ref mut entries) => {
                entries.flush();
                Ok(KA::deserialize_into(&mut entries.key, state))
            }
            PairsAsMapSink::Seq(ref mut sink) => sink.next_key(state),
            PairsAsMapSink::Pending(..) => Ok(SinkHandle::null()),
        }
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        match **self {
            PairsAsMapSink::Map(ref mut entries) => {
                Ok(VA::deserialize_into(&mut entries.value, state))
            }
            PairsAsMapSink::Seq(ref mut sink) => sink.next_value(state),
            PairsAsMapSink::Pending(..) => Ok(SinkHandle::null()),
        }
    }

    fn __private_key_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match **self {
            PairsAsMapSink::Map(ref mut entries) => {
                entries.flush();
                KA::__private_atom_into(&mut entries.key, atom, state)
            }
            _ => crate::de::atom_into_handle(self.next_key(state)?, atom, state),
        }
    }

    fn __private_value_atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match **self {
            PairsAsMapSink::Map(ref mut entries) => {
                VA::__private_atom_into(&mut entries.value, atom, state)
            }
            PairsAsMapSink::Seq(ref mut sink) => sink.__private_value_atom(atom, state),
            PairsAsMapSink::Pending(..) => Ok(()),
        }
    }

    fn __private_borrowed_key_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match **self {
            PairsAsMapSink::Map(ref mut entries) => {
                entries.flush();
                KA::__private_borrowed_atom_into(&mut entries.key, atom, state)
            }
            _ => crate::de::borrowed_atom_into_handle(self.next_key(state)?, atom, state),
        }
    }

    fn __private_borrowed_value_atom(
        &mut self,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        match **self {
            PairsAsMapSink::Map(ref mut entries) => {
                VA::__private_borrowed_atom_into(&mut entries.value, atom, state)
            }
            PairsAsMapSink::Seq(ref mut sink) => sink.__private_borrowed_value_atom(atom, state),
            PairsAsMapSink::Pending(..) => Ok(()),
        }
    }

    /// Takes all keys when flattened into a struct, the key is parsed like
    /// the keys of formats that only have string keys.
    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        let Some(entries) = self.entries() else {
            return Ok(None);
        };
        entries.flush();
        KA::__private_atom_into(&mut entries.key, Atom::Lexical(Text::borrowed(key)), state)?;
        Ok(Some(VA::deserialize_into(&mut entries.value, state)))
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        match **self {
            PairsAsMapSink::Map(ref mut entries) => {
                entries.key = None;
                entries.value = None;
                entries.errors.collect(err, state)
            }
            PairsAsMapSink::Seq(ref mut sink) => sink.recover(err, state),
            PairsAsMapSink::Pending(..) => Err(err),
        }
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        // a flattened value that received no keys has no entries
        let _ = self.entries();
        match **self {
            PairsAsMapSink::Map(ref mut entries) => {
                entries.flush();
                entries.errors.finish()?;
                *entries.out = Some(take(&mut entries.entries));
                Ok(())
            }
            PairsAsMapSink::Seq(ref mut sink) => sink.finish(state),
            PairsAsMapSink::Pending(..) => Ok(()),
        }
    }
}

impl<'de, K, V, KA, VA> Deserialize<'de, Vec<(K, V)>> for PairsAsMap<KA, VA>
where
    K: Send,
    V: Send,
    KA: Deserialize<'de, K>,
    VA: Deserialize<'de, V>,
{
    fn deserialize_into<'out>(
        out: &'out mut Option<Vec<(K, V)>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        adapted::arena_sink::<(KA, VA), _>(PairsAsMapSink::Pending(out), state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("map")
    }
}

impl<K, V, KA, VA> Serialize<Vec<(K, V)>> for PairsAsMap<KA, VA>
where
    K: Sync,
    V: Sync,
    KA: Serialize<K>,
    VA: Serialize<V>,
{
    fn serialize<'a>(value: &'a Vec<(K, V)>, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(MapIterEmitter::emit::<K, KA, VA>(
            value.iter().map(split_pair as fn(&(K, V)) -> (&K, &V)),
            state,
        ))
    }

    fn container_shape(value: &Vec<(K, V)>) -> ContainerShape {
        let mut shape = ContainerShape::with_order(Order::Significant);
        shape.set_len(value.len());
        shape
    }
}

/// Splits a reference to a pair into references to its parts.
fn split_pair<K, V>(pair: &(K, V)) -> (&K, &V) {
    (&pair.0, &pair.1)
}
