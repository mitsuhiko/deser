use std::collections::BTreeMap;

use deser::ser::{for_each_event, Chunk, Serialize, SerializerState};
use deser::Error;
use deser_path::{Path, PathSerializable};

struct MyBool(bool);

impl Serialize for MyBool {
    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
        let path = state.get::<Path>();
        assert_eq!(path.segments().len(), 2);
        Ok(Chunk::Bool(self.0))
    }
}

#[test]
fn test_path() {
    let mut events = Vec::new();
    let mut map = BTreeMap::new();
    map.insert("key", vec![MyBool(false), MyBool(true)]);

    for_each_event(&PathSerializable::wrap(&map), |event, _, state| {
        events.push(format!("{:?}|{:?}", event, state.get::<Path>().segments()));
        Ok(())
    })
    .unwrap();

    dbg!(&events);

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
