use deser::ser::{for_each_event, Chunk, MapEmitter, Serialize, SerializeHandle, SerializerState};
use deser::Error;
use std::collections::{btree_map, BTreeMap};

struct Flags(BTreeMap<u64, bool>);

impl Serialize for Flags {
    fn serialize(&self, _state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Map(Box::new(FlagsMapEmitter {
            iter: self.0.iter(),
            value: None,
        })))
    }
}

pub struct FlagsMapEmitter<'a> {
    iter: btree_map::Iter<'a, u64, bool>,
    value: Option<&'a bool>,
}

impl<'a> MapEmitter for FlagsMapEmitter<'a> {
    fn next_key(&mut self) -> Option<SerializeHandle> {
        if let Some((key, value)) = self.iter.next() {
            self.value = Some(value);
            Some(SerializeHandle::boxed(key.to_string()))
        } else {
            None
        }
    }

    fn next_value(&mut self) -> SerializeHandle {
        SerializeHandle::to(self.value.unwrap())
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
            "Atom(Str(\"0\"))",
            "Atom(Bool(true))",
            "Atom(Str(\"1\"))",
            "Atom(Bool(true))",
            "Atom(Str(\"2\"))",
            "Atom(Bool(false))",
            "MapEnd"
        ]
    );
}
