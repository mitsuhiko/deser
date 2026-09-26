use std::collections::BTreeMap;

use deser::State;
use deser::de::{Deserialize, DeserializeDriver, Format, Sink, SinkHandle};
use deser::{Atom, Error, Event};
use deser_path::{Path, PathLayer, PathSegment};

#[derive(Debug, PartialEq, Eq)]
struct MyBool(bool);

deser::make_slot_wrapper!(SlotWrapper);

impl<'de> Deserialize<'de> for MyBool {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SlotWrapper::make_handle(out)
    }
}

impl<'de> Sink<'de> for SlotWrapper<MyBool> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Bool(value) => {
                let path = state.get::<Path>().unwrap();
                assert_eq!(path.segments().len(), 1);
                **self = Some(MyBool(value));
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}

#[test]
fn test_path() {
    let mut out = None::<BTreeMap<String, MyBool>>;

    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.push_layer(PathLayer::new());
        driver.emit(Event::map_start()).unwrap();
        driver.emit("foo").unwrap();
        driver.emit(true).unwrap();
        driver.emit("bar").unwrap();
        driver.emit(false).unwrap();
        driver.emit(Event::MapEnd).unwrap();
    }

    let map = out.unwrap();

    assert_eq!(map["foo"], MyBool(true));
    assert_eq!(map["bar"], MyBool(false));
}

#[derive(Debug)]
struct RecordPath(String);

impl<'de> Deserialize<'de> for RecordPath {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SlotWrapper::make_handle(out)
    }
}

impl<'de> Sink<'de> for SlotWrapper<RecordPath> {
    fn atom(&mut self, _atom: Atom, state: &mut State) -> Result<(), Error> {
        let path = state.get::<Path>().unwrap();
        **self = Some(RecordPath(path.to_string()));
        Ok(())
    }
}

fn from_json<'de, T: Deserialize<'de>>(json: &'de str) -> Result<T, Error> {
    deser_json::Deserializer::from_str(json)
        .deserialize_with(|driver| driver.push_layer(PathLayer::new()))
}

#[test]
fn test_nested_paths() {
    let map: BTreeMap<String, Vec<BTreeMap<String, RecordPath>>> =
        from_json(r#"{"a": [{"x": 1, "y": 2}, {"z": 3}], "b": [{"w": 4}]}"#).unwrap();
    assert_eq!(map["a"][0]["x"].0, "a[0].x");
    assert_eq!(map["a"][0]["y"].0, "a[0].y");
    assert_eq!(map["a"][1]["z"].0, "a[1].z");
    assert_eq!(map["b"][0]["w"].0, "b[0].w");
}

#[test]
fn test_segments() {
    let mut out = None::<BTreeMap<u32, Vec<RecordPath>>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.push_layer(PathLayer::new());
        driver.emit(Event::map_start()).unwrap();
        driver.emit(42u64).unwrap();
        driver.emit(Event::seq_start()).unwrap();
        driver.emit(true).unwrap();
        let path = driver.state().get::<Path>().unwrap();
        assert_eq!(
            path.segments(),
            [PathSegment::Index(42), PathSegment::Index(0)]
        );
        driver.emit(Event::SeqEnd).unwrap();
        driver.emit(Event::MapEnd).unwrap();
        assert!(driver.state().get::<Path>().unwrap().segments().is_empty());
    }
    assert_eq!(out.unwrap()[&42][0].0, "[42][0]");
}

#[test]
fn test_paths_through_buffering() {
    #[derive(deser::Deserialize, Debug)]
    #[deser(tag = "type")]
    enum Tagged {
        Item { values: Vec<RecordPath> },
    }

    let map: BTreeMap<String, Tagged> =
        from_json(r#"{"x": {"values": [1, 2], "type": "Item"}}"#).unwrap();
    let Tagged::Item { values } = &map["x"];
    assert_eq!(values[0].0, "x.values[0]");
    assert_eq!(values[1].0, "x.values[1]");
}

#[derive(deser::Deserialize, Debug)]
#[allow(dead_code)]
struct Server {
    host: String,
    port: u16,
}

#[test]
fn test_error_paths() {
    let err = from_json::<BTreeMap<String, Vec<Server>>>(
        r#"{"servers": [{"host": "a", "port": 1}, {"host": "b", "port": true}]}"#,
    )
    .unwrap_err();
    assert_eq!(err.path(), Some("servers[1].port"));
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected bool, expected u16 at line 1 column 62 (path: servers[1].port)"
    );

    // missing fields are reported for the struct
    let err =
        from_json::<BTreeMap<String, Vec<Server>>>(r#"{"servers": [{"host": "a"}]}"#).unwrap_err();
    assert_eq!(err.path(), Some("servers[0]"));

    // errors at the root have no path
    let err = from_json::<u32>("true").unwrap_err();
    assert_eq!(err.path(), None);

    // syntax errors have no path
    let err = from_json::<Vec<u32>>("[1, 2").unwrap_err();
    assert_eq!(err.path(), None);
}

#[test]
fn test_error_paths_through_buffering() {
    #[derive(deser::Deserialize, Debug)]
    #[deser(tag = "type")]
    #[allow(dead_code)]
    enum Tagged {
        Item { values: Vec<u32> },
    }

    // the tag comes last, so the values are replayed when the error happens
    let err =
        from_json::<BTreeMap<String, Tagged>>(r#"{"x": {"values": [1, "two"], "type": "Item"}}"#)
            .unwrap_err();
    assert_eq!(err.path(), Some("x.values[1]"));
    // the location is the one of the replayed value
    assert_eq!((err.line(), err.column()), (Some(1), Some(22)));
}
