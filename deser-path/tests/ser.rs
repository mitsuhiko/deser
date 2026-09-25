use std::collections::BTreeMap;

use deser::ser::{Chunk, Serialize, SerializeDriver};
use deser::State;
use deser::{Atom, Error};
use deser_path::{Path, PathSerializable};

struct MyBool(bool);

impl Serialize for MyBool {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        let path = state.get::<Path>().unwrap();
        assert_eq!(path.segments().len(), 2);
        Ok(Chunk::Atom(Atom::Bool(self.0)))
    }
}

#[test]
fn test_path() {
    let mut events = Vec::new();
    let mut map = BTreeMap::new();
    map.insert("key", vec![MyBool(false), MyBool(true)]);

    let serializable = PathSerializable::wrap(&map);
    let mut driver = SerializeDriver::new(&serializable);
    while let Some((event, _, state)) = driver.next().unwrap() {
        events.push(format!(
            "{:?}|{:?}",
            event,
            state.get::<Path>().map_or(&[][..], Path::segments)
        ));
    }

    assert_eq!(
        events,
        vec![
            "MapStart|[]",
            "Atom(Str(\"key\"))|[]",
            "SeqStart|[Key(\"key\")]",
            "Atom(Bool(false))|[Key(\"key\"), Index(0)]",
            "Atom(Bool(true))|[Key(\"key\"), Index(1)]",
            "SeqEnd|[Key(\"key\")]",
            "MapEnd|[]"
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

    let serializable = PathSerializable::wrap(&map);
    let mut driver = SerializeDriver::new(&serializable);
    let mut paths = Vec::new();
    while let Some((event, _, state)) = driver.next().unwrap() {
        if let deser::Event::Atom(Atom::Bool(_)) = event {
            paths.push(format!("{:?}", state.get::<Path>().unwrap().segments()));
        }
    }
    assert_eq!(
        paths,
        vec![
            "[Key(\"a\"), Index(1)]".to_string(),
            "[Key(\"b\"), Index(1)]".to_string(),
        ]
    );
}
