//! Tests that exercise the unsafe code paths in deser.  These are primarily
//! useful when run under miri.
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};

use deser::State;
use deser::de::{DeserializeDriver, DeserializeOwned, OwnedSink, Sink, SinkHandle};
use deser::ser::{Emit, SerializeDriver, SerializeHandle, StructEmitter};
use deser::{Atom, Deserialize, Error, Event, Serialize};

fn depth() -> usize {
    // deeper than the preallocated stacks of the drivers (128)
    if cfg!(miri) { 150 } else { 1000 }
}

/// The distance between the points where values are cut, split or aborted
/// in tests that check many of them.
///
/// Miri is slow, it checks every 11th point.
fn step() -> usize {
    if cfg!(miri) { 11 } else { 1 }
}

/// Returns the points where the events of a value are cut in tests that
/// check what happens at every point.
///
/// Miri is slow, it checks every third point.
fn every_point(len: usize) -> impl Iterator<Item = usize> {
    (0..len).step_by(if cfg!(miri) { 3 } else { 1 })
}

/// Emits the given events and drops the driver afterwards, no matter if the
/// events form a complete value.
fn emit_partial<T: DeserializeOwned>(events: &[Event]) -> Option<T> {
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
    for cut in every_point(events.len()) {
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
        emit_partial(&[Event::seq_start(), "a".into(), "b".into(), Event::SeqEnd]).unwrap();
    assert_eq!(array, ["a", "b"]);

    // not enough elements
    assert!(
        emit_partial::<[String; 3]>(&[Event::seq_start(), "a".into(), "b".into(), Event::SeqEnd,])
            .is_none()
    );

    // too many elements
    assert!(
        emit_partial::<[String; 1]>(&[Event::seq_start(), "a".into(), "b".into(), Event::SeqEnd,])
            .is_none()
    );

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
        Event::seq_start(),
        Event::seq_start(),
        "a".into(),
        "b".into(),
        Event::SeqEnd,
        Event::seq_start(),
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
    let mut driver = DeserializeDriver::new(&mut driver_out);
    let state = driver.state_mut();

    let mut out = None::<[String; 2]>;
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let mut sink = <[String; 2]>::deserialize_into(&mut out, state);
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

impl<'de> Deserialize<'de> for LyingBytes {
    fn deserialize_into<'out>(
        _out: &'out mut Option<Self>,
        _state: &mut State,
    ) -> SinkHandle<'out, 'de> {
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

impl<'a, 'de> Sink<'de> for CommitOnDrop<'a> {
    fn atom(&mut self, atom: Atom, _state: &mut State) -> Result<(), Error> {
        if let Atom::U64(v) = atom {
            self.value = Some(v);
        }
        Ok(())
    }
}

impl<'de> Sink<'de> for Parent {
    fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(SinkHandle::arena(
            CommitOnDrop {
                slot: &mut self.slot,
                value: None,
            },
            state,
        ))
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
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
        let mut driver = DeserializeDriver::from_fn(|_| SinkHandle::to(&mut sink));
        driver.emit(Event::seq_start()).unwrap();
        driver.emit(1u64).unwrap();
        driver.emit(2u64).unwrap();
        driver.emit(Event::SeqEnd).unwrap();
    }
    assert_eq!(sink.finished_with, Some(Some(2)));
}

#[test]
fn test_owned_sink_after_take() {
    let mut driver_out = None::<()>;
    let mut driver = DeserializeDriver::new(&mut driver_out);
    let state = driver.state_mut();

    let mut owned = OwnedSink::<Vec<String>>::deserialize(state);
    owned.get_mut().seq(state).unwrap();
    owned
        .get_mut()
        .next_value(state)
        .unwrap()
        .atom(Atom::Str("a".into()), state)
        .unwrap();
    owned.get_mut().finish(state).unwrap();
    assert_eq!(owned.take(), Some(vec!["a".to_string()]));

    // after take the sink ignores everything
    owned.get_mut().seq(state).unwrap();
    let _ = owned.get_mut().next_value(state);
    owned.get_mut().finish(state).unwrap();
    assert_eq!(owned.take(), None);
    assert_eq!(owned.get().expecting(), "compatible type");
}

#[test]
fn test_owned_sink_dropped_half_way() {
    let mut driver_out = None::<()>;
    let mut driver = DeserializeDriver::new(&mut driver_out);
    let state = driver.state_mut();

    let mut owned = OwnedSink::<BTreeMap<String, Inner>>::deserialize(state);
    owned.get_mut().map(state).unwrap();
    owned
        .get_mut()
        .next_key(state)
        .unwrap()
        .atom(Atom::Str("a".into()), state)
        .unwrap();
    let mut value = owned.get_mut().next_value(state).unwrap();
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
    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::structure(
            BufferEmitter {
                depth: value.0,
                index: 0,
                buffer: String::new(),
                child: value,
            },
            state,
        ))
    }
}

