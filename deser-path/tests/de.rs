use std::collections::BTreeMap;

use deser::de::{Deserializable, DeserializerState, Driver, Sink, SinkHandle};
use deser::{Error, Event};
use deser_path::{Path, PathSink};

#[derive(Debug, PartialEq, Eq)]
struct MyBool(bool);

deser::make_slot_wrapper!(SlotWrapper);

impl Deserializable for MyBool {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
        SinkHandle::Borrowed(SlotWrapper::wrap(out))
    }
}

impl Sink for SlotWrapper<MyBool> {
    fn bool(&mut self, value: bool, state: &DeserializerState) -> Result<(), Error> {
        let path = state.get::<Path>();
        assert_eq!(path.segments().len(), 1);
        **self = Some(MyBool(value));
        Ok(())
    }
}

#[test]
fn test_path() {
    let mut out = None::<BTreeMap<String, MyBool>>;

    {
        let sink = PathSink::wrap_ref(Deserializable::deserialize_into(&mut out));
        let mut driver = Driver::from_sink(SinkHandle::Owned(Box::new(sink)));
        driver.emit(&Event::MapStart).unwrap();
        driver.emit(&Event::Str("foo".into())).unwrap();
        driver.emit(&Event::Bool(true)).unwrap();
        driver.emit(&Event::Str("bar".into())).unwrap();
        driver.emit(&Event::Bool(false)).unwrap();
        driver.emit(&Event::MapEnd).unwrap();
    }

    let map = out.unwrap();

    assert_eq!(map["foo"], MyBool(true));
    assert_eq!(map["bar"], MyBool(false));
}
