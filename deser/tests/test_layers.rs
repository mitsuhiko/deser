use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use deser::de::{DeserializeDriver, Format, Layer, LayerEvent, Limits, Next, Sink, SinkHandle};
use deser::ser::{self, SerializeDriver};
use deser::{
    Atom, Deserialize, Error, ErrorAttachment, ErrorContext, ErrorKind, Event, Serialize, State,
};

/// Removes the length from container starts, the tests are not about it.
fn without_len(event: deser::Event<'static>) -> deser::Event<'static> {
    match event {
        deser::Event::MapStart(shape) => {
            deser::Event::MapStart(deser::ContainerShape::new().with_order(shape.order()))
        }
        deser::Event::SeqStart(shape) => {
            deser::Event::SeqStart(deser::ContainerShape::new().with_order(shape.order()))
        }
        event => event,
    }
}

/// Emits events into a driver, each with an input range of one byte.
fn emit_all(driver: &mut DeserializeDriver<'_, '_>, events: Vec<Event<'_>>) -> Result<(), Error> {
    for (idx, event) in events.into_iter().enumerate() {
        driver.state_mut().set_input_range(idx, idx + 1);
        driver.emit(event)?;
    }
    Ok(())
}

/// A format for tests that emits a list of events.
struct Events<'a>(Vec<Event<'a>>);

impl<'de> Format<'de> for Events<'de> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
        for (idx, event) in std::mem::take(&mut self.0).into_iter().enumerate() {
            driver.state_mut().set_input_range(idx, idx + 1);
            driver.emit_borrowed(event)?;
        }
        Ok(())
    }
}

/// Records what the layer sees.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

struct Observer(&'static str, Log);

impl Layer for Observer {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        self.1.0.lock().unwrap().push(format!(
            "{}: {:?} key={} depth={} borrowed={}",
            self.0,
            event.event(),
            next.state().is_map_key(),
            next.state().depth(),
            event.is_borrowed()
        ));
        next.emit(event)
    }
}

#[test]
fn test_layer_order_and_position() {
    let log = Log::default();
    let mut out = None::<BTreeMap<String, Vec<u32>>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.push_layer(Observer("a", log.clone()));
        driver.push_layer(Observer("b", log.clone()));
        driver.emit(Event::map_start()).unwrap();
        driver.emit_borrowed("x").unwrap();
        driver.emit(Event::seq_start()).unwrap();
        driver.emit(1u64).unwrap();
        driver.emit(Event::SeqEnd).unwrap();
        driver.emit(Event::MapEnd).unwrap();
    }
    assert_eq!(out.unwrap()["x"], [1]);
    assert_eq!(
        log.take(),
        [
            "a: MapStart key=false depth=0 borrowed=false",
            "b: MapStart key=false depth=0 borrowed=false",
            "a: Atom(Str(\"x\")) key=true depth=1 borrowed=true",
            "b: Atom(Str(\"x\")) key=true depth=1 borrowed=true",
            "a: SeqStart key=false depth=1 borrowed=false",
            "b: SeqStart key=false depth=1 borrowed=false",
            "a: Atom(U64(1)) key=false depth=2 borrowed=false",
            "b: Atom(U64(1)) key=false depth=2 borrowed=false",
            "a: SeqEnd key=false depth=2 borrowed=false",
            "b: SeqEnd key=false depth=2 borrowed=false",
            "a: MapEnd key=false depth=1 borrowed=false",
            "b: MapEnd key=false depth=1 borrowed=false",
        ]
    );
}

#[test]
fn test_borrowed_events_pass_through_layers() {
    let input = String::from("hello");
    let log = Log::default();
    let value: Vec<&str> = Events(vec![
        Event::seq_start(),
        input.as_str().into(),
        Event::SeqEnd,
    ])
    .deserialize_with(|driver| driver.push_layer(Observer("a", log.clone())))
    .unwrap();
    assert_eq!(value, ["hello"]);
}

/// Duplicates every item of sequences and drops nulls.
struct DuplicateItems;

impl Layer for DuplicateItems {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        match event.event() {
            Event::Atom(Atom::Null) => Ok(()),
            Event::Atom(atom) => {
                let copy = atom.to_static();
                next.emit(event)?;
                next.emit(LayerEvent::new(Event::Atom(copy)))
            }
            _ => next.emit(event),
        }
    }
}