impl<'a> StructEmitter for BufferEmitter<'a> {
    fn next(
        &mut self,
        state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        let index = self.index;
        self.index += 1;
        self.buffer = format!("key-{}-{}", self.depth, index);
        Ok(match index {
            0 => Some((
                Cow::Borrowed(self.buffer.as_str()),
                SerializeHandle::arena(self.buffer.clone(), state),
            )),
            1 if self.depth > 0 => Some((
                Cow::Borrowed(self.buffer.as_str()),
                SerializeHandle::arena(Nested(self.child.0 - 1), state),
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
        if let Event::Atom(Atom::Str(s)) = event
            && s.starts_with("key-")
        {
            keys += 1;
        }
    }
    // two keys per level (one on the last) plus the value of the first key
    assert_eq!(keys, depth() * 3 + 2);
}

struct Panicking;

impl Serialize for Panicking {
    fn serialize<'a>(_value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        panic!("serialize panicked");
    }
}

struct PanickingSink;

impl<'de> Sink<'de> for PanickingSink {
    fn atom(&mut self, _atom: Atom, _state: &mut State) -> Result<(), Error> {
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
        driver.emit(Event::seq_start()).unwrap();
        driver.emit(Event::seq_start()).unwrap();
        driver.emit("a").unwrap();
        driver.emit(Event::SeqEnd).unwrap();
        driver.emit(Event::seq_start()).unwrap();
        driver.emit(Event::seq_start()).unwrap();
        // emitting an end for the wrong container panics
        driver.emit(Event::MapEnd).unwrap();
    }));
    assert!(rv.is_err());

    let mut sink = PanickingSink;
    let rv = catch_unwind(AssertUnwindSafe(|| {
        let mut driver = DeserializeDriver::from_fn(|_| SinkHandle::to(&mut sink));
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
    for cut in every_point(events.len()) {
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
        for cut in every_point(events.len()) {
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

/// Collects the events of a serializable with `drive`, optionally starting
/// with `next` for the first `skip` events and aborting after `abort` events.
fn drive_events<T: Serialize + ?Sized>(
    value: &T,
    skip: usize,
    abort: Option<usize>,
) -> Result<Vec<Event<'static>>, Error> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(&value);
    for _ in 0..skip {
        match driver.next()? {
            Some((event, _, _)) => events.push(event.to_static()),
            None => return Ok(events),
        }
    }
    driver.drive(|event, _| {
        if Some(events.len()) == abort {
            return Err(Error::new(deser::ErrorKind::Custom, "aborted"));
        }
        events.push(event.to_static());
        Ok(())
    })?;
    Ok(events)
}

#[test]
fn test_drive() {
    let mut value = HashMap::new();
    value.insert("k".to_string(), vec!["v".to_string()]);
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
        map: value,
        flat: Inner {
            name: "c".into(),
            tags: vec![],
        },
    };
    let tagged = Tagged::A {
        inner: Inner {
            name: "x".into(),
            tags: vec!["a".into()],
        },
        list: vec![None, Some(Box::new(Inner::default()))],
    };
    let nested = Nested(if cfg!(miri) { 20 } else { 100 });
    let values: [deser::ser::SerializeRef<'_>; 3] = [
        deser::ser::SerializeRef::new(&value),
        deser::ser::SerializeRef::new(&tagged),
        deser::ser::SerializeRef::new(&nested),
    ];

    for value in values {
        let mut expected = Vec::new();
        {
            let mut driver = SerializeDriver::new(&value);
            while let Some((event, _, _)) = driver.next().unwrap() {
                expected.push(event.to_static());
            }
        }
        let step = step();
        for skip in (0..=expected.len()).step_by(step) {
            // drive produces the same events, even after next was used
            assert_eq!(drive_events(&value, skip, None).unwrap(), expected);
        }
        for abort in (0..expected.len()).step_by(step) {
            // errors from the callback abort and leave the driver droppable
            assert!(drive_events(&value, 0, Some(abort)).is_err());
        }
    }

    // deep nesting beyond the preallocated stack
    let node = nested_node(depth());
    let events = drive_events(&node, 3, None).unwrap();
    let rv: Node = emit_partial(&events).unwrap();
    drop_node(rv);
    drop_node(node);
}

#[derive(Debug, Clone, PartialEq)]
struct Converted(String, Vec<String>);

impl From<Inner> for Converted {
    fn from(value: Inner) -> Converted {
        Converted(value.name, value.tags)
    }
}

impl From<Converted> for Inner {
    fn from(value: Converted) -> Inner {
        Inner {
            name: value.0,
            tags: value.1,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
#[deser(tag = "type")]
enum ForwardingTagged {
    A(#[deser(as = deser::adapters::FromInto<Inner>)] Converted),
    #[deser(other)]
    Other(#[deser(tag)] String, deser::de::Recording),
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct WithAdapters {
    #[deser(as = Vec<deser::adapters::FromInto<Inner>>)]
    converted: Vec<Converted>,
    #[deser(as = deser::adapters::VecSkipError)]
    skipping: Vec<Inner>,
    #[deser(as = deser::adapters::DefaultOnError)]
    lenient: Inner,
    tagged: Vec<ForwardingTagged>,
}

#[test]
fn test_adapters_and_forwarding() {
    let value = WithAdapters {
        converted: vec![
            Converted("a".into(), vec!["x".into()]),
            Converted("b".into(), vec![]),
        ],
        skipping: vec![Inner {
            name: "c".into(),
            tags: vec!["y".into()],
        }],
        lenient: Inner {
            name: "d".into(),
            tags: vec![],
        },
        tagged: vec![
            ForwardingTagged::A(Converted("e".into(), vec!["z".into()])),
            ForwardingTagged::Other("f".into(), {
                let mut out = None;
                {
                    let mut driver = DeserializeDriver::new(&mut out);
                    for event in [Event::map_start(), "k".into(), "v".into(), Event::MapEnd] {
                        driver.emit(event).unwrap();
                    }
                }
                out.unwrap()
            }),
        ],
    };

    let mut expected = Vec::new();
    {
        let mut driver = SerializeDriver::new(&value);
        while let Some((event, _, _)) = driver.next().unwrap() {
            expected.push(event.to_static());
        }
    }
    let step = step();
    for skip in (0..=expected.len()).step_by(step) {
        assert_eq!(drive_events(&value, skip, None).unwrap(), expected);
    }
    for abort in (0..expected.len()).step_by(step) {
        assert!(drive_events(&value, 0, Some(abort)).is_err());
    }

    assert_eq!(emit_partial::<WithAdapters>(&expected).unwrap(), value);
    for cut in (0..expected.len()).step_by(step) {
        assert!(emit_partial::<WithAdapters>(&expected[..cut]).is_none());
    }
    for idx in (0..expected.len()).step_by(step) {
        let mut events = expected.clone();
        events[idx] = match events[idx] {
            Event::Atom(Atom::Str(_)) => Event::Atom(Atom::Bool(true)),
            _ => Event::Atom(Atom::Str("unexpected".into())),
        };
        let _ = catch_unwind(AssertUnwindSafe(|| emit_partial::<WithAdapters>(&events)));
    }
}

/// Forwards to an owned `Inner` when serialized.
#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
#[deser(as = deser::adapters::FromInto<Inner>)]
struct FlatConverted(String, Vec<String>);

impl From<Inner> for FlatConverted {
    fn from(value: Inner) -> FlatConverted {
        FlatConverted(value.name, value.tags)
    }
}

impl From<FlatConverted> for Inner {
    fn from(value: FlatConverted) -> Inner {
        Inner {
            name: value.0,
            tags: value.1,
        }
    }
}

/// Forwards to an owned `FlatConverted` which forwards again.
#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
#[deser(as = deser::adapters::FromInto<FlatConverted>)]
struct FlatTwice(FlatConverted);

impl From<FlatConverted> for FlatTwice {
    fn from(value: FlatConverted) -> FlatTwice {
        FlatTwice(value)
    }
}

impl From<FlatTwice> for FlatConverted {
    fn from(value: FlatTwice) -> FlatConverted {
        value.0
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct WithFlattenedForwarding {
    before: String,
    #[deser(flatten)]
    flat: FlatTwice,
    after: Vec<String>,
}

#[derive(Serialize)]
struct PanickingInner {
    value: Panicking,
}

/// Forwards to an owned value that panics when serialized.
#[derive(Clone, Serialize)]
#[deser(as = deser::adapters::FromInto<PanickingInner>)]
struct PanickingFlat;

impl From<PanickingFlat> for PanickingInner {
    fn from(_: PanickingFlat) -> PanickingInner {
        PanickingInner { value: Panicking }
    }
}

#[derive(Serialize)]
struct WithPanickingFlattened {
    before: String,
    #[deser(flatten)]
    flat: PanickingFlat,
}

#[test]
fn test_flattened_forwarding() {
    let value = WithFlattenedForwarding {
        before: "a".into(),
        flat: FlatTwice(FlatConverted("b".into(), vec!["x".into(), "y".into()])),
        after: vec!["c".into()],
    };

    let mut expected = Vec::new();
    {
        let mut driver = SerializeDriver::new(&value);
        while let Some((event, _, _)) = driver.next().unwrap() {
            expected.push(event.to_static());
        }
    }
    // stop at every point and continue with drive, or abort at every point
    for skip in every_point(expected.len() + 1) {
        assert_eq!(drive_events(&value, skip, None).unwrap(), expected);
    }
    for abort in 0..expected.len() {
        assert!(drive_events(&value, 0, Some(abort)).is_err());
    }
    // drop the driver at every point
    for cut in every_point(expected.len()) {
        let mut driver = SerializeDriver::new(&value);
        for _ in 0..cut {
            driver.next().unwrap();
        }
    }
    assert_eq!(
        emit_partial::<WithFlattenedForwarding>(&expected).unwrap(),
        value
    );

    let value = WithPanickingFlattened {
        before: "a".into(),
        flat: PanickingFlat,
    };
    let rv = catch_unwind(AssertUnwindSafe(|| {
        let mut driver = SerializeDriver::new(&value);
        while driver.next().unwrap().is_some() {}
    }));
    assert!(rv.is_err());
    let rv = catch_unwind(AssertUnwindSafe(|| drive_events(&value, 0, None)));
    assert!(rv.is_err());
}

#[test]
fn test_drivers_move_between_threads() {
    let events = outer_events();
    let step = step();

    // the sinks are allocated on one thread and continued, finished or
    // dropped on another one (which frees them into its own cache)
    for split in (0..=events.len()).step_by(step) {
        for finish in [true, false] {
            let mut out = None::<Outer>;
            std::thread::scope(|scope| {
                let mut driver = DeserializeDriver::new(&mut out);
                for event in &events[..split] {
                    driver.emit(event.clone()).unwrap();
                }
                let rest = &events[split..];
                scope
                    .spawn(move || {
                        if finish {
                            for event in rest {
                                driver.emit(event.clone()).unwrap();
                            }
                        }
                        drop(driver);
                    })
                    .join()
                    .unwrap();
            });
            assert_eq!(out.is_some(), finish || split == events.len());
        }
    }

    // the emitters are created on one thread and continued on another one
    let value = emit_partial::<Outer>(&events).unwrap();
    for split in (0..=events.len()).step_by(step) {
        let mut driver = SerializeDriver::new(&value);
        let mut produced = Vec::new();
        for _ in 0..split {
            let (event, _, _) = driver.next().unwrap().unwrap();
            produced.push(event.to_static());
        }
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    while let Some((event, _, _)) = driver.next().unwrap() {
                        produced.push(event.to_static());
                    }
                })
                .join()
                .unwrap();
        });
        assert_eq!(produced, events);
    }
}

#[test]
fn test_owned_driver() {
    use deser::de::OwnedDriver;

    let events = outer_events();
    let step = step();

    // complete values, fed in two parts
    for split in (0..=events.len()).step_by(step) {
        let mut driver = OwnedDriver::<Outer>::new();
        driver
            .with(|driver| {
                for event in &events[..split] {
                    driver.emit(event.clone())?;
                }
                Ok::<_, Error>(())
            })
            .unwrap();
        // moving the driver (also to another thread) does not move the slot
        let driver = std::thread::spawn(move || {
            let mut driver = driver;
            driver
                .with(|driver| {
                    for event in &events_clone(split) {
                        driver.emit(event.clone())?;
                    }
                    Ok::<_, Error>(())
                })
                .unwrap();
            driver
        })
        .join()
        .unwrap();
        assert_eq!(
            driver.finish().unwrap(),
            emit_partial::<Outer>(&events).unwrap()
        );
    }

    // incomplete values are dropped or fail to finish
    for cut in (0..events.len()).step_by(step) {
        let mut driver = OwnedDriver::<Outer>::new();
        driver
            .with(|driver| {
                for event in &events[..cut] {
                    driver.emit(event.clone())?;
                }
                Ok::<_, Error>(())
            })
            .unwrap();
        if cut % 2 == 0 {
            drop(driver);
        } else {
            assert!(driver.finish().is_err());
        }
    }

    // a panic while the driver is lent out
    let mut driver = OwnedDriver::<Outer>::new();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        driver.with(|driver| {
            driver.emit(events[0].clone()).unwrap();
            panic!("boom");
        })
    }));
    drop(driver);
}

fn events_clone(split: usize) -> Vec<Event<'static>> {
    outer_events()[split..].to_vec()
}

/// Emits a map of the words of the input to their lengths, borrowing the
/// words.
fn emit_words<'de>(input: &'de str, driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
    driver.emit(Event::map_start())?;
    for word in input.split(' ') {
        driver.emit_borrowed(word)?;
        driver.emit(word.len() as u64)?;
    }
    driver.emit(Event::MapEnd)
}

#[test]
fn test_transient_driver() {
    // owned values copy the data that only lives for the call
    let mut out = None::<BTreeMap<String, u64>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        let input = String::from("hello transient world");
        driver
            .transient(|driver| emit_words(&input, driver))
            .unwrap();
    }
    assert_eq!(out.unwrap()["transient"], 9);

    // borrowed values cannot borrow it, `Cow` copies it
    let mut out = None::<BTreeMap<&'static str, u64>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        let input = String::from("a b");
        assert!(
            driver
                .transient(|driver| emit_words(&input, driver))
                .is_err()
        );
    }
    let mut out = None::<BTreeMap<Cow<'static, str>, u64>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        let input = String::from("a bb");
        driver
            .transient(|driver| emit_words(&input, driver))
            .unwrap();
    }
    let out = out.unwrap();
    assert!(out.keys().all(|key| matches!(key, Cow::Owned(_))));
    assert_eq!(out[&Cow::Borrowed("bb")], 2);

    // values that are buffered (untagged enums record their input) and
    // replayed later
    #[derive(Debug, Deserialize)]
    #[deser(untagged)]
    enum Words<'a> {
        #[allow(dead_code)]
        Numbers(Vec<u64>),
        Borrowed(BTreeMap<Cow<'a, str>, u64>),
    }
    let mut out = None::<Words<'static>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        let input = String::from("x yy");
        driver
            .transient(|driver| emit_words(&input, driver))
            .unwrap();
    }
    match out.unwrap() {
        Words::Borrowed(map) => assert_eq!(map[&Cow::Borrowed("yy")], 2),
        other => panic!("unexpected {other:?}"),
    }

