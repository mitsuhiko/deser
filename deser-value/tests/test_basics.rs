use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use deser::ext::{ExtValue, Uuid};
use deser::{Deserialize, Order};
use deser_value::{Kind, Map, Seq, Value, from_value, to_value, value};

fn hash(value: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn test_size() {
    assert_eq!(std::mem::size_of::<Kind>(), 40);
    assert_eq!(std::mem::size_of::<Value>(), 48);
}

#[test]
fn test_macro() {
    let id = 42;
    let value = value!({
        "id": id,
        "name": "Jane",
        "tags": ["a", null, true, false, -1, 1.5, [], {}],
        "nested": {"deep": {"deeper": [1, [2, [3]]]}},
        1: "one",
        -2: "minus two",
        (id + 1): "expression",
        'c': 'd',
        true: null,
    });
    let map = value.as_map().unwrap();
    assert_eq!(map.len(), 9);
    assert_eq!(value["id"], 42);
    assert_eq!(value["name"], "Jane");
    assert_eq!(value["tags"][4], -1);
    assert_eq!(value["tags"][5], 1.5);
    assert!(value["tags"][1].is_null());
    assert_eq!(value["nested"]["deep"]["deeper"][1][1][0], 3);
    assert_eq!(map.get(&1), Some(&value!("one")));
    assert_eq!(map.get(&-2), Some(&value!("minus two")));
    assert_eq!(map.get(&43u64), Some(&value!("expression")));
    assert_eq!(map.get(&'c'), Some(&value!('d')));
    assert_eq!(map.get(&true), Some(&value!(null)));

    // order of insertion is retained
    let keys: Vec<_> = map.keys().cloned().collect();
    assert_eq!(
        keys,
        [
            value!("id"),
            value!("name"),
            value!("tags"),
            value!("nested"),
            value!(1),
            value!(-2),
            value!(43),
            value!('c'),
            value!(true),
        ]
    );

    assert_eq!(value!([]), Value::from(Seq::new()));
    assert_eq!(value!({}), Value::from(Map::new()));
    assert_eq!(value!([1, 2,]), value!([1, 2]));
    assert_eq!(value!({"a": 1,}), value!({"a": 1}));
    assert_eq!(value!(Some(1)), value!(1));
    assert_eq!(value!(None::<u32>), value!(null));
}

#[test]
fn test_numbers() {
    // non-negative integers are held as u64
    assert!(matches!(*value!(1i64), Kind::U64(1)));
    assert!(matches!(*value!(-1i64), Kind::I64(-1)));

    // integers compare and hash by value
    let signed = Value::new(Kind::I64(5));
    assert_eq!(signed, value!(5));
    assert_eq!(hash(&signed), hash(&value!(5)));
    let map = value!({5: "five"});
    assert_eq!(map[&signed], "five");

    // u128 and i128 which fit into 64 bits are regular integers
    assert!(matches!(*to_value(&5u128).unwrap(), Kind::U64(5)));
    assert!(matches!(*to_value(&-5i128).unwrap(), Kind::I64(-5)));
    assert_eq!(value!(5u128), value!(5));
    let big = to_value(&u128::MAX).unwrap();
    assert!(big.downcast_ext::<u128>().is_some());
    assert_eq!(big.as_u128(), Some(u128::MAX));
    assert_eq!(big.as_u64(), None);
    assert_eq!(from_value::<u128>(&big).unwrap(), u128::MAX);
    assert_eq!(value!(i128::MIN).as_i128(), Some(i128::MIN));

    // floats compare by bits
    assert_eq!(value!(f64::NAN), value!(f64::NAN));
    assert_ne!(value!(0.0), value!(-0.0));
    assert_ne!(value!(1.0), value!(1));

    // single precision floats keep their precision but compare and hash
    // like the same value as f64
    let single = value!(0.1f32);
    assert!(matches!(*single, Kind::F32(value) if value == 0.1));
    assert!(matches!(*to_value(&0.1f32).unwrap(), Kind::F32(_)));
    assert_eq!(single, value!(f64::from(0.1f32)));
    assert_ne!(single, value!(0.1f64));
    assert_eq!(hash(&single), hash(&value!(f64::from(0.1f32))));
    assert_eq!(single, 0.1f32);
    assert_eq!(single, f64::from(0.1f32));
    assert_eq!(single.as_f64(), Some(f64::from(0.1f32)));
    assert_eq!(from_value::<f32>(&single).unwrap(), 0.1);
    assert_eq!(from_value::<f64>(&single).unwrap(), f64::from(0.1f32));
    assert_eq!(format!("{:?}", single), "0.1");
    assert_eq!(single.name(), "float");

    assert_eq!(value!(1).as_f64(), Some(1.0));
    assert_eq!(value!(-1).as_u64(), None);
    assert_eq!(value!(-1).as_i64(), Some(-1));
}

#[test]
fn test_extensions() {
    let uuid = Uuid([1; 16]);
    let value = Value::ext(uuid);
    assert_eq!(value.downcast_ext::<Uuid>(), Some(&uuid));
    // accessors look through the fallback
    assert_eq!(
        value.as_str(),
        None,
        "the fallback of uuids is an owned string"
    );
    assert_eq!(value.name(), "uuid");
    assert_eq!(value, Value::ext(uuid));
    assert_ne!(value, Value::ext(Uuid([2; 16])));
    assert_ne!(value, value!(uuid.to_string()));
    assert_eq!(hash(&value), hash(&Value::ext(uuid)));

    // round trips through the extension
    let back: Uuid = from_value(&value).unwrap();
    assert_eq!(back, uuid);
    assert_eq!(to_value(&uuid).unwrap().downcast_ext::<Uuid>(), Some(&uuid));

    assert_eq!(Value::from(ExtValue::owned(7u128)), value!(7));
}

#[test]
fn test_maps() {
    let mut map = Map::new();
    assert_eq!(map.insert("b", 1), None);
    assert_eq!(map.insert("a", 2), None);
    assert_eq!(map.insert("b", 3), Some(value!(1)));
    assert_eq!(map.len(), 2);
    // replacing retains the position
    assert_eq!(
        map.iter().collect::<Vec<_>>(),
        [(&value!("b"), &value!(3)), (&value!("a"), &value!(2))]
    );
    assert!(map.contains_key("a"));
    assert!(map.contains_key(&String::from("a")));
    assert!(map.contains_key(&value!("a")));
    assert!(!map.contains_key(&1));
    *map.get_mut("a").unwrap() = value!(4);
    assert_eq!(map.get_index(1), Some((&value!("a"), &value!(4))));

    map.insert(1, 1);
    assert_eq!(map.remove("b"), Some(value!(3)));
    // removing retains the order
    assert_eq!(map.keys().collect::<Vec<_>>(), [&value!("a"), &value!(1)]);

    map.sort_by(|a, _, b, _| a.as_i64().cmp(&b.as_i64()));
    assert_eq!(map.keys().collect::<Vec<_>>(), [&value!("a"), &value!(1)]);

    map.retain(|key, _| key.is_str());
    assert_eq!(map.len(), 1);

    // maps compare without order
    assert_eq!(value!({"a": 1, "b": 2}), value!({"b": 2, "a": 1}));
    assert_ne!(value!({"a": 1, "b": 2}), value!({"a": 1}));
    assert_ne!(value!({"a": 1}), value!({"a": 2}));
    assert_eq!(
        hash(&value!({"a": 1, "b": 2})),
        hash(&value!({"b": 2, "a": 1}))
    );

    // but sequences with order
    assert_ne!(value!([1, 2]), value!([2, 1]));

    // containers as keys
    let map = value!({[1, 2]: "seq", {"a": 1}: "map", null: "null"});
    assert_eq!(map[&value!([1, 2])], "seq");
    assert_eq!(map[&value!({"a": 1})], "map");
    assert_eq!(map[&value!(null)], "null");

    let collected: Value = [("a", 1), ("b", 2)].into_iter().collect();
    assert_eq!(collected, value!({"a": 1, "b": 2}));
    let owned: Vec<(Value, Value)> = collected
        .into_kind()
        .as_map()
        .unwrap()
        .clone()
        .into_iter()
        .collect();
    assert_eq!(owned.len(), 2);
}

#[test]
fn test_index_mut() {
    let mut value = Value::null();
    value["a"]["b"] = value!(1);
    value["a"]["c"] = value!([1, 2]);
    value["a"]["c"][1] = value!("two");
    assert_eq!(value, value!({"a": {"b": 1, "c": [1, "two"]}}));

    let mut value = value!([1]);
    let rv = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        value[1] = value!(2);
    }));
    assert!(rv.is_err());
}

