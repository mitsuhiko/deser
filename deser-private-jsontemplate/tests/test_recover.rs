//! Tests for recovering from errors of items (see `Sink::recover`).
//!
//! The errors the sinks recover from have the offset of the event that
//! failed, formats only resolve lines and columns for the errors they
//! return.
use super::dialect;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use deser::adapters::{DefaultOnError, MapSkipError, VecSkipError};
use deser::de::{Layer, LayerEvent, Limits, Next, Sink, SinkHandle};
use deser::{Deserialize, Error, Event, State};

/// A sequence which keeps the errors of the items that failed.
#[derive(Debug, PartialEq)]
struct Items<T>(Vec<Result<T, String>>);

struct ItemsSink<'a, T> {
    out: &'a mut Option<Items<T>>,
    items: Vec<Result<T, String>>,
    current: Option<T>,
}

impl<T> ItemsSink<'_, T> {
    fn flush(&mut self) {
        if let Some(value) = self.current.take() {
            self.items.push(Ok(value));
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Items<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(ItemsSink {
            out,
            items: Vec::new(),
            current: None,
        })
    }
}

impl<'de, T: Deserialize<'de>> Sink<'de> for ItemsSink<'_, T> {
    fn expecting(&self) -> std::borrow::Cow<'_, str> {
        "sequence".into()
    }

    fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.flush();
        Ok(T::deserialize_into(&mut self.current))
    }

    fn recover(&mut self, err: Error, _state: &mut State) -> Result<(), Error> {
        self.current = None;
        self.items.push(Err(err.to_string()));
        Ok(())
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        self.flush();
        *self.out = Some(Items(std::mem::take(&mut self.items)));
        Ok(())
    }
}

/// A map which keeps the errors of the entries that failed.
#[derive(Debug, PartialEq)]
struct Entries<K, V>(Vec<Result<(K, V), String>>);

struct EntriesSink<'a, K, V> {
    out: &'a mut Option<Entries<K, V>>,
    entries: Vec<Result<(K, V), String>>,
    key: Option<K>,
    value: Option<V>,
}

impl<K, V> EntriesSink<'_, K, V> {
    fn flush(&mut self) {
        if let (Some(key), Some(value)) = (self.key.take(), self.value.take()) {
            self.entries.push(Ok((key, value)));
        }
    }
}

impl<'de, K: Deserialize<'de>, V: Deserialize<'de>> Deserialize<'de> for Entries<K, V> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(EntriesSink {
            out,
            entries: Vec::new(),
            key: None,
            value: None,
        })
    }
}

impl<'de, K: Deserialize<'de>, V: Deserialize<'de>> Sink<'de> for EntriesSink<'_, K, V> {
    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.flush();
        Ok(K::deserialize_into(&mut self.key))
    }

    fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(V::deserialize_into(&mut self.value))
    }

    fn recover(&mut self, err: Error, _state: &mut State) -> Result<(), Error> {
        self.key = None;
        self.value = None;
        self.entries.push(Err(err.to_string()));
        Ok(())
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        self.flush();
        *self.out = Some(Entries(std::mem::take(&mut self.entries)));
        Ok(())
    }
}

fn ok<T>(value: T) -> Result<T, String> {
    Ok(value)
}

fn err<T>(msg: &str) -> Result<T, String> {
    Err(msg.to_string())
}