    // a driver can be lent out in parts and nested
    let mut out = None::<Vec<String>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit(Event::seq_start()).unwrap();
        for word in ["a", "b"] {
            let word = word.to_string();
            driver
                .transient(|driver| {
                    let inner = format!("{word}!");
                    driver.emit_borrowed(word.as_str())?;
                    driver.transient(|driver| driver.emit_borrowed(inner.as_str()))
                })
                .unwrap();
        }
        driver.emit(Event::SeqEnd).unwrap();
    }
    assert_eq!(out.unwrap(), ["a", "a!", "b", "b!"]);
}

#[test]
fn test_transient_driver_with_layers() {
    use deser::de::{Layer, LayerEvent, Next};

    /// Passes events on unchanged.
    struct Passthrough;

    impl Layer for Passthrough {
        fn event<'de>(
            &mut self,
            event: LayerEvent<'_, 'de>,
            next: &mut Next<'_, 'de>,
        ) -> Result<(), Error> {
            next.emit(event)
        }
    }

    let mut out = None::<BTreeMap<Cow<'static, str>, u64>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.push_layer(Passthrough);
        let input = String::from("layered words");
        driver
            .transient(|driver| {
                driver.push_layer(Passthrough);
                emit_words(&input, driver)
            })
            .unwrap();
    }
    assert!(out.unwrap().keys().all(|key| matches!(key, Cow::Owned(_))));
}

