use std::collections::BTreeMap;

use deser::de::{Deserialize, DeserializeDriver, DeserializerState, Sink, SinkHandle};
use deser::{Atom, Error, Event};
use deser_path::{Path, PathSink};

#[derive(Debug, PartialEq, Eq)]
struct MyBool(bool);

deser::make_slot_wrapper!(SlotWrapper);

impl Deserialize for MyBool {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        SlotWrapper::make_handle(out)
    }
}

impl Sink for SlotWrapper<MyBool> {
    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Bool(value) => {
                let path = state.get::<Path>();
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
        let sink = PathSink::wrap_ref(Deserialize::deserialize_into(&mut out));
        let mut driver = DeserializeDriver::from_sink(SinkHandle::boxed(sink));
        driver.emit(Event::MapStart).unwrap();
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

impl Deserialize for RecordPath {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        SlotWrapper::make_handle(out)
    }
}

impl Sink for SlotWrapper<RecordPath> {
    fn atom(&mut self, _atom: Atom, state: &DeserializerState) -> Result<(), Error> {
        let path = state.get::<Path>();
        **self = Some(RecordPath(format!("{:?}", path.segments())));
        Ok(())
    }
}

#[test]
fn test_nested_paths() {
    let mut out = None::<BTreeMap<String, Vec<BTreeMap<String, RecordPath>>>>;
    {
        let sink = PathSink::wrap_ref(Deserialize::deserialize_into(&mut out));
        let mut driver = DeserializeDriver::from_sink(SinkHandle::boxed(sink));
        deser_json::Deserializer::new(r#"{"a": [{"x": 1, "y": 2}, {"z": 3}], "b": [{"w": 4}]}"#)
            .drive(&mut driver)
            .unwrap();
    }

    let map = out.unwrap();
    assert_eq!(map["a"][0]["x"].0, r#"[Key("a"), Index(0), Key("x")]"#);
    assert_eq!(map["a"][0]["y"].0, r#"[Key("a"), Index(0), Key("y")]"#);
    assert_eq!(map["a"][1]["z"].0, r#"[Key("a"), Index(1), Key("z")]"#);
    assert_eq!(map["b"][0]["w"].0, r#"[Key("b"), Index(0), Key("w")]"#);
}
