use deser::de::{DeserializeDriver, Sink, SinkHandle};
use deser::ser::{Chunk, Serialize, SerializeDriver};
use deser::{make_slot_wrapper, Atom, Deserialize, Error, Event, State};

/// Event data used by the tests.
#[derive(Debug, Default, Clone, PartialEq)]
struct Marker(u32);

/// Captures the marker attached to the event it receives.
#[derive(Debug, PartialEq)]
struct Probe(Option<u32>);

make_slot_wrapper!(ProbeSlot);

impl Sink for ProbeSlot<Probe> {
    fn atom(&mut self, _atom: Atom, state: &mut State) -> Result<(), Error> {
        **self = Some(Probe(state.event::<Marker>().map(|marker| marker.0)));
        Ok(())
    }
}

impl Deserialize for Probe {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        ProbeSlot::make_handle(out)
    }
}

/// Emits events, attaching a marker to the events where one is given.
fn deserialize<T: Deserialize>(events: Vec<(Event<'_>, Option<u32>)>) -> T {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for (event, marker) in events {
            match marker {
                Some(marker) => driver
                    .emit_with(event, |state| state.event_mut::<Marker>().0 = marker)
                    .unwrap(),
                None => driver.emit(event).unwrap(),
            }
            assert!(!driver.state().has_event_data());
        }
    }
    out.unwrap()
}

#[test]
fn test_event_data_is_attached_to_one_event() {
    let probes: Vec<Probe> = deserialize(vec![
        (Event::SeqStart, None),
        (1u64.into(), Some(1)),
        (2u64.into(), None),
        (3u64.into(), Some(3)),
        (Event::SeqEnd, None),
    ]);
    assert_eq!(probes, [Probe(Some(1)), Probe(None), Probe(Some(3))]);
}

#[derive(Debug, PartialEq, Deserialize)]
#[deser(tag = "type")]
enum Probed {
    Variant { first: Probe, second: Probe },
}

#[test]
fn test_event_data_is_replayed() {
    // the tag comes last which means that the fields are recorded and
    // replayed once the tag is known.
    let value: Probed = deserialize(vec![
        (Event::MapStart, None),
        ("first".into(), None),
        (1u64.into(), Some(1)),
        ("second".into(), Some(99)),
        (2u64.into(), None),
        ("type".into(), None),
        ("Variant".into(), Some(100)),
        (Event::MapEnd, None),
    ]);
    assert_eq!(
        value,
        Probed::Variant {
            first: Probe(Some(1)),
            second: Probe(None),
        }
    );
}

/// Attaches a marker to its event when serialized.
struct Marked(u32, Option<u32>);

impl Serialize for Marked {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        if let Some(marker) = self.1 {
            state.event_mut::<Marker>().0 = marker;
        }
        Ok(Chunk::Atom(Atom::U64(self.0 as u64)))
    }
}

#[test]
fn test_event_data_when_serializing() {
    let values = vec![Marked(1, Some(10)), Marked(2, None), Marked(3, Some(30))];
    let expected = vec![
        (Event::SeqStart, None),
        (1u64.into(), Some(10)),
        (2u64.into(), None),
        (3u64.into(), Some(30)),
        (Event::SeqEnd, None),
    ];

    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(&values);
    while let Some((event, _, state)) = driver.next().unwrap() {
        events.push((event.to_static(), state.event::<Marker>().map(|x| x.0)));
    }
    assert_eq!(events, expected);

    let mut events = Vec::new();
    SerializeDriver::new(&values)
        .drive(|event, _, state| {
            events.push((event.to_static(), state.event::<Marker>().map(|x| x.0)));
            Ok(())
        })
        .unwrap();
    assert_eq!(events, expected);
}