#[test]
fn test_layers_can_change_events() {
    let value: Vec<u64> = Events(vec![
        Event::seq_start(),
        1u64.into(),
        ().into(),
        2u64.into(),
        Event::SeqEnd,
    ])
    .deserialize_with(|driver| driver.push_layer(DuplicateItems))
    .unwrap();
    assert_eq!(value, [1, 1, 2, 2]);
}

/// Counts the events the layer sees.
#[derive(Clone, Default)]
struct Counter(Arc<Mutex<usize>>);

impl Layer for Counter {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        *self.0.lock().unwrap() += 1;
        next.emit(event)
    }
}

/// Adds one to all integers.
struct Increment;

impl Layer for Increment {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        match event.event() {
            Event::Atom(Atom::U64(value)) => {
                let value = *value + 1;
                next.emit(LayerEvent::new(value.into()))
            }
            _ => next.emit(event),
        }
    }
}

#[test]
fn test_replayed_events_skip_layers() {
    #[derive(Deserialize, Debug, PartialEq)]
    #[deser(untagged)]
    enum Value {
        Flag(bool),
        Numbers(Vec<u64>),
    }

    let counter = Counter::default();
    let value: Value = Events(vec![
        Event::seq_start(),
        1u64.into(),
        2u64.into(),
        Event::SeqEnd,
    ])
    .deserialize_with(|driver| {
        driver.push_layer(counter.clone());
        driver.push_layer(Increment);
    })
    .unwrap();
    // the value is replayed twice (into both variants), but the layers only
    // see the events once and the changes are not applied again
    assert_eq!(value, Value::Numbers(vec![2, 3]));
    assert_eq!(*counter.0.lock().unwrap(), 4);
}

#[test]
fn test_limits() {
    fn check(limits: Limits, events: Vec<Event<'static>>) -> Result<(), String> {
        let rv: Result<deser::de::Recording, _> =
            Events(events).deserialize_with(|driver| driver.push_layer(limits));
        rv.map(|_| ()).map_err(|err| err.to_string())
    }

    let nested = || {
        vec![
            Event::seq_start(),
            Event::seq_start(),
            1u64.into(),
            Event::SeqEnd,
            Event::SeqEnd,
        ]
    };
    assert_eq!(check(Limits::new().max_depth(2), nested()), Ok(()));
    assert_eq!(
        check(Limits::new().max_depth(1), nested()),
        Err("Unexpected: recursion limit exceeded at offset 1".into())
    );
    assert_eq!(check(Limits::new().max_events(5), nested()), Ok(()));
    assert_eq!(
        check(Limits::new().max_events(4), nested()),
        Err("Unexpected: too many events at offset 4".into())
    );

    let map = || {
        vec![
            Event::map_start(),
            "a".into(),
            1u64.into(),
            "b".into(),
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            3u64.into(),
            Event::SeqEnd,
            Event::MapEnd,
        ]
    };
    assert_eq!(check(Limits::new().max_items(3), map()), Ok(()));
    assert_eq!(
        check(Limits::new().max_items(2), map()),
        Err("Unexpected: too many items at offset 7".into())
    );
    assert_eq!(check(Limits::new().max_len(1), map()), Ok(()));
    assert_eq!(
        check(
            Limits::new().max_len(3),
            vec![Event::seq_start(), "abcd".into(), Event::SeqEnd]
        ),
        Err("Unexpected: string or bytes too long at offset 1".into())
    );
}