#[test]
fn test_transient_driver_cannot_be_replaced() {
    let mut out = None::<Vec<u64>>;
    let mut driver = DeserializeDriver::new(&mut out);
    driver.emit(Event::seq_start()).unwrap();
    let input = String::from("x");
    // the output of the replacement, freed at the end
    let leaked = std::cell::Cell::new(std::ptr::null_mut::<()>());
    let rv = catch_unwind(AssertUnwindSafe(|| {
        driver.transient(|driver| {
            // a driver whose sink borrows the data of the call (its output
            // is leaked, a local would not live long enough)
            let other_out = Box::leak(Box::new(None::<Vec<&str>>));
            leaked.set((other_out as *mut Option<Vec<&str>>).cast());
            let mut other = DeserializeDriver::new(other_out);
            other.emit(Event::seq_start()).unwrap();
            other.emit_borrowed(input.as_str()).unwrap();
            std::mem::swap(driver, &mut other);
            // `other` is the original driver now, it's dropped here
        })
    }));
    assert!(rv.is_err());
    // the driver has no sink anymore, the replacement was dropped
    let rv = catch_unwind(AssertUnwindSafe(|| driver.emit(1u64)));
    assert!(rv.is_err());
    drop(driver);
    assert_eq!(out, None);
    // SAFETY: the replacement was dropped, nothing refers to its output
    drop(unsafe { Box::from_raw(leaked.get().cast::<Option<Vec<&str>>>()) });

    // wrapping the sink is not allowed either
    let mut out = None::<u64>;
    let mut driver = DeserializeDriver::new(&mut out);
    let rv = catch_unwind(AssertUnwindSafe(|| {
        driver.transient(|driver| driver.wrap_sink(|sink, _| sink));
    }));
    assert!(rv.is_err());
}

