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

    let outer: Outer = from_str(
        r#"{"inner": {"a": 1}, "list": [1, 2], "boxed": {"a": 2}, "null": null}"#,
    )
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
