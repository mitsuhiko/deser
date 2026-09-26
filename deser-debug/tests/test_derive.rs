use std::collections::BTreeMap;
use std::fmt;

use deser::Serialize;
use deser_debug::ToDebug;

/// Checks that the output matches `Debug`, also in alternate mode.
fn check<T: Serialize + fmt::Debug>(value: T) {
    assert_eq!(ToDebug::new(&value).to_string(), format!("{:?}", value));
    assert_eq!(
        format!("{:#?}", ToDebug::new(&value)),
        format!("{:#?}", value)
    );
}

#[derive(Serialize, Debug)]
struct Meters(f64);

#[derive(Serialize, Debug)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Serialize, Debug)]
struct Shapes {
    name: String,
    origin: Point,
    distance: Option<Meters>,
    tags: Vec<String>,
    extra: BTreeMap<String, (u8, bool)>,
}

#[derive(Serialize, Debug)]
enum Color {
    Red,
    Green,
}

#[derive(Serialize, Debug)]
enum External {
    Unit,
    Newtype(Meters),
    Tuple(u32, String),
    Struct { a: u32, b: Option<Color> },
}

#[derive(Serialize, Debug)]
#[deser(tag = "type")]
enum Internal {
    Unit,
    Newtype(Point),
    Struct { a: u32 },
}

#[derive(Serialize, Debug)]
#[deser(tag = "t", content = "c")]
enum Adjacent {
    Unit,
    Newtype(Meters),
    Tuple(u32, u32),
    Struct { a: u32 },
}

#[derive(Serialize, Debug)]
#[deser(untagged)]
enum Untagged {
    Unit,
    Newtype(Meters),
    Tuple(u32, u32),
    Struct { a: u32 },
}

#[test]
fn test_floats() {
    check(0.1f32);
    check(vec![0.1f32, 1.0, f32::MAX, f32::NAN]);
    check(0.1f64);
}

#[test]
fn test_structs() {
    check(Meters(5.5));
    check(Some(Meters(1.0)));
    check(Shapes {
        name: "shapes".into(),
        origin: Point { x: 1, y: -2 },
        distance: Some(Meters(3.25)),
        tags: vec!["a".into(), "b".into()],
        extra: BTreeMap::from([("k".into(), (1, true))]),
    });
}

#[test]
fn test_enums() {
    check(vec![Color::Red, Color::Green]);
    check(vec![
        External::Unit,
        External::Newtype(Meters(2.0)),
        External::Tuple(1, "x".into()),
        External::Struct {
            a: 1,
            b: Some(Color::Green),
        },
    ]);
    check(vec![
        Internal::Unit,
        Internal::Newtype(Point { x: 1, y: 2 }),
        Internal::Struct { a: 1 },
    ]);
    check(vec![
        Adjacent::Unit,
        Adjacent::Newtype(Meters(2.0)),
        Adjacent::Tuple(1, 2),
        Adjacent::Struct { a: 1 },
    ]);
    check(vec![
        Untagged::Unit,
        Untagged::Newtype(Meters(2.0)),
        Untagged::Tuple(1, 2),
        Untagged::Struct { a: 1 },
    ]);
}