/// The order in which values were dropped.
type DropLog = std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>;

#[test]
fn test_sink_panics_in_drop() {
    // the sinks below a sink that panics while it's dropped are still
    // dropped in inverse order: a child that writes into its parent when
    // it's dropped does so before the parent is dropped.
    struct Parent {
        slot: Option<String>,
        log: DropLog,
    }

    impl Drop for Parent {
        fn drop(&mut self) {
            self.log.lock().unwrap().push("parent");
        }
    }

    impl<'de> Sink<'de> for Parent {
        fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
            Ok(())
        }

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            let log = self.log.clone();
            Ok(SinkHandle::arena(
                Child {
                    out: &mut self.slot,
                    log,
                },
                state,
            ))
        }
    }

    struct Child<'a> {
        out: &'a mut Option<String>,
        log: DropLog,
    }

    impl Drop for Child<'_> {
        fn drop(&mut self) {
            *self.out = Some("written by the child".into());
            self.log.lock().unwrap().push("child");
        }
    }

    impl<'de> Sink<'de> for Child<'_> {
        fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
            Ok(())
        }

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            let log = self.log.clone();
            Ok(SinkHandle::arena(Panicking(log), state))
        }
    }

    struct Panicking(DropLog);

    impl Drop for Panicking {
        fn drop(&mut self) {
            self.0.lock().unwrap().push("panicking");
            panic!("the sink panics while it's dropped");
        }
    }

    impl<'de> Sink<'de> for Panicking {
        fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
            Ok(())
        }
    }

    let log = DropLog::default();
    let rv = catch_unwind(AssertUnwindSafe(|| {
        let mut driver = DeserializeDriver::from_fn(|state| {
            SinkHandle::arena(
                Parent {
                    slot: Some("the value of the parent".into()),
                    log: log.clone(),
                },
                state,
            )
        });
        for _ in 0..3 {
            driver.emit(Event::seq_start()).unwrap();
        }
    }));
    assert!(rv.is_err());
    assert_eq!(*log.lock().unwrap(), ["panicking", "child", "parent"]);
}

