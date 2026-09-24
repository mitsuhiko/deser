//! Tests that exercise the unsafe code paths in deser.  These are primarily
//! useful when run under miri.
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::panic::{catch_unwind, AssertUnwindSafe};

use deser::de::{DeserializeDriver, DeserializerState, OwnedSink, Sink, SinkHandle};
use deser::ser::{Chunk, SerializeDriver, SerializeHandle, SerializerState, StructEmitter};
use deser::{Atom, Deserialize, Error, Event, Serialize};

fn depth() -> usize {
    // deeper than the preallocated stacks of the drivers
    if cfg!(miri) {
        200
    } else {
        1000
    }
}

/// Emits the given events and drops the driver afterwards, no matter if the
/// events form a complete value.
fn emit_partial<T: Deserialize>(events: &[Event]) -> Option<T> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in events {
            if driver.emit(event.clone()).is_err() {
                break;
            }
        }
    }
    out
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Default)]
struct Inner {
    name: String,
    tags: Vec<String>,
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct Outer {
    inner: Option<Inner>,
    boxed: Box<Inner>,
    array: [String; 3],
    map: HashMap<String, Vec<String>>,
    #[deser(flatten)]
    flat: Inner,
}

fn outer_events() -> Vec<Event<'static>> {
    let mut driver_value = HashMap::new();
    driver_value.insert("k".to_string(), vec!["v".to_string()]);
    let value = Outer {
        inner: Some(Inner {
            name: "a".into(),
            tags: vec!["x".into(), "y".into()],
        }),
        boxed: Box::new(Inner {
            name: "b".into(),
            tags: vec!["z".into()],
        }),
        array: ["1".into(), "2".into(), "3".into()],
        map: driver_value,
        flat: Inner {
            name: "c".into(),
            tags: vec![],
        },
    };
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(&value);
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(event.to_static());
    }
    events
}

#[test]
fn test_drop_driver_at_every_point() {
    let events = outer_events();
    let full: Outer = emit_partial(&events).unwrap();
    assert_eq!(full.array, ["1", "2", "3"]);
    for cut in 0..events.len() {
        // every prefix leaves sinks half way through, dropping the driver
        // must clean up properly.
        assert!(emit_partial::<Outer>(&events[..cut]).is_none());
    }
}

#[test]
fn test_errors_at_every_point() {
    let events = outer_events();
    for idx in 0..events.len() {
        // replace one event with something unexpected
        let mut events = events.clone();
        events[idx] = match events[idx] {
            Event::Atom(Atom::Str(_)) => Event::Atom(Atom::Bool(true)),
            _ => Event::Atom(Atom::Str("unexpected".into())),
        };
        let _ = catch_unwind(AssertUnwindSafe(|| emit_partial::<Outer>(&events)));
    }
}

#[test]
fn test_arrays() {
    let array: [String; 2] =
        emit_partial(&[Event::SeqStart, "a".into(), "b".into(), Event::SeqEnd]).unwrap();
    assert_eq!(array, ["a", "b"]);

    // not enough elements
    assert!(emit_partial::<[String; 3]>(
        &[Event::SeqStart, "a".into(), "b".into(), Event::SeqEnd,]
    )
    .is_none());

    // too many elements
    assert!(emit_partial::<[String; 1]>(
        &[Event::SeqStart, "a".into(), "b".into(), Event::SeqEnd,]
    )
    .is_none());

    // bytes
    let bytes: [u8; 3] = emit_partial(&[Event::Atom(Atom::Bytes(b"abc"[..].into()))]).unwrap();
    assert_eq!(&bytes, b"abc");
    assert!(emit_partial::<[u8; 2]>(&[Event::Atom(Atom::Bytes(b"abc"[..].into()))]).is_none());
    assert!(emit_partial::<[u16; 3]>(&[Event::Atom(Atom::Bytes(b"abc"[..].into()))]).is_none());
    let bytes: Vec<u8> = emit_partial(&[Event::Atom(Atom::Bytes(b"abc"[..].into()))]).unwrap();
    assert_eq!(bytes, b"abc");
    assert!(emit_partial::<Vec<u16>>(&[Event::Atom(Atom::Bytes(b"abc"[..].into()))]).is_none());

    // nested arrays of arrays
    let nested: [[String; 2]; 2] = emit_partial(&[
        Event::SeqStart,
        Event::SeqStart,
        "a".into(),
        "b".into(),
        Event::SeqEnd,
        Event::SeqStart,
        "c".into(),
        "d".into(),
        Event::SeqEnd,
        Event::SeqEnd,
    ])
    .unwrap();
    assert_eq!(nested, [["a", "b"], ["c", "d"]]);
}

