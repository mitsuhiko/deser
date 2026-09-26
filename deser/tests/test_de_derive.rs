use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::{Deserialize, Event};

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> T {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in events {
            driver.emit(event).unwrap();
        }
    }
    out.unwrap()
}

#[test]
fn test_container_defaults() {
    #[derive(Deserialize, Default)]
    #[deser(default)]
    pub struct MyContainer {
        field1: usize,
        field2: bool,
    }

    let s: MyContainer = deserialize(vec![Event::map_start(), Event::MapEnd]);
    assert_eq!(s.field1, 0);
    assert!(!s.field2);

    let s: MyContainer = deserialize(vec![
        Event::map_start(),
        "field1".into(),
        1usize.into(),
        "field2".into(),
        false.into(),
        Event::MapEnd,
    ]);
    assert_eq!(s.field1, 1);
    assert!(!s.field2);
}

#[test]
fn test_field_defaults() {
    #[derive(Deserialize)]
    pub struct MyContainer {
        #[deser(default)]
        field1: usize,
        #[deser(default)]
        field2: bool,
    }

    let s: MyContainer = deserialize(vec![Event::map_start(), Event::MapEnd]);
    assert_eq!(s.field1, 0);
    assert!(!s.field2);

    let s: MyContainer = deserialize(vec![
        Event::map_start(),
        "field1".into(),
        1usize.into(),
        "field2".into(),
        false.into(),
        Event::MapEnd,
    ]);
    assert_eq!(s.field1, 1);
    assert!(!s.field2);
}

#[test]
fn test_option_defaults() {
    #[derive(Deserialize)]
    pub struct MyContainer {
        val: Option<String>,
    }

    let s: MyContainer = deserialize(vec![Event::map_start(), Event::MapEnd]);
    assert_eq!(s.val, None);

    let s: MyContainer = deserialize(vec![
        Event::map_start(),
        "val".into(),
        "foo".into(),
        Event::MapEnd,
    ]);
    assert_eq!(s.val, Some("foo".into()));

    fn other_default() -> Option<String> {
        Some("aha!".into())
    }

    #[derive(Deserialize)]
    pub struct MyOtherContainer {
        #[deser(default = other_default())]
        val: Option<String>,
    }

    let s: MyOtherContainer = deserialize(vec![Event::map_start(), Event::MapEnd]);
    assert_eq!(s.val, Some("aha!".into()));
}

#[test]
fn test_nested_option_defaults() {
    #[derive(Deserialize)]
    pub struct MyContainer {
        first: Option<Option<bool>>,
        second: Option<Option<bool>>,
        third: Option<Option<bool>>,
    }

    let s: MyContainer = deserialize(vec![
        Event::map_start(),
        "first".into(),
        true.into(),
        "second".into(),
        ().into(),
        Event::MapEnd,
    ]);
    assert_eq!(s.first, Some(Some(true)));
    assert_eq!(s.second, Some(None));
    assert_eq!(s.third, None);
}

#[test]
fn test_container_and_field_defaults() {
    #[derive(Deserialize)]
    #[deser(default)]
    pub struct MyContainer {
        #[deser(default)]
        field1: usize,
        field2: bool,
    }

    impl Default for MyContainer {
        fn default() -> Self {
            Self {
                field1: 42,
                field2: true,
            }
        }
    }

    let s: MyContainer = deserialize(vec![Event::map_start(), Event::MapEnd]);
    assert_eq!(s.field1, 0);
    assert!(s.field2);

    let s: MyContainer = deserialize(vec![
        Event::map_start(),
        "field1".into(),
        1usize.into(),
        "field2".into(),
        false.into(),
        Event::MapEnd,
    ]);
    assert_eq!(s.field1, 1);
    assert!(!s.field2);
}

