//! Formatting values with `deser-debug`.
//!
//! `ToDebug` formats any value that can be serialized the way
//! `#[derive(Debug)]` would, so types do not need to derive `Debug`.  The
//! data model of deser is small (a struct is a map, `Some(42)` is just `42`)
//! and the Rust shape comes from the description of the values (see
//! `deser::ser::Describe`).  This shows:
//!
//! * `{:?}` and `{:#?}` for types that only derive `Serialize`,
//! * enums show their variants no matter how they are serialized (the JSON
//!   of internally, adjacently tagged and untagged enums looks different),
//! * implementing `Debug` for a type with `ToDebug`,
//! * values that do not describe themselves are formatted as plain maps,
//!   lists and primitives (a hand-written `Serialize` without `describe`).
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use deser::ser::{Chunk, SerializeHandle, StructEmitter};
use deser::{Deserialize, Error, Serialize, State};
use deser_debug::ToDebug;

#[derive(Serialize, Deserialize)]
pub struct Meters(f64);

#[derive(Serialize, Deserialize)]
pub struct Point {
    x: i32,
    y: i32,
}

#[derive(Serialize, Deserialize)]
#[deser(tag = "type")]
pub enum Shape {
    Circle { center: Point, radius: Meters },
    Polygon(Polygon),
    Empty,
}

#[derive(Serialize, Deserialize)]
pub struct Polygon {
    points: Vec<(i32, i32)>,
}

#[derive(Serialize, Deserialize)]
#[deser(tag = "t", content = "c")]
pub enum Fill {
    Solid(String),
    Gradient(String, String),
}

#[derive(Serialize, Deserialize)]
#[deser(untagged)]
pub enum Label {
    Text(String),
    Styled { text: String, bold: bool },
}

/// A type which has a `Debug` implementation backed by `ToDebug`.
#[derive(Serialize, Deserialize)]
pub struct Drawing {
    name: String,
    shapes: Vec<Shape>,
    fill: Option<Fill>,
    labels: BTreeMap<String, Label>,
}

impl fmt::Debug for Drawing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&ToDebug::new(self), f)
    }
}

/// A hand-written `Serialize` which does not describe itself.
pub struct Anonymous;

impl Serialize for Anonymous {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Struct(Box::new(AnonymousEmitter(0))))
    }
}

struct AnonymousEmitter(usize);

impl StructEmitter for AnonymousEmitter {
    fn next(
        &mut self,
        _state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        self.0 += 1;
        Ok(match self.0 {
            1 => Some((Cow::Borrowed("answer"), SerializeHandle::boxed(42u32))),
            2 => Some((Cow::Borrowed("maybe"), SerializeHandle::boxed(Some(true)))),
            _ => None,
        })
    }
}

fn main() {
    // types that only derive `Serialize`
    let point = Point { x: 1, y: -2 };
    let formatted = format!("{:?}", ToDebug::new(&point));
    println!("{}", formatted);
    assert_eq!(formatted, "Point { x: 1, y: -2 }");

    // `Option` and newtypes are serialized as their content but keep their
    // shape in the debug output
    let radius = Some(Meters(2.5));
    println!("{}", deser_json::to_string(&radius).unwrap());
    let formatted = format!("{:?}", ToDebug::new(&radius));
    println!("{}", formatted);
    assert_eq!(formatted, "Some(Meters(2.5))");

    // enums show their variants, whatever their representation
    let drawing: Drawing = deser_json::from_str(
        r#"{
            "name": "example",
            "shapes": [
                {"type": "Circle", "center": {"x": 0, "y": 0}, "radius": 1.5},
                {"type": "Polygon", "points": [[0, 0], [1, 0], [0, 1]]},
                {"type": "Empty"}
            ],
            "fill": {"t": "Gradient", "c": ["red", "blue"]},
            "labels": {
                "title": {"text": "Hello", "bold": true},
                "footer": "World"
            }
        }"#,
    )
    .unwrap();
    println!("{}", deser_json::to_string(&drawing).unwrap());
    println!("{:#?}", drawing);
    assert_eq!(
        format!("{:?}", ToDebug::new(&drawing.fill)),
        r#"Some(Gradient("red", "blue"))"#
    );
    assert_eq!(
        format!("{:?}", ToDebug::new(&drawing.shapes[1])),
        "Polygon(Polygon { points: [(0, 0), (1, 0), (0, 1)] })"
    );

    // without a description a struct looks like a map, its fields still
    // describe themselves
    let formatted = format!("{:?}", ToDebug::new(&Anonymous));
    println!("{}", formatted);
    assert_eq!(formatted, r#"{"answer": 42, "maybe": Some(true)}"#);
}
