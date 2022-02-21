use std::collections::BTreeMap;

use deser::ser::{Chunk, Serialize, SerializeDriver, SerializerState};
use deser::{Atom, Error};
use deser_path::{Path, PathSerializable};

struct MyBool(bool);

impl Serialize for MyBool {
    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
        let path = state.extensions().get_or_default::<Path>();
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
            state.extensions().get_or_default::<Path>().segments()
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
