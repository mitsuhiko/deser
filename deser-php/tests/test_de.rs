use std::collections::{BTreeMap, HashMap};

use deser::{Deserialize, ErrorKind};
use deser_php::{Object, Reference, ReferenceKind, Visibility, from_slice};

use crate::common::{Key, Php};

fn php(input: &[u8]) -> Php {
    from_slice(input).unwrap()
}

fn key(name: &str) -> Key {
    Key {
        name: Php::Str(name.into()),
        visibility: None,
    }
}

#[test]
fn test_scalars() {
    assert_eq!(php(b"N;"), Php::Null);
    assert_eq!(php(b"b:1;"), Php::Bool(true));
    assert_eq!(php(b"i:-42;"), Php::Int(-42));
    assert_eq!(php(b"i:+007;"), Php::Int(7));
    assert_eq!(php(b"d:.5;"), Php::Float(0.5));
    assert_eq!(php(b"d:-INF;"), Php::Float(f64::NEG_INFINITY));
    assert_eq!(php(b"s:5:\"a\"b;c\";"), Php::Str("a\"b;c".into()));
    assert_eq!(php(b"S:3:\"a\\62c\";"), Php::Str("abc".into()));
    assert_eq!(php(b"s:2:\"\xff\xfe\";"), Php::Bytes(vec![0xff, 0xfe]));
    assert_eq!(from_slice::<u64>(b"i:18;").unwrap(), 18);
    assert_eq!(from_slice::<f32>(b"d:0.5;").unwrap(), 0.5);
}

#[test]
fn test_borrowed_strings() {
    let input = b"a:1:{i:0;s:5:\"hello\";}".to_vec();
    let value: Vec<&str> = from_slice(&input).unwrap();
    assert_eq!(value, ["hello"]);
}

#[test]
fn test_bytes() {
    // strings are bytes, types that expect bytes take them as they are
    assert_eq!(from_slice::<Vec<u8>>(b"s:3:\"abc\";").unwrap(), b"abc");
    assert_eq!(
        from_slice::<Vec<u8>>(b"s:2:\"\xff\x00\";").unwrap(),
        b"\xff\x00"
    );
}

#[test]
fn test_lists() {
    assert_eq!(
        php(b"a:2:{i:0;s:1:\"a\";i:1;s:1:\"b\";}"),
        Php::List(vec![Php::Str("a".into()), Php::Str("b".into())])
    );
    // strings that are integers are integers
    assert_eq!(
        php(b"a:2:{s:1:\"0\";i:1;i:1;i:2;}"),
        Php::List(vec![Php::Int(1), Php::Int(2)])
    );
    // empty arrays are lists, and maps for types that expect maps
    assert_eq!(php(b"a:0:{}"), Php::List(vec![]));
    assert_eq!(
        from_slice::<Vec<u32>>(b"a:0:{}").unwrap(),
        Vec::<u32>::new()
    );
    assert!(
        from_slice::<BTreeMap<String, u32>>(b"a:0:{}")
            .unwrap()
            .is_empty()
    );
    #[derive(Debug, PartialEq, Deserialize)]
    struct Settings {
        #[deser(default)]
        theme: Option<String>,
    }
    assert_eq!(
        from_slice::<Settings>(b"a:0:{}").unwrap(),
        Settings { theme: None }
    );
    // anything else is a map
    for input in [
        &b"a:2:{i:1;i:1;i:0;i:2;}"[..],
        b"a:2:{i:0;i:1;i:2;i:2;}",
        b"a:1:{i:1;i:1;}",
        b"a:2:{i:0;i:1;s:1:\"a\";i:2;}",
        b"a:1:{s:2:\"00\";i:1;}",
    ] {
        assert!(matches!(php(input), Php::Map(None, _)), "{:?}", input);
    }
}