#[test]
#[should_panic(expected = "Missing field 'field1'")]
fn test_container_no_defaults() {
    #[derive(Deserialize)]
    pub struct MyContainer {
        #[allow(unused)]
        field1: usize,
        #[allow(unused)]
        field2: bool,
    }

    let _: MyContainer = deserialize(vec![Event::map_start(), Event::MapEnd]);
}

#[test]
fn test_container_explicit_defaults() {
    fn default_it() -> MyContainer {
        MyContainer {
            field1: 1,
            field2: 2,
        }
    }

    #[derive(Deserialize)]
    #[deser(default = default_it())]
    pub struct MyContainer {
        field1: usize,
        field2: usize,
    }

    let s: MyContainer = deserialize(vec![Event::map_start(), Event::MapEnd]);
    assert_eq!(s.field1, 1);
    assert_eq!(s.field2, 2);
}

#[test]
fn test_field_explicit_default() {
    fn default_field_1() -> usize {
        1
    }

    fn default_field_2() -> usize {
        2
    }

    #[derive(Deserialize)]
    pub struct MyContainer {
        #[deser(default = default_field_1())]
        field1: usize,
        #[deser(default = default_field_2())]
        field2: usize,
    }

    let s: MyContainer = deserialize(vec![Event::map_start(), Event::MapEnd]);
    assert_eq!(s.field1, 1);
    assert_eq!(s.field2, 2);
}

#[test]
fn test_field_expression_defaults() {
    use std::collections::BTreeMap;

    fn make_tags(n: usize) -> Vec<String> {
        (0..n).map(|x| x.to_string()).collect()
    }

    const PORT: u16 = 8080;

    #[derive(Deserialize, Debug, PartialEq)]
    pub struct Name(String);

    impl From<&str> for Name {
        fn from(value: &str) -> Name {
            Name(value.to_uppercase())
        }
    }

    #[derive(Deserialize)]
    pub struct MyContainer {
        #[deser(default = 42)]
        int: u16,
        #[deser(default = -1.5)]
        float: f64,
        #[deser(default = PORT + 1)]
        port: u16,
        #[deser(default = "localhost")]
        string: String,
        #[deser(default = "name")]
        name: Name,
        #[deser(default = Some("x".to_string()))]
        opt: Option<String>,
        #[deser(default = Vec::new())]
        vec: Vec<u32>,
        #[deser(default = BTreeMap::<String, u32>::new())]
        map: BTreeMap<String, u32>,
        #[deser(default = make_tags(2))]
        tags: Vec<String>,
        #[deser(default = <bool as Default>::default())]
        flag: bool,
    }

    let s: MyContainer = deserialize(vec![Event::map_start(), Event::MapEnd]);
    assert_eq!(s.int, 42);
    assert_eq!(s.float, -1.5);
    assert_eq!(s.port, 8081);
    assert_eq!(s.string, "localhost");
    assert_eq!(s.name, Name("NAME".into()));
    assert_eq!(s.opt.as_deref(), Some("x"));
    assert!(s.vec.is_empty());
    assert!(s.map.is_empty());
    assert_eq!(s.tags, vec!["0".to_string(), "1".to_string()]);
    assert!(!s.flag);

    // defaults are only used for missing values
    let s: MyContainer = deserialize(vec![
        Event::map_start(),
        "int".into(),
        1u64.into(),
        "string".into(),
        "remote".into(),
        Event::MapEnd,
    ]);
    assert_eq!(s.int, 1);
    assert_eq!(s.string, "remote");
}

#[test]
fn test_defaults_are_lazy() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CALLS: AtomicUsize = AtomicUsize::new(0);

    fn counted() -> usize {
        CALLS.fetch_add(1, Ordering::SeqCst)
    }

    #[derive(Deserialize)]
    pub struct MyContainer {
        #[deser(default = counted())]
        field: usize,
    }

    let s: MyContainer = deserialize(vec![
        Event::map_start(),
        "field".into(),
        42u64.into(),
        Event::MapEnd,
    ]);
    assert_eq!(s.field, 42);
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    let s: MyContainer = deserialize(vec![Event::map_start(), Event::MapEnd]);
    assert_eq!(s.field, 0);
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}