#[test]
fn test_emitter_panics_in_drop() {
    // the frames below an emitter that panics while it's dropped are still
    // dropped in inverse order: a value that borrows from the emitter of
    // the frame below it is dropped before that emitter.
    use deser::ser::SeqEmitter;

    struct Root(DropLog);

    impl Serialize for Root {
        fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
            Ok(Emit::seq(
                OuterEmitter {
                    data: "owned by the outer emitter".into(),
                    done: false,
                    log: value.0.clone(),
                },
                state,
            ))
        }
    }

    struct OuterEmitter {
        data: String,
        done: bool,
        log: DropLog,
    }

    impl Drop for OuterEmitter {
        fn drop(&mut self) {
            self.log.lock().unwrap().push("outer");
        }
    }

    impl SeqEmitter for OuterEmitter {
        fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
            if std::mem::replace(&mut self.done, true) {
                return Ok(None);
            }
            Ok(Some(SerializeHandle::heap(Borrower {
                data: &self.data,
                log: self.log.clone(),
            })))
        }
    }

    /// Borrows from the outer emitter and reads it when it's dropped.
    struct Borrower<'a> {
        data: &'a String,
        log: DropLog,
    }

    impl Drop for Borrower<'_> {
        fn drop(&mut self) {
            assert_eq!(self.data, "owned by the outer emitter");
            self.log.lock().unwrap().push("borrower");
        }
    }

    impl Serialize for Borrower<'_> {
        fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
            Ok(Emit::seq(Middle(Some(Inner(value.log.clone()))), state))
        }
    }

    /// The frame of the borrower stays below the one that panics.
    struct Middle(Option<Inner>);

    impl SeqEmitter for Middle {
        fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
            Ok(self.0.take().map(SerializeHandle::heap))
        }
    }

    struct Inner(DropLog);

    impl Serialize for Inner {
        fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
            Ok(Emit::seq(PanickingEmitter(value.0.clone()), state))
        }
    }

    struct PanickingEmitter(DropLog);

    impl Drop for PanickingEmitter {
        fn drop(&mut self) {
            self.0.lock().unwrap().push("panicking");
            panic!("the emitter panics while it's dropped");
        }
    }

    impl SeqEmitter for PanickingEmitter {
        fn next(&mut self, _state: &mut State) -> Result<Option<SerializeHandle<'_>>, Error> {
            Ok(Some(SerializeHandle::heap(1u64)))
        }
    }

    // dropped with an open container on top and with a pending value
    for events in [3, 4] {
        let log = DropLog::default();
        let root = Root(log.clone());
        let rv = catch_unwind(AssertUnwindSafe(|| {
            let mut driver = SerializeDriver::new(&root);
            for _ in 0..events {
                driver.next().unwrap();
            }
        }));
        assert!(rv.is_err());
        assert_eq!(*log.lock().unwrap(), ["panicking", "borrower", "outer"]);
    }
}