#[test]
fn test_error_context() {
    // sink errors get the offset of the event
    let mut out = None::<Vec<Vec<u32>>>;
    let mut driver = DeserializeDriver::new(&mut out);
    let err = emit_all(
        &mut driver,
        vec![
            Event::seq_start(),
            Event::seq_start(),
            1u64.into(),
            true.into(),
        ],
    )
    .unwrap_err();
    assert_eq!(err.offset(), Some(3));
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected bool, expected u32 at offset 3"
    );

    // without a range there is no offset
    let mut out = None::<u32>;
    let err = DeserializeDriver::new(&mut out).emit(true).unwrap_err();
    assert_eq!(err.offset(), None);

    // the context functions see the state at the time of the error
    let mut out = None::<Vec<Vec<u32>>>;
    let mut driver = DeserializeDriver::new(&mut out);
    driver.state_mut().add_error_context::<Depths>();
    driver.state_mut().add_error_context::<Depths>();
    let err = emit_all(
        &mut driver,
        vec![Event::seq_start(), Event::seq_start(), "x".into()],
    )
    .unwrap_err();
    // registering the type twice has no effect
    assert_eq!(err.attachment::<Depths>().unwrap().0, vec![2]);
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected string, expected u32 at offset 2 (depths: [2])"
    );

    // errors of layers get the context too
    let mut out = None::<Vec<u32>>;
    let mut driver = DeserializeDriver::new(&mut out);
    driver.push_layer(Limits::new().max_len(0));
    let err = emit_all(&mut driver, vec![Event::seq_start(), "x".into()]).unwrap_err();
    assert_eq!(err.offset(), Some(1));
}

/// The depths of the states an error passed through.
#[derive(Debug)]
struct Depths(Vec<usize>);

impl ErrorAttachment for Depths {
    fn fmt_context(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, " (depths: {:?})", self.0)
    }
}

impl ErrorContext for Depths {
    fn add_context(mut err: Error, state: &State) -> Error {
        match err.attachment_mut::<Depths>() {
            Some(depths) => {
                depths.0.push(state.depth());
                err
            }
            None => err.with_attachment(Depths(vec![state.depth()])),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Marker(u32);

impl ErrorAttachment for Marker {}

/// Sets a marker in the state for every map key.
struct MarkKeys;

impl Layer for MarkKeys {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        let state = next.state_mut();
        state.set_replayable::<Marker>();
        state.add_error_context::<Marker>();
        if let Event::Atom(Atom::U64(value)) = event.event()
            && state.is_map_key()
        {
            state.get_mut::<Marker>().0 = *value as u32;
        }
        next.emit(event)
    }
}

impl ErrorContext for Marker {
    fn add_context(err: Error, state: &State) -> Error {
        match state.get::<Marker>() {
            Some(marker) => err.with_attachment(marker.clone()),
            None => err,
        }
    }
}

#[test]
fn test_error_context_of_replayed_values() {
    #[derive(Deserialize, Debug)]
    #[deser(tag = "type")]
    #[allow(dead_code)]
    enum Tagged {
        Item { values: BTreeMap<u32, bool> },
    }

    // the tag comes last, the values are replayed.  The error has the
    // offset of the value and the context from the replayed state.
    let rv: Result<Tagged, _> = Events(vec![
        Event::map_start(),
        "values".into(),
        Event::map_start(),
        1u64.into(),
        true.into(),
        2u64.into(),
        "no".into(),
        Event::MapEnd,
        "type".into(),
        "Item".into(),
        Event::MapEnd,
    ])
    .deserialize_with(|driver| driver.push_layer(MarkKeys));
    let err = rv.unwrap_err();
    assert_eq!(err.offset(), Some(6));
    assert_eq!(err.attachment::<Marker>().unwrap().0, 2);
}

/// Deserializes strings in upper case.
struct UppercaseSink<'a, 'de>(SinkHandle<'a, 'de>);

impl<'a, 'de> Sink<'de> for UppercaseSink<'a, 'de> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Str(s) => self.0.atom(Atom::Str(s.to_uppercase().into()), state),
            other => self.0.atom(other, state),
        }
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.0.finish(state)
    }

    fn expecting(&self) -> std::borrow::Cow<'_, str> {
        self.0.expecting()
    }
}

#[test]
fn test_wrap_sink() {
    let value: String = Events(vec!["hello".into()])
        .deserialize_with(|driver| {
            driver.wrap_sink(|sink| SinkHandle::boxed(UppercaseSink(sink)));
        })
        .unwrap();
    assert_eq!(value, "HELLO");
}

#[test]
#[should_panic(expected = "sinks can only be wrapped before events are emitted")]
fn test_wrap_sink_after_events() {
    let mut out = None::<Vec<u32>>;
    let mut driver = DeserializeDriver::new(&mut out);
    driver.emit(Event::seq_start()).unwrap();
    driver.wrap_sink(|sink| sink);
}

