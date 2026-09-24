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
