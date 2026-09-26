use deser::Serialize;
use deser_json::to_string;

#[test]
fn test_basic() {
    assert_eq!(to_string(&[1, 2, 3, 4]).unwrap(), "[1,2,3,4]");
}

#[test]
fn test_flatten() {
    #[derive(Serialize, PartialEq, Eq, Debug)]
    pub struct User {
        id: u64,
        #[deser(flatten)]
        attrs: Attrs,
    }

    #[derive(Serialize, PartialEq, Eq, Debug)]
    pub struct Attrs {
        is_active: bool,
        is_admin: bool,
        flags: Vec<String>,
    }

    let json = to_string(&User {
        id: 42,
        attrs: Attrs {
            is_active: true,
            is_admin: true,
            flags: vec!["german".into(), "staff".into()],
        },
    })
    .unwrap();
    assert_eq!(
        json,
        r#"{"id":42,"is_active":true,"is_admin":true,"flags":["german","staff"]}"#
    );
}

#[test]
fn test_char() {
    assert_eq!(to_string(&'a').unwrap(), r#""a""#);
    assert_eq!(to_string(&'"').unwrap(), r#""\"""#);
}

#[test]
fn test_map_keys() {
    use std::collections::BTreeMap;

    let mut map = BTreeMap::new();
    map.insert(42u32, 23u32);
    map.insert(1, 2);
    assert_eq!(to_string(&map).unwrap(), r#"{"1":2,"42":23}"#);

    let mut map = BTreeMap::new();
    map.insert(-1i32, true);
    assert_eq!(to_string(&map).unwrap(), r#"{"-1":true}"#);

    let mut map = BTreeMap::new();
    map.insert('x', 1u32);
    assert_eq!(to_string(&map).unwrap(), r#"{"x":1}"#);
}

#[test]
fn test_generics() {
    #[derive(Serialize)]
    pub struct Wrapper<T> {
        value: T,
    }

    #[derive(Serialize)]
    pub struct Newtype<T>(T);

    assert_eq!(
        to_string(&Wrapper { value: vec![1u32] }).unwrap(),
        r#"{"value":[1]}"#
    );
    assert_eq!(to_string(&Newtype(42u32)).unwrap(), "42");
}

#[test]
fn test_string_escapes() {
    assert_eq!(to_string(&"").unwrap(), r#""""#);
    assert_eq!(to_string(&"plain").unwrap(), r#""plain""#);
    assert_eq!(
        to_string(&"a \"quoted\" \\ string\nwith\tcontrol \x01 chars").unwrap(),
        r#""a \"quoted\" \\ string\nwith\tcontrol \u0001 chars""#
    );
    assert_eq!(to_string(&"\"").unwrap(), r#""\"""#);
    assert_eq!(
        to_string(&"日本語のテキスト\u{1f600}").unwrap(),
        "\"日本語のテキスト\u{1f600}\""
    );
}

#[test]
fn test_key_escapes() {
    let mut map = std::collections::BTreeMap::new();
    map.insert("plain", 1);
    map.insert("a \"quoted\" key", 2);
    map.insert("\n", 3);
    map.insert("日本語", 4);
    assert_eq!(
        to_string(&map).unwrap(),
        r#"{"\n":3,"a \"quoted\" key":2,"plain":1,"日本語":4}"#
    );
}

#[test]
fn test_string_lengths() {
    // strings of all lengths around the copy thresholds, with and without
    // escapes at the end.
    for len in 0..70 {
        let s: String = (0..len).map(|x| (b'a' + (x % 26) as u8) as char).collect();
        assert_eq!(to_string(&s).unwrap(), format!("\"{}\"", s));
        let escaped = format!("{}\n", s);
        assert_eq!(to_string(&escaped).unwrap(), format!("\"{}\\n\"", s));
        let values = vec![s.clone(), escaped.clone(), s.clone()];
        assert_eq!(
            to_string(&values).unwrap(),
            format!("[\"{}\",\"{}\\n\",\"{}\"]", s, s, s)
        );
    }
}

#[test]
fn test_nested_containers() {
    use std::collections::BTreeMap;

    assert_eq!(
        to_string(&vec![vec![1u32, 2], vec![], vec![3]]).unwrap(),
        "[[1,2],[],[3]]"
    );

    let mut inner = BTreeMap::new();
    inner.insert("x", vec![1u32]);
    inner.insert("y", vec![]);
    let mut map = BTreeMap::new();
    map.insert("a", inner.clone());
    map.insert("b", BTreeMap::new());
    map.insert("c", inner);
    assert_eq!(
        to_string(&vec![map]).unwrap(),
        r#"[{"a":{"x":[1],"y":[]},"b":{},"c":{"x":[1],"y":[]}}]"#
    );
}

#[test]
fn test_wide_integers() {
    assert_eq!(to_string(&42u128).unwrap(), "42");
    assert_eq!(to_string(&-42i128).unwrap(), "-42");
    assert_eq!(to_string(&u128::MAX).unwrap(), u128::MAX.to_string());
    assert_eq!(to_string(&i128::MIN).unwrap(), i128::MIN.to_string());

    let mut map = std::collections::BTreeMap::new();
    map.insert(u128::MAX, 1u32);
    assert_eq!(
        to_string(&map).unwrap(),
        format!(r#"{{"{}":1}}"#, u128::MAX)
    );
}

#[test]
fn test_well_known_types() {
    use deser::ext::{BigInt, Datetime, Decimal, Duration, Timestamp, Uuid};
    use std::collections::BTreeMap;

    // decimals and big integers are written as numbers
    let decimal: Decimal = "-12.50".parse().unwrap();
    assert_eq!(to_string(&decimal).unwrap(), "-12.50");
    let big: BigInt = "123456789012345678901234567890123456789012"
        .parse()
        .unwrap();
    assert_eq!(
        to_string(&big).unwrap(),
        "123456789012345678901234567890123456789012"
    );
    let mut map = BTreeMap::new();
    map.insert(big.clone(), decimal.clone());
    assert_eq!(
        to_string(&map).unwrap(),
        r#"{"123456789012345678901234567890123456789012":-12.50}"#
    );
    // numbers are passed on exactly
    let value: Decimal = deser_json::from_str("-12.50").unwrap();
    assert_eq!(value, decimal);
    let value: Decimal = deser_json::from_str("\"-12.50\"").unwrap();
    assert_eq!(value, decimal);
    // unless disabled
    let value: Decimal = deser_json::DeserializerConfig::new()
        .exact_numbers(false)
        .from_str("-12.50")
        .unwrap();
    assert_eq!(value.as_str(), "-12.5");
    let value: BigInt =
        deser_json::from_str("\"123456789012345678901234567890123456789012\"").unwrap();
    assert_eq!(value, big);

    // the others are written as strings
    let datetime: Datetime = "1979-05-27T07:32:00Z".parse().unwrap();
    assert_eq!(to_string(&datetime).unwrap(), r#""1979-05-27T07:32:00Z""#);
    let timestamp = Timestamp {
        seconds: 0,
        nanosecond: 0,
    };
    assert_eq!(to_string(&timestamp).unwrap(), r#""1970-01-01T00:00:00Z""#);
    let duration = Duration {
        seconds: 90,
        nanosecond: 0,
    };
    assert_eq!(to_string(&duration).unwrap(), r#""PT1M30S""#);
    let uuid: Uuid = "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap();
    assert_eq!(
        to_string(&uuid).unwrap(),
        r#""67e55044-10b1-426f-9247-bb680e5fe0c8""#
    );
    let value: Uuid = deser_json::from_str(r#""67e55044-10b1-426f-9247-bb680e5fe0c8""#).unwrap();
    assert_eq!(value, uuid);
}

#[test]
fn test_extension_fallback() {
    use deser::State;
    use deser::ext::{ExtValue, Extension};
    use deser::ser::Chunk;
    use deser::{Atom, Error};

    #[derive(Debug, Clone, PartialEq)]
    struct Timestamp(i64);

    impl Extension for Timestamp {
        fn name(&self) -> &str {
            "timestamp"
        }

        fn fallback(&self) -> Atom<'_> {
            Atom::I64(self.0)
        }
    }

    impl Serialize for Timestamp {
        fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
            Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(self))))
        }
    }

    assert_eq!(to_string(&vec![Timestamp(-1)]).unwrap(), "[-1]");
}

#[test]
fn test_float_precision() {
    assert_eq!(to_string(&0.1f32).unwrap(), "0.1");
    assert_eq!(to_string(&0.1f64).unwrap(), "0.1");
    assert_eq!(to_string(&Some(0.1f32)).unwrap(), "0.1");
    assert_eq!(to_string(&vec![0.087f32]).unwrap(), "[0.087]");
    assert_eq!(to_string(&Box::new(0.5f32)).unwrap(), "0.5");
    assert_eq!(to_string(&f32::NAN).unwrap(), "null");

    #[derive(Serialize)]
    struct Metadata {
        completed_in: f32,
        optional: Option<f32>,
    }
    assert_eq!(
        to_string(&Metadata {
            completed_in: 0.087,
            optional: Some(1.1),
        })
        .unwrap(),
        r#"{"completed_in":0.087,"optional":1.1}"#
    );
}