#[test]
fn test_array_sink_misuse() {
    // sinks are public API and can be called in any order
    let mut driver_out = None::<()>;
    let driver = DeserializeDriver::new(&mut driver_out);
    let state = driver.state();

    let mut out = None::<[String; 2]>;
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let mut sink = <[String; 2]>::deserialize_into(&mut out);
        sink.seq(state).unwrap();
        for idx in 0..4 {
            match sink.next_value(state) {
                Ok(mut value) => value
                    .atom(Atom::Str(idx.to_string().into()), state)
                    .unwrap(),
                Err(_) => break,
            }
        }
        sink.finish(state).unwrap();
        // finishing again and pushing more values must not cause problems
        let _ = sink.finish(state);
        if let Ok(mut value) = sink.next_value(state) {
            value.atom(Atom::Str("x".into()), state).unwrap();
        }
        let _ = sink.finish(state);
    }));
    assert_eq!(out, Some(["0".to_string(), "1".to_string()]));
}

/// A deserializer for a type whose byte specialization claims to be bytes.
/// This used to be undefined behavior.
#[derive(Debug)]
struct LyingBytes(#[allow(dead_code)] String);

impl Deserialize for LyingBytes {
    fn deserialize_into(_out: &mut Option<Self>) -> SinkHandle<'_> {
        SinkHandle::null()
    }

    fn __private_is_bytes() -> bool {
        true
    }
}

#[test]
fn test_lying_bytes() {
    let bytes = Event::Atom(Atom::Bytes(vec![1u8; 64].into()));
    assert!(emit_partial::<Vec<LyingBytes>>(std::slice::from_ref(&bytes)).is_none());
    assert!(emit_partial::<[LyingBytes; 64]>(&[bytes]).is_none());
}

struct Parent {
    slot: Option<u64>,
    finished_with: Option<Option<u64>>,
}

struct CommitOnDrop<'a> {
    slot: &'a mut Option<u64>,
    value: Option<u64>,
}

impl<'a> Drop for CommitOnDrop<'a> {
    fn drop(&mut self) {
        *self.slot = self.value;
    }
}

impl<'a> Sink for CommitOnDrop<'a> {
    fn atom(&mut self, atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
        if let Atom::U64(v) = atom {
            self.value = Some(v);
        }
        Ok(())
    }
}

impl Sink for Parent {
    fn seq(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        Ok(())
    }

    fn next_value(&mut self, _state: &DeserializerState) -> Result<SinkHandle<'_>, Error> {
        Ok(SinkHandle::boxed(CommitOnDrop {
            slot: &mut self.slot,
            value: None,
        }))
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.finished_with = Some(self.slot);
        Ok(())
    }
}

#[test]
fn test_child_sinks_dropped_before_parent_is_used() {
    let mut sink = Parent {
        slot: None,
        finished_with: None,
    };
    {
        let mut driver = DeserializeDriver::from_sink(SinkHandle::to(&mut sink));
        driver.emit(Event::SeqStart).unwrap();
        driver.emit(1u64).unwrap();
        driver.emit(2u64).unwrap();
        driver.emit(Event::SeqEnd).unwrap();
    }
    assert_eq!(sink.finished_with, Some(Some(2)));
}

#[test]
fn test_owned_sink_after_take() {
    let mut driver_out = None::<()>;
    let driver = DeserializeDriver::new(&mut driver_out);
    let state = driver.state();

    let mut owned = OwnedSink::<Vec<String>>::deserialize();
    owned.borrow_mut().seq(state).unwrap();
    owned
        .borrow_mut()
        .next_value(state)
        .unwrap()
        .atom(Atom::Str("a".into()), state)
        .unwrap();
    owned.borrow_mut().finish(state).unwrap();
    assert_eq!(owned.take(), Some(vec!["a".to_string()]));

    // after take the sink ignores everything
    owned.borrow_mut().seq(state).unwrap();
    let _ = owned.borrow_mut().next_value(state);
    owned.borrow_mut().finish(state).unwrap();
    assert_eq!(owned.take(), None);
    assert_eq!(owned.borrow().expecting(), "compatible type");
}