#[test]
fn test_container_expression_default() {
    #[derive(Deserialize)]
    #[deser(default = MyContainer::make("x"))]
    pub struct MyContainer {
        name: String,
        #[deser(default = 7)]
        field: usize,
    }

    impl MyContainer {
        fn make(name: &str) -> MyContainer {
            MyContainer {
                name: name.into(),
                field: 1,
            }
        }
    }

    let s: MyContainer = deserialize(vec![Event::map_start(), Event::MapEnd]);
    assert_eq!(s.name, "x");
    assert_eq!(s.field, 7);
}

#[test]
fn test_generic_expression_default() {
    #[derive(Deserialize)]
    pub struct MyContainer<T> {
        #[deser(default = Vec::new())]
        items: Vec<T>,
        #[deser(default = "n/a")]
        name: String,
    }

    let s: MyContainer<u32> = deserialize(vec![Event::map_start(), Event::MapEnd]);
    assert!(s.items.is_empty());
    assert_eq!(s.name, "n/a");
}

#[test]
fn test_rename_all_camel_case() {
    #[derive(Deserialize)]
    #[deser(rename_all = "camelCase")]
    struct Test {
        foo_bar_baz: bool,
    }

    let s: Test = deserialize(vec![
        Event::map_start(),
        "fooBarBaz".into(),
        true.into(),
        "dummy".into(),
        42u64.into(),
        Event::MapEnd,
    ]);
    assert!(s.foo_bar_baz);
}

#[test]
fn test_rename_all_snake_case() {
    #[derive(Deserialize)]
    #[deser(rename_all = "snake_case")]
    struct Test {
        foo_bar_baz: bool,
    }

    let s: Test = deserialize(vec![
        Event::map_start(),
        "foo_bar_baz".into(),
        true.into(),
        "dummy".into(),
        42u64.into(),
        Event::MapEnd,
    ]);
    assert!(s.foo_bar_baz);
}

#[test]
fn test_rename_all_lowercase() {
    #[derive(Deserialize)]
    #[deser(rename_all = "lowercase")]
    struct Test {
        foo_bar_baz: bool,
    }

    let s: Test = deserialize(vec![
        Event::map_start(),
        "foo_bar_baz".into(),
        true.into(),
        "dummy".into(),
        42u64.into(),
        Event::MapEnd,
    ]);
    assert!(s.foo_bar_baz);
}

#[test]
fn test_rename_all_pascal_case() {
    #[derive(Deserialize)]
    #[deser(rename_all = "PascalCase")]
    struct Test {
        foo_bar_baz: bool,
    }

    let s: Test = deserialize(vec![
        Event::map_start(),
        "FooBarBaz".into(),
        true.into(),
        "dummy".into(),
        42u64.into(),
        Event::MapEnd,
    ]);
    assert!(s.foo_bar_baz);
}

#[test]
fn test_rename_all_kebab_case() {
    #[derive(Deserialize)]
    #[deser(rename_all = "kebab-case")]
    struct Test {
        foo_bar_baz: bool,
    }

    let s: Test = deserialize(vec![
        Event::map_start(),
        "foo-bar-baz".into(),
        true.into(),
        "dummy".into(),
        42u64.into(),
        Event::MapEnd,
    ]);
    assert!(s.foo_bar_baz);
}

#[test]
fn test_rename_all_uppercase() {
    #[derive(Deserialize)]
    #[deser(rename_all = "UPPERCASE")]
    struct Test {
        foo_bar_baz: bool,
    }

    let s: Test = deserialize(vec![
        Event::map_start(),
        "FOO_BAR_BAZ".into(),
        true.into(),
        "dummy".into(),
        42u64.into(),
        Event::MapEnd,
    ]);
    assert!(s.foo_bar_baz);
}