#[test]
fn test_debug() {
    let value = value!({
        "a": [1, -2, 1.5, null, true],
        "b": {"c": 'x', 1: []},
        "e": {},
    });
    assert_eq!(
        format!("{:?}", value),
        r#"{"a": [1, -2, 1.5, null, true], "b": {"c": 'x', 1: []}, "e": {}}"#
    );
    assert_eq!(
        format!("{:#?}", value),
        r#"{
    "a": [
        1,
        -2,
        1.5,
        null,
        true,
    ],
    "b": {
        "c": 'x',
        1: [],
    },
    "e": {},
}"#
    );
    assert_eq!(format!("{:?}", Value::bytes(*b"a\0")), r#"b"a\x00""#);
    assert_eq!(
        format!("{:?}", value.as_map().unwrap()),
        format!("{:?}", value)
    );
    assert_eq!(
        format!("{:#?}", value["a"].as_seq().unwrap()),
        format!("{:#?}", value["a"])
    );
    assert_eq!(format!("{:?}", Seq::new()), "[]");
}

#[test]
fn test_order() {
    let value = to_value(&BTreeMap::from([(2, "b"), (1, "a")])).unwrap();
    assert_eq!(value.as_map().unwrap().order(), Order::Sorted);
    let value = to_value(&HashMap::from([(1, "a")])).unwrap();
    assert_eq!(value.as_map().unwrap().order(), Order::Arbitrary);
    let value = to_value(&std::collections::BTreeSet::from([1, 2])).unwrap();
    assert_eq!(value.as_seq().unwrap().order(), Order::Sorted);
    let value = to_value(&vec![1, 2]).unwrap();
    assert_eq!(value.as_seq().unwrap().order(), Order::Natural);

    // the order is retained when values are cloned
    let value = to_value(&BTreeMap::from([(2, vec![1])])).unwrap();
    assert_eq!(value.clone().as_map().unwrap().order(), Order::Sorted);

    // and serialized
    let mut events = Vec::new();
    deser::ser::SerializeDriver::new(&value)
        .drive(|event, _| {
            events.push(event.to_static());
            Ok(())
        })
        .unwrap();
    assert!(
        matches!(events[0], deser::Event::MapStart(shape) if shape.order() == Order::Sorted && shape.len() == Some(1))
    );
}