#[test]
fn test_owned_sink_dropped_half_way() {
    let mut driver_out = None::<()>;
    let driver = DeserializeDriver::new(&mut driver_out);
    let state = driver.state();

    let mut owned = OwnedSink::<BTreeMap<String, Inner>>::deserialize();
    owned.borrow_mut().map(state).unwrap();
    owned
        .borrow_mut()
        .next_key(state)
        .unwrap()
        .atom(Atom::Str("a".into()), state)
        .unwrap();
    let mut value = owned.borrow_mut().next_value(state).unwrap();
    value.map(state).unwrap();
    drop(value);
    drop(owned);
}

#[derive(Deserialize, Serialize, Debug)]
struct Node {
    name: String,
    children: Vec<Node>,
    boxed: Option<Box<Node>>,
}

fn nested_node(depth: usize) -> Node {
    let mut node = Node {
        name: "leaf".into(),
        children: vec![],
        boxed: None,
    };
    for idx in 0..depth {
        node = if idx % 2 == 0 {
            Node {
                name: format!("vec-{}", idx),
                children: vec![node],
                boxed: None,
            }
        } else {
            Node {
                name: format!("box-{}", idx),
                children: vec![],
                boxed: Some(Box::new(node)),
            }
        };
    }
    node
}

fn drop_node(node: Node) {
    let mut stack = vec![node];
    while let Some(mut node) = stack.pop() {
        stack.append(&mut node.children);
        if let Some(boxed) = node.boxed.take() {
            stack.push(*boxed);
        }
    }
}

#[test]
fn test_deep_roundtrip_and_partial_drops() {
    let node = nested_node(depth());
    let mut events = Vec::new();
    {
        let mut driver = SerializeDriver::new(&node);
        while let Some((event, _, _)) = driver.next().unwrap() {
            events.push(event.to_static());
        }
    }

    // dropping the serializer half way
    {
        let mut driver = SerializeDriver::new(&node);
        for _ in 0..events.len() / 2 {
            driver.next().unwrap();
        }
    }

    let rv: Node = emit_partial(&events).unwrap();
    drop_node(rv);

    // dropping the deserializer half way
    assert!(emit_partial::<Node>(&events[..events.len() / 2]).is_none());
    drop_node(node);
}

/// An emitter that hands out keys borrowed from a buffer it owns.
struct BufferEmitter<'a> {
    depth: usize,
    index: usize,
    buffer: String,
    child: &'a Nested,
}

struct Nested(usize);

impl Serialize for Nested {
    fn serialize(&self, _state: &SerializerState) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Struct(Box::new(BufferEmitter {
            depth: self.0,
            index: 0,
            buffer: String::new(),
            child: self,
        })))
    }
}

impl<'a> StructEmitter for BufferEmitter<'a> {
    fn next(
        &mut self,
        _state: &SerializerState,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        let index = self.index;
        self.index += 1;
        self.buffer = format!("key-{}-{}", self.depth, index);
        Ok(match index {
            0 => Some((
                Cow::Borrowed(self.buffer.as_str()),
                SerializeHandle::boxed(self.buffer.clone()),
            )),
            1 if self.depth > 0 => Some((
                Cow::Borrowed(self.buffer.as_str()),
                SerializeHandle::boxed(Nested(self.child.0 - 1)),
            )),
            _ => None,
        })
    }
}

#[test]
fn test_borrowed_keys_across_reallocation() {
    let value = Nested(depth());
    let mut driver = SerializeDriver::new(&value);
    let mut keys = 0;
    while let Some((event, _, _)) = driver.next().unwrap() {
        if let Event::Atom(Atom::Str(s)) = event {
            if s.starts_with("key-") {
                keys += 1;
            }
        }
    }
    // two keys per level (one on the last) plus the value of the first key
    assert_eq!(keys, depth() * 3 + 2);
}

struct Panicking;

impl Serialize for Panicking {
    fn serialize(&self, _state: &SerializerState) -> Result<Chunk<'_>, Error> {
        panic!("serialize panicked");
    }
}

struct PanickingSink;

impl Sink for PanickingSink {
    fn atom(&mut self, _atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
        panic!("sink panicked");
    }
}

