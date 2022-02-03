use deser::ser::{for_each_event, Chunk, MapEmitter, Serializable, SerializerState};
use deser::Error;
use std::collections::BTreeMap;

struct Flags(BTreeMap<u64, bool>);

impl Serializable for Flags {
    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Map(Box::new(FlagsMapEmitter {
            iter: self.0.iter(),
            key: String::new(),
            value: None,
        })))
    }
}

pub struct FlagsMapEmitter<'a> {
    iter: std::collections::btree_map::Iter<'a, u64, bool>,
    key: String,
    value: Option<&'a bool>,
}

impl<'a> MapEmitter for FlagsMapEmitter<'a> {
    fn next_key(&mut self) -> Option<&dyn Serializable> {
        if let Some((key, value)) = self.iter.next() {
            self.key = key.to_string();
            self.value = Some(value);
            Some(&self.key)
        } else {
            None
        }
    }

    fn next_value(&mut self) -> &dyn Serializable {
        self.value.unwrap()
    }
}

#[test]
fn test_as_string_map() {
    let mut events = Vec::new();
    let flags = Flags({
        let mut map = BTreeMap::new();
        map.insert(0, true);
        map.insert(1, true);
        map.insert(2, false);
        map
    });
    for_each_event(&flags, |event, _, _| {
        events.push(format!("{:?}", event));
        Ok(())
    })
    .unwrap();

    assert_eq!(
        events,
        vec![
            "MapStart",
            "Str(\"0\")",
            "Bool(true)",
            "Str(\"1\")",
            "Bool(true)",
            "Str(\"2\")",
            "Bool(false)",
            "MapEnd"
        ]
    );
}
