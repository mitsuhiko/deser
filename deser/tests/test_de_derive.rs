use deser::de::Driver;
use deser::{Deserialize, Event};

fn deserialize<T: Deserialize>(events: Vec<Event<'_>>) -> T {
    let mut out = None;
    {
        let mut driver = Driver::new(&mut out);
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