#[test]
fn test_rename_all_screaming_snake_case() {
    #[derive(Deserialize)]
    #[deser(rename_all = "SCREAMING_SNAKE_CASE")]
    struct Test {
        foo_bar_baz: bool,
    }

    let s: Test = deserialize(vec![
        Event::map_start(),
        "FOO_BAR_BAZ".into(),
        true.into(),
        "dummy".into(),
        42u64.into(),
        Event::MapEnd,
    ]);
    assert!(s.foo_bar_baz);
}

#[test]
fn test_rename_all_screaming_kebab_case() {
    #[derive(Deserialize)]
    #[deser(rename_all = "SCREAMING-KEBAB-CASE")]
    struct Test {
        foo_bar_baz: bool,
    }

    let s: Test = deserialize(vec![
        Event::map_start(),
        "FOO-BAR-BAZ".into(),
        true.into(),
        "dummy".into(),
        42u64.into(),
        Event::MapEnd,
    ]);
    assert!(s.foo_bar_baz);
}

#[test]
fn test_rename() {
    #[derive(Deserialize)]
    #[deser(rename_all = "UPPERCASE")]
    struct Test {
        #[deser(rename = "KIND")]
        ty: usize,
        value: usize,
    }

    let s: Test = deserialize(vec![
        Event::map_start(),
        "KIND".into(),
        1u64.into(),
        "VALUE".into(),
        2u64.into(),
        Event::MapEnd,
    ]);
    assert_eq!(s.ty, 1);
    assert_eq!(s.value, 2);
}

#[test]
fn test_field_alias() {
    #[derive(Deserialize)]
    struct Test {
        #[deser(alias = "type", alias = "ty")]
        kind: usize,
    }

    let s: Test = deserialize(vec![
        Event::map_start(),
        "ty".into(),
        1u64.into(),
        Event::MapEnd,
    ]);
    assert_eq!(s.kind, 1);
}

#[test]
fn test_variant_alias() {
    #[derive(Deserialize, PartialEq, Debug)]
    enum Stuff {
        #[deser(alias = "a", alias = "alpha")]
        A,
    }

    let s: Stuff = deserialize(vec!["alpha".into()]);
    assert_eq!(s, Stuff::A);
}

#[test]
fn test_flatten_basics() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    struct Test {
        a: usize,
        b: usize,
        #[deser(flatten)]
        inner1: Inner1,
        #[deser(flatten)]
        inner2: Inner2,
        c: usize,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    struct Inner1 {
        inner1_a: usize,
        inner1_b: usize,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    struct Inner2 {
        inner2_a: usize,
        inner2_b: usize,
    }

    let s: Test = deserialize(vec![
        Event::map_start(),
        "a".into(),
        1u64.into(),
        "b".into(),
        2u64.into(),
        "inner1_a".into(),
        99u64.into(),
        "inner1_b".into(),
        100u64.into(),
        "inner2_a".into(),
        199u64.into(),
        "inner2_b".into(),
        200u64.into(),
        "c".into(),
        3u64.into(),
        Event::MapEnd,
    ]);

    assert_eq!(
        s,
        Test {
            a: 1,
            b: 2,
            c: 3,
            inner1: Inner1 {
                inner1_a: 99,
                inner1_b: 100,
            },
            inner2: Inner2 {
                inner2_a: 199,
                inner2_b: 200,
            },
        }
    );
}

#[test]
#[should_panic = "Missing field 'b'"]
fn test_flatten_incomplete_inner() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    struct Test {
        a: usize,
        #[deser(flatten)]
        inner: Inner,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    struct Inner {
        b: usize,
    }

    let _: Test = deserialize(vec![
        Event::map_start(),
        "a".into(),
        1u64.into(),
        Event::MapEnd,
    ]);
}

#[test]
fn test_deserializing_newtype() {
    #[derive(Deserialize)]
    struct MyInt(u32);

    let x: MyInt = deserialize(vec![1u64.into()]);

    assert_eq!(x.0, 1);
}