#[test]
fn test_conversions() {
    #[derive(Debug, PartialEq, deser::Serialize, Deserialize)]
    struct User {
        id: u64,
        name: String,
        roles: Vec<String>,
        manager: Option<Box<User>>,
    }

    let user = User {
        id: 1,
        name: "Jane".into(),
        roles: vec!["admin".into()],
        manager: None,
    };
    let value = to_value(&user).unwrap();
    assert_eq!(
        value,
        value!({"id": 1, "name": "Jane", "roles": ["admin"], "manager": null})
    );
    assert_eq!(from_value::<User>(&value).unwrap(), user);

    // borrowing
    #[derive(Deserialize)]
    struct Borrowed<'a> {
        name: &'a str,
    }
    let borrowed: Borrowed = from_value(&value).unwrap();
    assert!(std::ptr::eq(borrowed.name, value["name"].as_str().unwrap()));

    // keys of JSON are lexical and parse as numbers, strings do not
    let value: Value = deser_json::from_str(r#"{"42": "answer"}"#).unwrap();
    let map: HashMap<u32, String> = from_value(&value).unwrap();
    assert_eq!(map[&42], "answer");
    assert!(from_value::<HashMap<u32, String>>(&value!({"42": "answer"})).is_err());
    let map: HashMap<u32, String> = from_value(&value!({42: "answer"})).unwrap();
    assert_eq!(map[&42], "answer");

    // maps and sequences
    let map: Map = from_value(&value!({"a": 1})).unwrap();
    assert_eq!(map.get("a"), Some(&value!(1)));
    let seq: Seq = from_value(&value!([1, 2])).unwrap();
    assert_eq!(seq.len(), 2);
    assert_eq!(
        from_value::<Map>(&value!([1])).unwrap_err().to_string(),
        "Unexpected: unexpected sequence, expected map"
    );
    assert_eq!(
        from_value::<Seq>(&value!("x")).unwrap_err().to_string(),
        "Unexpected: unexpected string, expected sequence"
    );

    // values as fields
    #[derive(Debug, Deserialize, deser::Serialize)]
    struct Envelope {
        kind: String,
        payload: Value,
    }
    let envelope: Envelope =
        deser_json::from_str(r#"{"kind": "x", "payload": {"a": [1, 2]}}"#).unwrap();
    assert_eq!(envelope.payload, value!({"a": [1, 2]}));
    assert_eq!(
        deser_json::to_string(&envelope).unwrap(),
        r#"{"kind":"x","payload":{"a":[1,2]}}"#
    );

    // recordings
    let recording: deser::de::Recording = deser_json::from_str(r#"[1, {"a": 2}]"#).unwrap();
    assert_eq!(to_value(&recording).unwrap(), value!([1, {"a": 2}]));
}

#[test]
fn test_duplicate_keys() {
    let err = deser_json::from_str::<Value>(r#"{"a": 1, "b": 2, "a": 3}"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        r#"Unexpected: duplicate map key "a" at line 1 column 23"#
    );
}

#[test]
fn test_meta() {
    let mut value = value!(1);
    assert!(value.meta().is_none());
    value.meta_mut().event_data_mut().insert(42u32);
    assert_eq!(value.event_data().unwrap().get::<u32>(), Some(&42));
    // meta data is ignored for comparisons
    assert_eq!(value, value!(1));
    let cloned = value.clone();
    assert_eq!(cloned.event_data().unwrap().get::<u32>(), Some(&42));
    let meta = value.take_meta().unwrap();
    assert!(value.meta().is_none());
    // empty meta data is not retained
    value.set_meta(Some(deser_value::Meta::new()));
    assert!(value.meta().is_none());
    let value = value.with_meta(meta);
    assert!(value.meta().is_some());
}

#[test]
fn test_container_keys_compare_in_order() {
    assert_eq!(
        value!({"a": 1, [1]: 2, "b": 3, [2]: 4}),
        value!({[1]: 2, "b": 3, [2]: 4, "a": 1})
    );
    assert_ne!(
        value!({"a": 1, [1]: 2, [2]: 4}),
        value!({"a": 1, [2]: 4, [1]: 2})
    );
    assert_ne!(value!({[1]: 2, "a": 1}), value!({"b": 2, "a": 1}));
}

#[test]
fn test_serializer() {
    use deser::ser::{Chunk, Serialize};
    use deser::{Error, ErrorKind, State};
    use deser_value::Serializer;

    struct Failing;

    impl Serialize for Failing {
        fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
            Err(Error::new(ErrorKind::Unexpected, "failed"))
        }
    }

    let mut serializer = Serializer::new();
    serializer.serialize(&1u32).unwrap();
    // a value that fails is not added
    assert!(serializer.serialize(&vec![Failing]).is_err());
    serializer.serialize(&"x").unwrap();
    assert_eq!(serializer.values(), [value!(1), value!("x")]);
}
