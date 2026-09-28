use std::collections::BTreeMap;

use deser::State;
use deser::de::{Deserialize, DeserializeDriver, Sink, SinkHandle};
use deser::{Atom, Error, Event};
use deser_path::{Path, PathLayer, PathSegment};

#[derive(Debug, PartialEq, Eq)]
struct MyBool(bool);

deser::make_slot_wrapper!(SlotWrapper);

impl<'de> Deserialize<'de> for MyBool {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        _state: &mut State,
    ) -> SinkHandle<'out, 'de> {
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
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        _state: &mut State,
    ) -> SinkHandle<'out, 'de> {
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
    assert_eq!(
        err.attachment::<Path>().unwrap().to_string(),
        "servers[1].port"
    );
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected bool, expected u16 at line 1 column 62 (path: servers[1].port)"
    );

    // missing fields are reported for the struct
    let err =
        from_json::<BTreeMap<String, Vec<Server>>>(r#"{"servers": [{"host": "a"}]}"#).unwrap_err();
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "servers[0]");

    // errors at the root have no path
    let err = from_json::<u32>("true").unwrap_err();
    assert!(err.attachment::<Path>().is_none());

    // syntax errors have no path
    let err = from_json::<Vec<u32>>("[1, 2").unwrap_err();
    assert!(err.attachment::<Path>().is_none());
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
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "x.values[1]");
    // the location is the one of the replayed value
    assert_eq!((err.line(), err.column()), (Some(1), Some(22)));
}

#[test]
fn test_unknown_field_paths() {
    use deser::de::{IgnoredFields, UnknownFields};
    use deser_json::DeserializerConfig;

    #[derive(deser::Deserialize, Debug)]
    #[deser(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Strict {
        host: String,
    }

    // the error points at the key
    let err = from_json::<BTreeMap<String, Vec<Strict>>>(
        r#"{"servers": [{"host": "a"}, {"host": "b", "prot": 1}]}"#,
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unknown field `prot`, expected `host` at line 1 column 43 (path: servers[1].prot)"
    );

    // collected keys carry the same information
    let ignored = IgnoredFields::new();
    let json = "{\"servers\": [\n  {\"host\": \"a\", \"port\": 1, \"tiemout\": 2}\n]}";
    deser_json::Deserializer::from_str_with_config(
        json,
        &DeserializerConfig::new().track_locations(true),
    )
    .deserialize_with::<BTreeMap<String, Vec<Server>>, _>(|driver| {
        UnknownFields::Collect(ignored.clone()).set(driver.state_mut());
        driver.push_layer(PathLayer::new())
    })
    .unwrap();
    let ignored = ignored.take();
    assert_eq!(ignored.len(), 1);
    assert_eq!(
        ignored[0].to_string(),
        "Unexpected: unknown field `tiemout`, expected `host` or `port` at line 2 column 28 (path: servers[0].tiemout)"
    );

    // keys of flattened internally tagged enums before the tag are located
    // at their value
    #[derive(deser::Deserialize, Debug)]
    #[deser(tag = "type")]
    #[allow(dead_code)]
    enum Kind {
        A { a: u32 },
    }

    #[derive(deser::Deserialize, Debug)]
    #[deser(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Holder {
        #[deser(flatten)]
        kind: Kind,
    }

    let err = from_json::<Vec<Holder>>(r#"[{"x": 1, "a": 2, "type": "A"}]"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unknown field `x` at line 1 column 8 (path: [0].x)"
    );
}

#[test]
fn test_error_paths_after_recovery() {
    use deser::adapters::{DefaultOnError, VecSkipError};

    #[derive(deser::Deserialize, Debug)]
    #[allow(dead_code)]
    struct Item {
        id: u32,
        tags: Vec<String>,
    }

    #[derive(deser::Deserialize, Debug)]
    #[allow(dead_code)]
    struct Doc {
        #[deser(as = VecSkipError)]
        items: Vec<Item>,
        #[deser(as = DefaultOnError)]
        extra: Option<Item>,
        rest: Vec<u32>,
    }

    // the failed values are skipped, the paths of the values after them are
    // still correct
    let err = from_json::<Doc>(
        r#"{
            "items": [{"id": 1, "tags": [1, [2]]}, {"id": 2, "tags": ["a"]}],
            "extra": {"id": {"x": [1, 2]}, "tags": []},
            "rest": [1, 2, "x"]
        }"#,
    )
    .unwrap_err();
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "rest[2]");

    // errors the sinks recover from have the path attached
    struct Paths(Vec<String>);

    struct PathsSink<'a>(&'a mut Option<Paths>, Vec<String>);

    impl<'de> Deserialize<'de> for Paths {
        fn deserialize_into<'out>(
            out: &'out mut Option<Self>,
            state: &mut State,
        ) -> SinkHandle<'out, 'de> {
            SinkHandle::arena(PathsSink(out, Vec::new()), state)
        }
    }

    impl<'de> Sink<'de> for PathsSink<'_> {
        fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
            Ok(())
        }

        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            Ok(SinkHandle::arena(Nothing, state))
        }

        fn recover(&mut self, err: Error, _state: &mut State) -> Result<(), Error> {
            self.1.push(err.attachment::<Path>().unwrap().to_string());
            Ok(())
        }

        fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
            *self.0 = Some(Paths(std::mem::take(&mut self.1)));
            Ok(())
        }
    }

    // accepts empty maps and fails on everything else
    struct Nothing;

    impl<'de> Sink<'de> for Nothing {
        fn map(&mut self, _state: &mut State) -> Result<(), Error> {
            Ok(())
        }

        fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            Ok(SinkHandle::arena(Nothing, state))
        }
    }

    let paths = from_json::<Paths>(r#"[{}, 1, {"a": [1]}, {"b": {"c": 1}}, {}, [{}]]"#).unwrap();
    assert_eq!(paths.0, ["[1]", "[2].a", "[3].b", "[5]"]);
}

#[test]
fn test_paths_of_collected_errors() {
    #[derive(deser::Deserialize, Debug)]
    #[allow(dead_code)]
    struct Server {
        host: String,
        port: u16,
    }

    #[derive(deser::Deserialize, Debug)]
    #[allow(dead_code)]
    struct Config {
        servers: Vec<Server>,
        names: BTreeMap<String, u32>,
    }

    let err = deser_json::Deserializer::from_str(
        r#"{
            "servers": [{"host": 1, "port": 1}, {"host": "b"}],
            "names": {"a": 1, "b": "x"}
        }"#,
    )
    .deserialize_with::<Config, _>(|driver| {
        driver.push_layer(PathLayer::new());
        driver.state_mut().set_collect_errors(true);
    })
    .unwrap_err();
    let errors = err
        .errors()
        .map(|err| format!("{}: {}", err.attachment::<Path>().unwrap(), err.message()))
        .collect::<Vec<_>>();
    assert_eq!(
        errors,
        [
            "servers[0].host: unexpected unsigned integer, expected string",
            "servers[1]: missing field `port`",
            "names.b: unexpected string, expected u32",
        ]
    );
}