#[test]
fn test_panics() {
    let value = (vec!["a".to_string()], vec![vec![Panicking]]);
    let rv = catch_unwind(AssertUnwindSafe(|| {
        let mut driver = SerializeDriver::new(&value);
        while driver.next().unwrap().is_some() {}
    }));
    assert!(rv.is_err());

    let mut out = None::<(Vec<String>, Vec<Vec<String>>)>;
    let rv = catch_unwind(AssertUnwindSafe(|| {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit(Event::SeqStart).unwrap();
        driver.emit(Event::SeqStart).unwrap();
        driver.emit("a").unwrap();
        driver.emit(Event::SeqEnd).unwrap();
        driver.emit(Event::SeqStart).unwrap();
        driver.emit(Event::SeqStart).unwrap();
        // emitting an end for the wrong container panics
        driver.emit(Event::MapEnd).unwrap();
    }));
    assert!(rv.is_err());

    let mut sink = PanickingSink;
    let rv = catch_unwind(AssertUnwindSafe(|| {
        let mut driver = DeserializeDriver::from_sink(SinkHandle::to(&mut sink));
        driver.emit(1u64).unwrap();
    }));
    assert!(rv.is_err());
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[deser(tag = "type")]
enum Tagged {
    A {
        inner: Inner,
        list: Vec<Option<Box<Inner>>>,
    },
    B,
}

#[test]
fn test_tagged_drop_and_errors_at_every_point() {
    let value = Tagged::A {
        inner: Inner {
            name: "x".into(),
            tags: vec!["a".into(), "b".into()],
        },
        list: vec![
            None,
            Some(Box::new(Inner {
                name: "y".into(),
                tags: vec![],
            })),
        ],
    };
    let mut events = Vec::new();
    {
        let mut driver = SerializeDriver::new(&value);
        while let Some((event, _, _)) = driver.next().unwrap() {
            events.push(event.to_static());
        }
    }
    // move the tag to the end so that everything is buffered
    let tag = events.drain(1..3).collect::<Vec<_>>();
    let end = events.pop().unwrap();
    events.extend(tag);
    events.push(end);

    assert_eq!(emit_partial::<Tagged>(&events).unwrap(), value);
    for cut in 0..events.len() {
        assert!(emit_partial::<Tagged>(&events[..cut]).is_none());
    }
    for idx in 0..events.len() {
        let mut events = events.clone();
        events[idx] = match events[idx] {
            Event::Atom(Atom::Str(_)) => Event::Atom(Atom::Bool(true)),
            _ => Event::Atom(Atom::Str("unexpected".into())),
        };
        let _ = catch_unwind(AssertUnwindSafe(|| emit_partial::<Tagged>(&events)));
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
enum AllExternal {
    Tuple(String, Vec<Inner>),
    Struct { inner: Box<Inner> },
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[deser(tag = "t", content = "c")]
enum AllAdjacent {
    External(AllExternal),
    Untagged(Vec<AllUntagged>),
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[deser(untagged)]
enum AllUntagged {
    Number(u32),
    Inner(Inner),
    Tagged(Tagged),
}

#[test]
fn test_enum_representations_drop_and_errors_at_every_point() {
    let value = AllAdjacent::Untagged(vec![
        AllUntagged::Number(1),
        AllUntagged::Inner(Inner {
            name: "x".into(),
            tags: vec!["a".into()],
        }),
        AllUntagged::Tagged(Tagged::B),
    ]);
    let other = AllAdjacent::External(AllExternal::Tuple(
        "x".into(),
        vec![Inner {
            name: "y".into(),
            tags: vec![],
        }],
    ));

    for value in [value, other] {
        let mut events = Vec::new();
        {
            let mut driver = SerializeDriver::new(&value);
            while let Some((event, _, _)) = driver.next().unwrap() {
                events.push(event.to_static());
            }
        }
        // move the tag to the end so that the content is buffered
        let tag = events.drain(1..3).collect::<Vec<_>>();
        let end = events.pop().unwrap();
        events.extend(tag);
        events.push(end);

        assert_eq!(emit_partial::<AllAdjacent>(&events).unwrap(), value);
        for cut in 0..events.len() {
            assert!(emit_partial::<AllAdjacent>(&events[..cut]).is_none());
        }
        for idx in 0..events.len() {
            let mut events = events.clone();
            events[idx] = match events[idx] {
                Event::Atom(Atom::Str(_)) => Event::Atom(Atom::Bool(true)),
                _ => Event::Atom(Atom::Str("unexpected".into())),
            };
            let _ = catch_unwind(AssertUnwindSafe(|| emit_partial::<AllAdjacent>(&events)));
        }
    }
}