#[test]
fn test_forwarded_values_dropped_in_inverse_order() {
    // a flattened value that forwards to a value which forwards again: the
    // second forwarded value borrows from the first one and is dropped
    // before it
    struct Forwarding(DropLog);

    impl Serialize for Forwarding {
        fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
            Ok(Emit::Forward(SerializeHandle::heap(First {
                text: "owned by the first value".into(),
                log: value.0.clone(),
            })))
        }
    }

    struct First {
        text: String,
        log: DropLog,
    }

    impl Drop for First {
        fn drop(&mut self) {
            self.log.lock().unwrap().push("first");
        }
    }

    impl Serialize for First {
        fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
            Ok(Emit::Forward(SerializeHandle::heap(Second {
                first: value,
            })))
        }
    }

    /// Borrows from the first value and reads it when it's dropped.
    struct Second<'a> {
        first: &'a First,
    }

    impl Drop for Second<'_> {
        fn drop(&mut self) {
            assert_eq!(self.first.text, "owned by the first value");
            self.first.log.lock().unwrap().push("second");
        }
    }

    impl Serialize for Second<'_> {
        fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
            let mut fields = BTreeMap::new();
            fields.insert("text", value.first.text.as_str());
            Ok(Emit::Forward(SerializeHandle::arena(fields, state)))
        }
    }

    #[derive(Serialize)]
    struct WithForwarding {
        before: u32,
        #[deser(flatten)]
        flat: Forwarding,
    }

    let log = DropLog::default();
    let value = WithForwarding {
        before: 1,
        flat: Forwarding(log.clone()),
    };
    let events = drive_events(&value, 0, None).unwrap();
    assert_eq!(events.len(), 6);
    assert_eq!(*log.lock().unwrap(), ["second", "first"]);
}