#[test]
fn test_recover_atoms_and_containers() {
    let items: Items<u32> =
        dialect::from_str(r#"[1, "x", [1, [2]], {"a": {"b": 1}}, 5, true]"#).unwrap();
    assert_eq!(
        items,
        Items(vec![
            ok(1),
            err("Unexpected: unexpected string, expected u32 at offset 4"),
            err("Unexpected: unexpected sequence, expected u32 at offset 9"),
            err("Unexpected: unexpected map, expected u32 at offset 19"),
            ok(5),
            err("Unexpected: unexpected bool, expected u32 at offset 39"),
        ])
    );
}

#[test]
fn test_recover_nested() {
    // the error is deep inside the second item, the rest of it is skipped
    let items: Items<Vec<Vec<u32>>> =
        dialect::from_str(r#"[[[1]], [[1, "x", [3, {"a": 1}]], [4]], [[5]]]"#).unwrap();
    assert_eq!(
        items,
        Items(vec![
            ok(vec![vec![1]]),
            err("Unexpected: unexpected string, expected u32 at offset 13"),
            ok(vec![vec![5]]),
        ])
    );
}

#[test]
fn test_recover_nested_boundaries() {
    // the innermost container that recovers handles the error
    let items: Items<Items<u32>> = dialect::from_str(r#"[[1, "x", 2], "y", [[3]], [4]]"#).unwrap();
    assert_eq!(
        items,
        Items(vec![
            ok(Items(vec![
                ok(1),
                err("Unexpected: unexpected string, expected u32 at offset 5"),
                ok(2),
            ])),
            err("Unexpected: unexpected string, expected sequence at offset 14"),
            ok(Items(vec![err(
                "Unexpected: unexpected sequence, expected u32 at offset 20"
            )])),
            ok(Items(vec![ok(4)])),
        ])
    );
}

#[derive(Debug, PartialEq, Deserialize)]
#[deser(deny_unknown_fields)]
struct Point {
    x: u32,
    y: u32,
}

#[test]
fn test_recover_struct_errors() {
    let items: Items<Point> = dialect::from_str(
        r#"[
            {"x": 1, "y": 2},
            {"x": 1},
            {"x": [1], "y": 2},
            {"z": {"a": [1]}, "x": 1, "y": 2},
            {"x": 3, "y": 4}
        ]"#,
    )
    .unwrap();
    assert_eq!(
        items,
        Items(vec![
            ok(Point { x: 1, y: 2 }),
            err("MissingField: missing field `y` at offset 51"),
            err("Unexpected: unexpected sequence, expected u32 at offset 72"),
            err("Unexpected: unknown field `z`, expected `x` or `y` at offset 99"),
            ok(Point { x: 3, y: 4 }),
        ])
    );
}

#[test]
fn test_recover_keys() {
    // after a key failed, its value is skipped (atom or container)
    let entries: Entries<u32, u32> =
        dialect::from_str(r#"{"1": 1, "x": 2, "3": {"a": [1]}, "y": [1, {"b": [2]}], "4": 4}"#)
            .unwrap();
    assert_eq!(
        entries,
        Entries(vec![
            ok((1, 1)),
            err(r#"Unexpected: invalid value "x", expected u32 at offset 9"#),
            err("Unexpected: unexpected map, expected u32 at offset 22"),
            err(r#"Unexpected: invalid value "y", expected u32 at offset 34"#),
            ok((4, 4)),
        ])
    );
}

#[test]
fn test_unrecovered_errors() {
    // without a sink that recovers, errors fail the deserialization as
    // before, also inside containers that recover from other errors
    let err = dialect::from_str::<Vec<Vec<u32>>>(r#"[[1], [2, "x"]]"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected string, expected u32 at line 1 column 11"
    );
    let err = dialect::from_str::<Items<u32>>(r#"{"a": 1}"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected map, expected sequence at line 1 column 1"
    );
}

#[test]
fn test_syntax_errors_are_not_recovered() {
    let err = dialect::from_str::<Items<Vec<u32>>>(r#"[[1, "x", , 2]]"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected comma at line 1 column 11"
    );
}

#[test]
fn test_layer_errors_are_not_recovered() {
    // the limit is enforced for the skipped parts too
    let mut de = dialect::Deserializer::from_str(r#"[1, ["x", [[[1]]]]]"#);
    let err = de
        .deserialize_with::<Items<Vec<Vec<u32>>>, _>(|driver| {
            driver.push_layer(Limits::new().max_depth(4))
        })
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: recursion limit exceeded at line 1 column 13"
    );
}

/// Records the position the layers see for every event.
#[derive(Clone, Default)]
struct Positions(Arc<Mutex<Vec<String>>>);

impl Layer for Positions {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        let desc = match event.event() {
            Event::Atom(atom) => format!("{:?}", atom),
            Event::MapStart(_) => "{".into(),
            Event::SeqStart(_) => "[".into(),
            Event::MapEnd => "}".into(),
            Event::SeqEnd => "]".into(),
        };
        let state = next.state();
        let key = if state.is_map_key() { " key" } else { "" };
        self.0
            .lock()
            .unwrap()
            .push(format!("{}{} @{}", desc, key, state.depth()));
        next.emit(event)
    }
}

#[test]
fn test_skipped_events_keep_positions() {
    // layers see the same positions for skipped events as for delivered
    // ones
    let input = r#"{"a": [{"k": 1}, {"k": {"x": [1]}, "j": 2}], "b": 3}"#;
    let positions = Positions::default();
    let mut de = dialect::Deserializer::from_str(input);
    de.deserialize_with::<BTreeMap<String, deser::de::Recording>, _>(|driver| {
        driver.push_layer(positions.clone())
    })
    .unwrap();
    let expected = positions.0.lock().unwrap().clone();

    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct Doc {
        a: Items<BTreeMap<String, u32>>,
        b: u32,
    }

    let positions = Positions::default();
    let mut de = dialect::Deserializer::from_str(input);
    let doc = de
        .deserialize_with::<Doc, _>(|driver| driver.push_layer(positions.clone()))
        .unwrap();
    assert_eq!(*positions.0.lock().unwrap(), expected);
    assert_eq!(doc.b, 3);
    assert_eq!(doc.a.0.len(), 2);
    assert!(doc.a.0[1].is_err());
}

#[test]
fn test_skipped_values_of_keys_keep_positions() {
    let input = r#"{"1": [1], "x": [2, {"y": [3]}], "z": 5, "2": [4]}"#;
    let positions = Positions::default();
    let mut de = dialect::Deserializer::from_str(input);
    de.deserialize_with::<deser::de::Recording, _>(|driver| driver.push_layer(positions.clone()))
        .unwrap();
    let expected = positions.0.lock().unwrap().clone();

    let positions = Positions::default();
    let mut de = dialect::Deserializer::from_str(input);
    let entries = de
        .deserialize_with::<Entries<u32, Vec<u32>>, _>(|driver| {
            driver.push_layer(positions.clone())
        })
        .unwrap();
    assert_eq!(*positions.0.lock().unwrap(), expected);
    assert_eq!(entries.0.len(), 4);
    assert_eq!(entries.0[0], Ok((1, vec![1])));
    assert!(entries.0[1].is_err());
    assert!(entries.0[2].is_err());
    assert_eq!(entries.0[3], Ok((2, vec![4])));
}

#[derive(Debug, PartialEq, Deserialize)]
#[deser(tag = "type")]
enum Tagged {
    #[deser(rename = "a")]
    A { items: Items<u32>, rest: u32 },
}

#[test]
fn test_recover_in_replayed_values() {
    // the tag comes last so the content is recorded and replayed
    let value: Tagged =
        dialect::from_str(r#"{"items": [1, {"x": [2]}, 3], "rest": 4, "type": "a"}"#).unwrap();
    let Tagged::A { items, rest } = value;
    assert_eq!(rest, 4);
    assert_eq!(items.0.len(), 3);
    assert_eq!(items.0[0], Ok(1));
    assert!(items.0[1].is_err());
    assert_eq!(items.0[2], Ok(3));
}

#[derive(Debug, Default, PartialEq, Deserialize)]
struct Lenient {
    #[deser(as = DefaultOnError)]
    point: Option<Point>,
    #[deser(as = VecSkipError)]
    points: Vec<Point>,
    #[deser(as = MapSkipError)]
    named: BTreeMap<String, Point>,
    after: u32,
}

#[test]
fn test_adapters() {
    let value: Lenient = dialect::from_str(
        r#"{
            "point": {"x": 1, "y": {"deep": [1, 2, {"deeper": null}]}},
            "points": [{"x": 1, "y": 2}, {"x": [[]], "y": 2}, {"x": 3, "y": 4}],
            "named": {"a": {"x": 1, "y": 2}, "b": {"x": 1}, "c": {"x": 3, "y": 4}},
            "after": 42
        }"#,
    )
    .unwrap();
    assert_eq!(
        value,
        Lenient {
            point: None,
            points: vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }],
            named: [
                ("a".to_string(), Point { x: 1, y: 2 }),
                ("c".to_string(), Point { x: 3, y: 4 }),
            ]
            .into_iter()
            .collect(),
            after: 42,
        }
    );
}

#[test]
fn test_adapters_do_not_hide_syntax_errors() {
    let err = dialect::from_str::<Lenient>(r#"{"point": {"x": [1, }, "after": 1}"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: expected a value at line 1 column 21"
    );
}

#[test]
fn test_default_on_error_with_limits() {
    let mut de = dialect::Deserializer::from_str(r#"{"point": [[[[1]]]], "after": 1}"#);
    let err = de
        .deserialize_with::<Lenient, _>(|driver| driver.push_layer(Limits::new().max_depth(3)))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: recursion limit exceeded at line 1 column 13"
    );
}
