use deser::de::DeserializeDriver;
use deser::{Deserialize, Event};

fn deserialize<T: Deserialize>(events: Vec<Event<'_>>) -> T {
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

    let s: MyContainer = deserialize(vec![Event::MapStart, Event::MapEnd]);
    assert_eq!(s.field1, 0);
    assert!(!s.field2);

    let s: MyContainer = deserialize(vec![
        Event::MapStart,
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

    let s: MyContainer = deserialize(vec![Event::MapStart, Event::MapEnd]);
    assert_eq!(s.field1, 0);
    assert!(!s.field2);

    let s: MyContainer = deserialize(vec![
        Event::MapStart,
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

    let s: MyContainer = deserialize(vec![Event::MapStart, Event::MapEnd]);
    assert_eq!(s.val, None);

    let s: MyContainer = deserialize(vec![
        Event::MapStart,
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
        #[deser(default = "other_default")]
        val: Option<String>,
    }

    let s: MyOtherContainer = deserialize(vec![Event::MapStart, Event::MapEnd]);
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
        Event::MapStart,
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

    let s: MyContainer = deserialize(vec![Event::MapStart, Event::MapEnd]);
    assert_eq!(s.field1, 0);
    assert!(s.field2);

    let s: MyContainer = deserialize(vec![
        Event::MapStart,
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

    let _: MyContainer = deserialize(vec![Event::MapStart, Event::MapEnd]);
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
    #[deser(default = "default_it")]
    pub struct MyContainer {
        field1: usize,
        field2: usize,
    }

    let s: MyContainer = deserialize(vec![Event::MapStart, Event::MapEnd]);
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
        #[deser(default = "default_field_1")]
        field1: usize,
        #[deser(default = "default_field_2")]
        field2: usize,
    }

    let s: MyContainer = deserialize(vec![Event::MapStart, Event::MapEnd]);
    assert_eq!(s.field1, 1);
    assert_eq!(s.field2, 2);
}

#[test]
fn test_rename_all_camel_case() {
    #[derive(Deserialize)]
    #[deser(rename_all = "camelCase")]
    struct Test {
        foo_bar_baz: bool,
    }

    let s: Test = deserialize(vec![
        Event::MapStart,
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
        Event::MapStart,
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
        Event::MapStart,
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
        Event::MapStart,
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
        Event::MapStart,
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
        Event::MapStart,
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
        Event::MapStart,
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
        Event::MapStart,
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
        Event::MapStart,
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
        Event::MapStart,
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
        Event::MapStart,
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
        Event::MapStart,
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
