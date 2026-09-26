use std::collections::BTreeMap;

use deser::State;
use deser::ser::{Chunk, Serialize, SerializeDriver};
use deser::{Atom, Error, ErrorKind};
use deser_path::{Path, PathLayer};

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

struct MyBool(bool);

impl Serialize for MyBool {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        let path = state.get::<Path>().unwrap();
        assert_eq!(path.segments().len(), 2);
        Ok(Chunk::Atom(Atom::Bool(self.0)))
    }
}

/// Serializes and returns the events with the path the format sees.
fn events(value: &dyn Serialize) -> Vec<String> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(value);
    driver.push_layer(PathLayer::new());
    driver
        .drive(|event, state| {
            events.push(format!(
                "{:?}|{}",
                without_len(event.to_static()),
                state.get::<Path>().map_or(String::new(), Path::to_string)
            ));
            Ok(())
        })
        .unwrap();
    events
}

#[test]
fn test_path() {
    let mut map = BTreeMap::new();
    map.insert("key", vec![MyBool(false), MyBool(true)]);

    assert_eq!(
        events(&map),
        vec![
            "MapStart(ContainerShape { len: None, order: Sorted })|",
            "Atom(Str(\"key\"))|",
            "SeqStart|key",
            "Atom(Bool(false))|key[0]",
            "Atom(Bool(true))|key[1]",
            "SeqEnd|key",
            "MapEnd|"
        ]
    );
}

#[test]
fn test_path_nested_maps() {
    let mut inner = BTreeMap::new();
    inner.insert(1u32, true);
    let mut map = BTreeMap::new();
    map.insert("a", inner.clone());
    map.insert("b", inner);

    let paths: Vec<_> = events(&map)
        .into_iter()
        .filter(|event| event.starts_with("Atom(Bool"))
        .collect();
    assert_eq!(paths, ["Atom(Bool(true))|a[1]", "Atom(Bool(true))|b[1]"]);
}

#[test]
fn test_path_structs() {
    #[derive(deser::Serialize)]
    struct Server {
        host: &'static str,
        ports: Vec<u16>,
    }
    #[derive(deser::Serialize)]
    struct Config {
        servers: Vec<Server>,
    }

    let config = Config {
        servers: vec![Server {
            host: "a",
            ports: vec![80],
        }],
    };
    assert_eq!(
        events(&config),
        vec![
            "MapStart|",
            "Atom(Str(\"servers\"))|",
            "SeqStart|servers",
            "MapStart|servers[0]",
            "Atom(Str(\"host\"))|servers[0]",
            "Atom(Str(\"a\"))|servers[0].host",
            "Atom(Str(\"ports\"))|servers[0]",
            "SeqStart|servers[0].ports",
            "Atom(U64(80))|servers[0].ports[0]",
            "SeqEnd|servers[0].ports",
            "MapEnd|servers[0]",
            "SeqEnd|servers",
            "MapEnd|",
        ]
    );
}

#[test]
fn test_error_path() {
    struct Fails;

    impl Serialize for Fails {
        fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
            Err(Error::new(ErrorKind::Unexpected, "cannot serialize"))
        }
    }

    let mut map = BTreeMap::new();
    map.insert("items", vec![None, Some(Fails)]);
    let mut driver = SerializeDriver::new(&map);
    driver.push_layer(PathLayer::new());
    let err = driver.drive(|_, _| Ok(())).unwrap_err();
    assert_eq!(err.path(), Some("items[1]"));
    assert_eq!(
        err.to_string(),
        "Unexpected: cannot serialize (path: items[1])"
    );
}