#[test]
fn test_format() {
    let value: Vec<u32> = Events(vec![Event::seq_start(), 1u64.into(), Event::SeqEnd])
        .deserialize()
        .unwrap();
    assert_eq!(value, [1]);

    let err = Events(vec![]).deserialize::<u32>().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
}

/// Drops map entries with null values (serialization).
#[derive(Default)]
struct SkipNulls {
    key: Option<Event<'static>>,
}

impl ser::Layer for SkipNulls {
    fn event(&mut self, event: Event<'_>, next: &mut ser::Next<'_>) -> Result<(), Error> {
        if let Some(key) = self.key.take() {
            if event == Event::Atom(Atom::Null) {
                return Ok(());
            }
            next.emit_key(key)?;
        } else if next.state().is_map_key() && matches!(event, Event::Atom(_)) {
            self.key = Some(event.to_static());
            return Ok(());
        }
        next.emit(event)
    }
}

fn serialize_with_layers<F: FnOnce(&mut SerializeDriver)>(
    value: &dyn Serialize,
    setup: F,
) -> Vec<Event<'static>> {
    let mut driver = SerializeDriver::new(value);
    setup(&mut driver);
    let mut events = Vec::new();
    driver
        .drive(|event, _| {
            events.push(without_len(event.to_static()));
            Ok(())
        })
        .unwrap();
    events
}

#[test]
fn test_ser_layers() {
    let mut map = BTreeMap::new();
    map.insert("a", Some(1u32));
    map.insert("b", None);
    map.insert("c", Some(3));
    let events = serialize_with_layers(&map, |driver| driver.push_layer(SkipNulls::default()));
    assert_eq!(
        events,
        [
            Event::MapStart(deser::ContainerShape::new().with_order(deser::Order::Sorted)),
            "a".into(),
            1u64.into(),
            "c".into(),
            3u64.into(),
            Event::MapEnd
        ]
    );
}

#[test]
fn test_ser_error_context() {
    struct Fails;

    impl Serialize for Fails {
        fn serialize(&self, _state: &mut State) -> Result<deser::ser::Chunk<'_>, Error> {
            Err(Error::new(ErrorKind::Unexpected, "nope"))
        }
    }

    let value = vec![vec![Fails]];
    let mut driver = SerializeDriver::new(&value);
    driver.state_mut().add_error_context::<Depths>();
    let err = driver.drive(|_, _| Ok(())).unwrap_err();
    assert_eq!(err.to_string(), "Unexpected: nope (depths: [2])");

    let mut driver = SerializeDriver::new(&value);
    driver.state_mut().add_error_context::<Depths>();
    let err = loop {
        match driver.next() {
            Ok(Some(_)) => {}
            Ok(None) => panic!("expected an error"),
            Err(err) => break err,
        }
    };
    assert_eq!(err.to_string(), "Unexpected: nope (depths: [2])");
}

#[test]
#[should_panic(expected = "layers are only supported by SerializeDriver::drive")]
fn test_ser_layers_require_drive() {
    let value = vec![1u32];
    let mut driver = SerializeDriver::new(&value);
    driver.push_layer(SkipNulls::default());
    let _ = driver.next();
}

/// Upper cases map keys (serialization).
struct UppercaseKeys;

impl ser::Layer for UppercaseKeys {
    fn event(&mut self, event: Event<'_>, next: &mut ser::Next<'_>) -> Result<(), Error> {
        match event {
            Event::Atom(Atom::Str(key)) if next.state().is_map_key() => {
                next.emit(Event::from(key.to_uppercase()))
            }
            event => next.emit(event),
        }
    }
}

#[test]
fn test_ser_layers_compose_with_delayed_keys() {
    let mut map = BTreeMap::new();
    map.insert("a", Some(1u32));
    map.insert("b", None);
    let events = serialize_with_layers(&map, |driver| {
        driver.push_layer(SkipNulls::default());
        driver.push_layer(UppercaseKeys);
    });
    assert_eq!(
        events,
        [
            Event::MapStart(deser::ContainerShape::new().with_order(deser::Order::Sorted)),
            "A".into(),
            1u64.into(),
            Event::MapEnd
        ]
    );
}
