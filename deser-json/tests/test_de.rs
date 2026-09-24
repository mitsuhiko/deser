use deser::Deserialize;
use deser_json::from_str;

#[test]
fn test_basic() {
    let x: Vec<u32> = from_str(r#"[1, 2, 3, 4]"#).unwrap();
    assert_eq!(x, vec![1, 2, 3, 4]);
}

#[test]
fn test_flatten() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct User {
        id: u64,
        #[deser(flatten)]
        attrs: Attrs,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Attrs {
        is_active: bool,
        is_admin: bool,
        flags: Vec<String>,
    }

    let user: User = from_str(
        r#"
        {
            "id": 42,
            "is_active": true,
            "is_admin": true,
            "flags": ["german", "staff"]
        }
    "#,
    )
    .unwrap();

    assert_eq!(
        user,
        User {
            id: 42,
            attrs: Attrs {
                is_active: true,
                is_admin: true,
                flags: vec!["german".into(), "staff".into()],
            }
        }
    )
}

#[test]
fn test_optional_compounds() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Inner {
        a: u32,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Outer {
        inner: Option<Inner>,
        list: Option<Vec<u32>>,
        boxed: Option<Box<Inner>>,
        missing: Option<Inner>,
        null: Option<Inner>,
    }

    let outer: Outer =
        from_str(r#"{"inner": {"a": 1}, "list": [1, 2], "boxed": {"a": 2}, "null": null}"#)
            .unwrap();
    assert_eq!(
        outer,
        Outer {
            inner: Some(Inner { a: 1 }),
            list: Some(vec![1, 2]),
            boxed: Some(Box::new(Inner { a: 2 })),
            missing: None,
            null: None,
        }
    );
}

#[test]
fn test_flatten_optional() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct User {
        id: u64,
        #[deser(flatten)]
        attrs: Option<Attrs>,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Attrs {
        is_admin: bool,
    }

    let user: User = from_str(r#"{"id": 42, "is_admin": true}"#).unwrap();
    assert_eq!(
        user,
        User {
            id: 42,
            attrs: Some(Attrs { is_admin: true }),
        }
    );
}

#[test]
fn test_maps() {
    use std::collections::{BTreeMap, HashMap};

    let map: HashMap<String, u32> = from_str(r#"{"a": 1, "b": 2}"#).unwrap();
    assert_eq!(map.len(), 2);
    assert_eq!(map["a"], 1);
    assert_eq!(map["b"], 2);

    let map: BTreeMap<String, u32> = from_str(r#"{"a": 1, "b": 2}"#).unwrap();
    assert_eq!(
        map.into_iter().collect::<Vec<_>>(),
        vec![("a".into(), 1), ("b".into(), 2)]
    );
}

#[test]
fn test_generics() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Wrapper<T> {
        value: T,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Newtype<T>(T);

    let w: Wrapper<Vec<u32>> = from_str(r#"{"value": [1, 2]}"#).unwrap();
    assert_eq!(w, Wrapper { value: vec![1, 2] });

    let n: Newtype<u32> = from_str(r#"42"#).unwrap();
    assert_eq!(n, Newtype(42));
}

#[test]
fn test_numeric_keys() {
    use std::collections::{BTreeMap, HashMap};

    let map: HashMap<u32, u32> = from_str(r#"{"42": 23}"#).unwrap();
    assert_eq!(map[&42], 23);

    let map: BTreeMap<i64, bool> = from_str(r#"{"-1": true, "2": false}"#).unwrap();
    assert_eq!(
        map.into_iter().collect::<Vec<_>>(),
        vec![(-1, true), (2, false)]
    );

    // strings are only coerced in key position
    assert!(from_str::<u32>(r#""42""#).is_err());
    assert!(from_str::<Vec<u32>>(r#"["42"]"#).is_err());
    assert!(from_str::<HashMap<u32, u32>>(r#"{"x": 1}"#).is_err());
}

#[test]
fn test_char() {
    let c: char = from_str(r#""x""#).unwrap();
    assert_eq!(c, 'x');
}

#[test]
fn test_unknown_keys() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Simple {
        a: u32,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct WithFlatten {
        a: u32,
        #[deser(flatten)]
        simple: Inner,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Inner {
        #[deser(alias = "bee")]
        b: u32,
    }

    let s: Simple = from_str(r#"{"x": {"y": [1, 2]}, "a": 1, "z": null}"#).unwrap();
    assert_eq!(s, Simple { a: 1 });

    let s: WithFlatten = from_str(r#"{"x": [1], "bee": 2, "a": 1, "z": null}"#).unwrap();
    assert_eq!(
        s,
        WithFlatten {
            a: 1,
            simple: Inner { b: 2 }
        }
    );
}

#[test]
fn test_strings() {
    let s: String = from_str(r#""hello world, this is a longer string""#).unwrap();
    assert_eq!(s, "hello world, this is a longer string");
    let s: String = from_str(r#""a longer string with \"escapes\" and \u00e9 and \\""#).unwrap();
    assert_eq!(s, "a longer string with \"escapes\" and \u{e9} and \\");
    let s: String = from_str("\"日本語のテキストもちゃんと動く\"").unwrap();
    assert_eq!(s, "日本語のテキストもちゃんと動く");
    assert!(from_str::<String>("\"control \x01 character\"").is_err());
    assert!(from_str::<String>("\"unterminated string").is_err());
}

#[test]
fn test_syntax_errors() {
    use std::collections::BTreeMap;

    for json in [
        "[1,]", "[,1]", "]", "[1 2]", "[1]]", "[1] x", "", "[", "[1", "[1,", "[}",
    ] {
        assert!(from_str::<Vec<u32>>(json).is_err(), "accepted {:?}", json);
    }
    for json in [
        r#"{"a":1,}"#,
        r#"{"a" 1}"#,
        r#"{1: 2}"#,
        r#"{"a":"#,
        r#"{"a"}"#,
        r#"{,"a":1}"#,
        r#"{"a":1"b":2}"#,
        r#"{"a":1]"#,
    ] {
        assert!(
            from_str::<BTreeMap<String, u32>>(json).is_err(),
            "accepted {:?}",
            json
        );
    }

    let map: BTreeMap<String, Vec<u32>> = from_str(r#" { "a" : [ ] , "b" : [ 1 , 2 ] } "#).unwrap();
    assert_eq!(map["a"], Vec::<u32>::new());
    assert_eq!(map["b"], vec![1, 2]);
}

#[test]
fn test_wide_integers() {
    use std::collections::BTreeMap;

    assert_eq!(from_str::<u128>(&u128::MAX.to_string()).unwrap(), u128::MAX);
    assert_eq!(from_str::<i128>(&i128::MIN.to_string()).unwrap(), i128::MIN);
    assert_eq!(
        from_str::<u128>("18446744073709551616").unwrap(),
        1u128 << 64
    );
    // just below i64::MIN
    assert_eq!(
        from_str::<i128>("-9223372036854775809").unwrap(),
        i64::MIN as i128 - 1
    );
    assert_eq!(
        from_str::<i128>("-18446744073709551616").unwrap(),
        -(1i128 << 64)
    );
    assert_eq!(from_str::<u128>("42").unwrap(), 42);
    assert_eq!(from_str::<i128>("-42").unwrap(), -42);

    // wide integers still work for floats
    assert_eq!(
        from_str::<f64>("18446744073709551616").unwrap(),
        18446744073709551616.0
    );
    assert_eq!(
        from_str::<f64>("-9223372036854775809").unwrap(),
        -9223372036854775809.0
    );
    // and floats stay floats
    assert_eq!(
        from_str::<f64>("18446744073709551616.5").unwrap(),
        18446744073709551616.5
    );
    assert_eq!(
        from_str::<f64>("18446744073709551616e2").unwrap(),
        1844674407370955161600.0
    );
    // too large for 128 bits
    let huge = format!("{}0", u128::MAX);
    assert_eq!(from_str::<f64>(&huge).unwrap(), u128::MAX as f64 * 10.0);
    assert!(from_str::<u128>(&huge).is_err());

    // out of range for narrower types
    assert!(from_str::<u64>("18446744073709551616").is_err());

    let map: BTreeMap<u128, bool> = from_str(&format!(r#"{{"{}": true}}"#, u128::MAX)).unwrap();
    assert!(map[&u128::MAX]);

    // roundtrip
    let values = vec![u128::MAX, 0, 1 << 100];
    assert_eq!(
        from_str::<Vec<u128>>(&deser_json::to_string(&values).unwrap()).unwrap(),
        values
    );
}

#[test]
fn test_internally_tagged_buffering() {
    use std::collections::HashMap;

    #[derive(Deserialize, PartialEq, Debug)]
    #[deser(tag = "type")]
    enum Message {
        Stats {
            // integer keys only work because the buffered keys are replayed
            // as map keys.
            counts: HashMap<u32, u32>,
            // extension values are retained when buffered
            total: u128,
            label: Option<String>,
        },
    }

    let msg: Message = from_str(
        r#"{
            "counts": {"1": 10, "2": 20},
            "total": 340282366920938463463374607431768211455,
            "label": null,
            "type": "Stats"
        }"#,
    )
    .unwrap();
    let mut counts = HashMap::new();
    counts.insert(1, 10);
    counts.insert(2, 20);
    assert_eq!(
        msg,
        Message::Stats {
            counts,
            total: u128::MAX,
            label: None,
        }
    );
}

#[test]
fn test_enum_representations() {
    use std::collections::BTreeMap;

    #[derive(Deserialize, PartialEq, Debug)]
    #[deser(tag = "t", content = "c")]
    enum Adjacent {
        Counts(BTreeMap<u32, u32>),
    }

    // content is buffered as it comes before the tag
    let value: Adjacent = from_str(r#"{"c": {"1": 2}, "t": "Counts"}"#).unwrap();
    let mut counts = BTreeMap::new();
    counts.insert(1, 2);
    assert_eq!(value, Adjacent::Counts(counts));

    #[derive(Deserialize, PartialEq, Debug)]
    #[deser(untagged)]
    enum Value {
        Big(u128),
        Text(String),
        List(Vec<Value>),
    }

    let value: Value = from_str(r#"[340282366920938463463374607431768211455, "x", []]"#).unwrap();
    assert_eq!(
        value,
        Value::List(vec![
            Value::Big(u128::MAX),
            Value::Text("x".into()),
            Value::List(vec![])
        ])
    );

    #[derive(Deserialize, PartialEq, Debug)]
    enum External {
        Point(i32, i32),
        Name { first: String },
    }
    let value: Vec<External> =
        from_str(r#"[{"Point": [1, -2]}, {"Name": {"first": "x"}}]"#).unwrap();
    assert_eq!(
        value,
        vec![External::Point(1, -2), External::Name { first: "x".into() }]
    );
}
