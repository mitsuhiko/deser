use std::collections::BTreeMap;

use deser::{ErrorKind, Serialize};
use deser_php::{Object, Reference, ReferenceKind, Serializer, Visibility, to_vec};

use crate::common::{Key, Php};

fn ser<T: Serialize + ?Sized>(value: &T) -> String {
    String::from_utf8(to_vec(value).unwrap()).unwrap()
}

#[test]
fn test_scalars() {
    assert_eq!(ser(&()), "N;");
    assert_eq!(ser(&None::<u32>), "N;");
    assert_eq!(ser(&true), "b:1;");
    assert_eq!(ser(&-42i32), "i:-42;");
    assert_eq!(ser(&i64::MIN), "i:-9223372036854775808;");
    assert_eq!(ser(&(i64::MAX as u64)), "i:9223372036854775807;");
    assert_eq!(ser(&1u128), "i:1;");
    assert_eq!(ser(&"a\"b"), "s:3:\"a\"b\";");
    assert_eq!(ser(&'ä'), "s:2:\"ä\";");
    assert_eq!(to_vec(&Php::Bytes(vec![0xff])).unwrap(), b"s:1:\"\xff\";");
}

#[test]
fn test_out_of_range() {
    let err = to_vec(&u64::MAX).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);
    assert!(to_vec(&i128::MIN).is_err());
}

#[test]
fn test_floats() {
    assert_eq!(ser(&0.1f64), "d:0.1;");
    assert_eq!(ser(&1.0f64), "d:1;");
    assert_eq!(ser(&-0.0f64), "d:-0;");
    assert_eq!(ser(&1e25f64), "d:1.0E+25;");
    assert_eq!(ser(&f64::NAN), "d:NAN;");
    assert_eq!(ser(&f64::NEG_INFINITY), "d:-INF;");
    // single precision floats are written with their shortest text
    assert_eq!(ser(&0.1f32), "d:0.1;");
    assert_eq!(ser(&1e-7f32), "d:1.0E-7;");
}

#[test]
fn test_arrays() {
    assert_eq!(ser(&vec![1, 2]), "a:2:{i:0;i:1;i:1;i:2;}");
    assert_eq!(ser(&Vec::<u32>::new()), "a:0:{}");
    assert_eq!(
        ser(&vec![vec![1], vec![]]),
        "a:2:{i:0;a:1:{i:0;i:1;}i:1;a:0:{}}"
    );
    let map = BTreeMap::from([("a", 1), ("b", 2)]);
    assert_eq!(ser(&map), "a:2:{s:1:\"a\";i:1;s:1:\"b\";i:2;}");
}

#[test]
fn test_keys() {
    // keys that are the text of an integer are integers
    let map = BTreeMap::from([("5", 1), ("05", 2), ("-0", 3), ("-7", 4)]);
    assert_eq!(
        ser(&map),
        "a:4:{s:2:\"-0\";i:3;i:-7;i:4;s:2:\"05\";i:2;i:5;i:1;}"
    );
    assert_eq!(ser(&BTreeMap::from([(true, 1)])), "a:1:{i:1;i:1;}");
    assert_eq!(ser(&BTreeMap::from([(-1i64, 1)])), "a:1:{i:-1;i:1;}");
    assert!(to_vec(&BTreeMap::from([(vec![1], 1)])).is_err());
    assert!(to_vec(&BTreeMap::from([(None::<u32>, 1)])).is_err());
}

#[derive(Serialize)]
struct User {
    name: String,
    age: u32,
}

#[test]
fn test_structs() {
    let user = User {
        name: "Jane".into(),
        age: 42,
    };
    assert_eq!(
        ser(&user),
        "a:2:{s:4:\"name\";s:4:\"Jane\";s:3:\"age\";i:42;}"
    );
    assert_eq!(
        ser(&Object::new("App\\User", user)),
        "O:8:\"App\\User\":2:{s:4:\"name\";s:4:\"Jane\";s:3:\"age\";i:42;}"
    );
}

#[test]
fn test_objects() {
    // object properties are strings
    let value = Php::Map(
        Some("Foo".into()),
        vec![(
            Key {
                name: Php::Int(0),
                visibility: None,
            },
            Php::Null,
        )],
    );
    assert_eq!(ser(&value), "O:3:\"Foo\":1:{s:1:\"0\";N;}");
    assert!(to_vec(&Object::new("a-b", BTreeMap::<String, u32>::new())).is_err());
    assert!(to_vec(&Object::new("Foo", vec![1])).is_err());
    assert!(to_vec(&Object::new("Foo", 1)).is_err());
}

#[test]
fn test_visibility() {
    let key = |name: &str, visibility| Key {
        name: Php::Str(name.into()),
        visibility,
    };
    let value = Php::Map(
        Some("Foo".into()),
        vec![
            (key("a", Some(Visibility::Public)), Php::Int(1)),
            (key("b", Some(Visibility::Protected)), Php::Int(2)),
            (
                key("c", Some(Visibility::Private("Foo".into()))),
                Php::Int(3),
            ),
        ],
    );
    let bytes = to_vec(&value).unwrap();
    assert_eq!(
        bytes,
        b"O:3:\"Foo\":3:{s:1:\"a\";i:1;s:4:\"\0*\0b\";i:2;s:6:\"\0Foo\0c\";i:3;}"
    );
    // public properties have no visibility when read
    let Php::Map(class, mut entries) = value else {
        unreachable!()
    };
    entries[0].0.visibility = None;
    assert_eq!(
        deser_php::from_slice::<Php>(&bytes).unwrap(),
        Php::Map(class, entries)
    );
}

#[test]
fn test_enums_and_custom_objects() {
    #[derive(Serialize)]
    enum Suit {
        Hearts,
    }
    assert_eq!(ser(&Suit::Hearts), "s:6:\"Hearts\";");
    assert_eq!(
        ser(&Object::new("Suit", Suit::Hearts)),
        "E:11:\"Suit:Hearts\";"
    );
    assert!(to_vec(&Object::new("Suit", "not a case")).is_err());
    let value = Php::Classed("Foo".into(), Box::new(Php::Bytes(b"{x}".to_vec())));
    assert_eq!(ser(&value), "C:3:\"Foo\":3:{{x}}");
}

#[test]
fn test_references() {
    let value = (
        Object::new("Foo", BTreeMap::<String, u32>::new()),
        Reference::new(ReferenceKind::Object, 2),
        Reference::new(ReferenceKind::Value, 2),
    );
    assert_eq!(ser(&value), "a:3:{i:0;O:3:\"Foo\":0:{}i:1;r:2;i:2;R:2;}");
    assert!(
        to_vec(&BTreeMap::from([(
            Reference::new(ReferenceKind::Value, 1),
            1
        )]))
        .is_err()
    );
}

#[test]
fn test_serializer() {
    let mut serializer = Serializer::new();
    serializer.serialize(&1).unwrap();
    // a failed value does not leave anything behind
    assert!(serializer.serialize(&vec![u64::MAX]).is_err());
    serializer.serialize(&vec!["a"]).unwrap();
    assert_eq!(serializer.output(), b"i:1;a:1:{i:0;s:1:\"a\";}");
    let mut de = deser_php::Deserializer::from_slice(serializer.output());
    assert_eq!(de.deserialize::<u32>().unwrap(), 1);
    assert_eq!(de.deserialize::<Vec<String>>().unwrap(), ["a"]);
    assert!(de.is_end());
}
