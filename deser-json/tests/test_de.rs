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