#[test]
fn test_map_keys() {
    let input = b"a:3:{i:5;s:1:\"a\";s:1:\"6\";s:1:\"b\";s:2:\"07\";s:1:\"c\";}";
    // integer keys are their text for maps with string keys
    let value: BTreeMap<String, String> = from_slice(input).unwrap();
    assert_eq!(
        value.keys().map(String::as_str).collect::<Vec<_>>(),
        ["07", "5", "6"]
    );
    // and their value for maps with integer keys
    let value: HashMap<u32, String> =
        from_slice(b"a:2:{i:5;s:1:\"a\";s:1:\"6\";s:1:\"b\";}").unwrap();
    assert_eq!(value[&5], "a");
    assert_eq!(value[&6], "b");
    // integers are text as PHP writes them
    let value: BTreeMap<String, String> = from_slice(b"a:1:{i:+05;s:1:\"a\";}").unwrap();
    assert_eq!(value["5"], "a");
    // keys that are not the canonical text of an integer stay strings
    assert!(from_slice::<HashMap<u32, String>>(input).is_err());
}

#[derive(Debug, PartialEq, Deserialize)]
struct User {
    name: String,
    #[deser(default)]
    age: u32,
}

#[test]
fn test_objects() {
    let input = br#"O:4:"User":2:{s:4:"name";s:4:"Jane";s:3:"age";i:42;}"#;
    let user: User = from_slice(input).unwrap();
    assert_eq!(
        user,
        User {
            name: "Jane".into(),
            age: 42
        }
    );
    let user: Object<User> = from_slice(input).unwrap();
    assert_eq!(user.class.as_deref(), Some("User"));
    // arrays have no class
    let user: Object<User> = from_slice(br#"a:1:{s:4:"name";s:4:"Jane";}"#).unwrap();
    assert_eq!(user.class, None);
    assert_eq!(
        php(br#"O:7:"Foo\Bar":0:{}"#),
        Php::Map(Some("Foo\\Bar".into()), vec![])
    );
}

#[test]
fn test_visibility() {
    // the prefix of protected and private properties is removed
    let input = b"O:4:\"User\":2:{s:7:\"\0*\0name\";s:4:\"Jane\";s:9:\"\0User\0age\";i:1;}";
    let user: User = from_slice(input).unwrap();
    assert_eq!(
        user,
        User {
            name: "Jane".into(),
            age: 1
        }
    );
    let Php::Map(_, entries) = php(input) else {
        panic!("not an object");
    };
    let keys: Vec<_> = entries.into_iter().map(|(key, _)| key).collect();
    assert_eq!(
        keys,
        [
            Key {
                visibility: Some(Visibility::Protected),
                ..key("name")
            },
            Key {
                visibility: Some(Visibility::Private("User".into())),
                ..key("age")
            },
        ]
    );
    // arrays keep the prefix
    let Php::Map(_, entries) = php(b"a:1:{s:4:\"\0*\0x\";N;}") else {
        panic!("not an array");
    };
    assert_eq!(entries[0].0, key("\0*\0x"));
}

#[derive(Debug, PartialEq, Deserialize)]
enum Suit {
    Hearts,
    Spades,
}

#[test]
fn test_enums() {
    let value: Vec<Suit> =
        from_slice(br#"a:2:{i:0;E:11:"Suit:Hearts";i:1;E:11:"Suit:Spades";}"#).unwrap();
    assert_eq!(value, [Suit::Hearts, Suit::Spades]);
    let value: Object<Suit> = from_slice(br#"E:11:"Suit:Hearts";"#).unwrap();
    assert_eq!(value.class.as_deref(), Some("Suit"));
    assert_eq!(
        php(br#"E:15:"App\Suit:Hearts";"#),
        Php::Classed("App\\Suit".into(), Box::new(Php::Str("Hearts".into())))
    );
}

#[test]
fn test_custom_objects() {
    assert_eq!(
        php(br#"C:11:"ArrayObject":5:{x:i:0}"#),
        Php::Classed(
            "ArrayObject".into(),
            Box::new(Php::Bytes(b"x:i:0".to_vec()))
        )
    );
}

#[test]
fn test_references() {
    let value = php(b"a:2:{i:0;O:8:\"stdClass\":0:{}i:1;r:2;}");
    assert_eq!(
        value,
        Php::List(vec![
            Php::Map(Some("stdClass".into()), vec![]),
            Php::Ref(Reference::new(ReferenceKind::Object, 2)),
        ])
    );
    let value: (Php, Reference) = from_slice(b"a:2:{i:0;i:5;i:1;R:2;}").unwrap();
    assert_eq!(value.1.kind(), ReferenceKind::Value);
    assert_eq!(value.1.number(), 2);
    // the fallback is the number
    assert_eq!(
        from_slice::<Vec<u32>>(b"a:2:{i:0;i:5;i:1;R:2;}").unwrap(),
        [5, 2]
    );
    // references must refer to earlier values, `r:` to objects
    for input in [
        &b"r:1;"[..],
        b"a:1:{i:0;r:1;}",
        b"a:1:{i:0;R:2;}",
        b"a:2:{i:0;i:5;i:1;r:2;}",
        b"a:1:{i:0;R:0;}",
    ] {
        assert!(from_slice::<Php>(input).is_err(), "{:?}", input);
    }
    // keys and `R:` have no number, `r:` has one
    assert!(from_slice::<Php>(b"a:3:{i:0;i:1;i:1;R:2;i:2;R:3;}").is_err());
    assert!(from_slice::<Php>(b"a:3:{i:0;O:1:\"A\":0:{}i:1;r:2;i:2;r:3;}").is_ok());
}

#[test]
fn test_errors() {
    let err = from_slice::<Php>(b"a:1:{i:0;i:1;").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
    let err = from_slice::<Php>(b"N;x").unwrap_err();
    assert_eq!(err.message(), "syntax error: trailing data after value");
    assert_eq!(err.offset(), Some(2));
    let err = from_slice::<Php>(b"a:1:{i:0;b:2;}").unwrap_err();
    assert_eq!(err.message(), "syntax error: invalid boolean");
    assert_eq!(err.offset(), Some(11));
    let err = from_slice::<Php>(b"i:9223372036854775808;").unwrap_err();
    assert_eq!(err.message(), "syntax error: integer out of range");
    let err = from_slice::<Php>(b"O:3:\"\xe4bc\":0:{}").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);
    // the value is validated before it's deserialized
    let err = from_slice::<Vec<u32>>(b"a:2:{i:0;s:1:\"a\";i:1;x}").unwrap_err();
    assert_eq!(err.message(), "syntax error: unknown type");
}

#[test]
fn test_deep_nesting() {
    let depth = 100_000;
    let mut input = "a:1:{i:0;".repeat(depth);
    input.push_str("N;");
    input.push_str(&"}".repeat(depth));
    let mut de = deser_php::Deserializer::from_slice(input.as_bytes());
    let mut driver = deser::de::DeserializeDriver::from_fn(|_| deser::de::SinkHandle::null());
    de.drive(&mut driver).unwrap();
    assert!(de.is_end());
}

#[test]
fn test_input_ranges() {
    use deser::State;
    use deser::de::{DeserializeDriver, Sink, SinkHandle};

    struct Ranges<'a>(&'a mut Vec<(usize, usize)>);

    impl<'de> Sink<'de> for Ranges<'_> {
        fn atom(&mut self, _atom: deser::Atom, state: &mut State) -> Result<(), deser::Error> {
            let range = state.input_range().unwrap();
            self.0.push((range.start, range.end));
            Ok(())
        }
    }

    let mut ranges = Vec::new();
    let input = b"s:2:\"ab\";";
    let mut de = deser_php::Deserializer::from_slice(input);
    let mut driver =
        DeserializeDriver::from_fn(|state| SinkHandle::arena(Ranges(&mut ranges), state));
    de.drive(&mut driver).unwrap();
    drop(driver);
    assert_eq!(ranges, [(0, 9)]);
}
