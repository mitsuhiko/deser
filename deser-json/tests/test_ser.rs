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
fn test_extension_fallback() {
    // until JSON knows about wide integers natively they use the fallback
    assert_eq!(to_string(&42u128).unwrap(), "42");
    assert_eq!(to_string(&-42i128).unwrap(), "-42");
}
