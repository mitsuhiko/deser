//! The tests run for both adapters.
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;

use deser::ErrorKind;
use deser::adapters::As;
use deser::ser::SerializeDriver;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Point {
    x: i32,
    y: i32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Shape {
    Empty,
    Circle(f64),
    Rect(Point, Point),
    Named { name: String, points: Vec<Point> },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum NumOrStr {
    Num(u64),
    Str(String),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Meta {
    version: u32,
    #[serde(flatten)]
    rest: BTreeMap<String, NumOrStr>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Doc {
    shapes: Vec<Shape>,
    index: HashMap<u32, String>,
    nested: Option<Box<Doc>>,
    big: u128,
    meta: Option<Meta>,
    #[serde(default)]
    tags: Vec<char>,
}

#[derive(Debug, PartialEq, serde::Deserialize)]
pub struct Borrowed<'a> {
    name: &'a str,
    parts: Vec<&'a str>,
}

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static GUARDS_DROPPED: Cell<usize> = const { Cell::new(0) };
}

/// Tracks how many instances are alive.
#[derive(Debug)]
pub struct Tracked;

impl<'de> serde::Deserialize<'de> for Tracked {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Tracked, D::Error> {
        u32::deserialize(deserializer)?;
        LIVE.with(|x| x.set(x.get() + 1));
        Ok(Tracked)
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        LIVE.with(|x| x.set(x.get() - 1));
    }
}

/// Holds a guard on the stack while it serializes a long sequence.
pub struct Guarded;

struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        GUARDS_DROPPED.with(|x| x.set(x.get() + 1));
    }
}

impl serde::Serialize for Guarded {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let _guard = Guard;
        serializer.collect_seq(0..100u32)
    }
}

/// Panics when deserialized from a map.
#[derive(Debug)]
pub struct Panics;

impl<'de> serde::Deserialize<'de> for Panics {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Panics, D::Error> {
        serde::de::IgnoredAny::deserialize(deserializer)?;
        panic!("boom");
    }
}

fn sample_doc() -> Doc {
    Doc {
        shapes: vec![
            Shape::Empty,
            Shape::Circle(1.5),
            Shape::Rect(Point { x: 0, y: 0 }, Point { x: 2, y: 3 }),
            Shape::Named {
                name: "tri".into(),
                points: vec![Point { x: 1, y: 1 }, Point { x: -1, y: 4 }],
            },
        ],
        index: HashMap::from([(42, "answer".into())]),
        nested: Some(Box::new(Doc {
            shapes: vec![],
            index: HashMap::new(),
            nested: None,
            big: 1,
            meta: None,
            tags: vec![],
        })),
        big: u64::MAX as u128 + 1,
        meta: Some(Meta {
            version: 2,
            rest: BTreeMap::from([
                ("a".into(), NumOrStr::Num(1)),
                ("b".into(), NumOrStr::Str("x".into())),
            ]),
        }),
        tags: vec!['a', 'b'],
    }
}

macro_rules! adapter_tests {
    ($name:ident, $adapter:ty) => {
        mod $name {
            use super::*;

            type A = $adapter;

            #[derive(Debug, PartialEq, deser::Serialize, deser::Deserialize)]
            struct Wrapper {
                #[deser(as = A)]
                doc: Doc,
                #[deser(as = Option<A>)]
                point: Option<Point>,
                #[deser(as = A)]
                missing: Option<Point>,
                #[deser(as = A)]
                value: serde_json::Value,
                #[deser(as = Vec<A>)]
                addrs: Vec<IpAddr>,
            }

            #[derive(Debug, PartialEq, deser::Serialize, deser::Deserialize)]
            #[deser(skip_serializing_optionals)]
            struct Optionals {
                #[deser(as = A)]
                a: Option<Point>,
                #[deser(as = A)]
                b: Option<Point>,
            }

            #[derive(Debug, deser::Deserialize)]
            #[allow(dead_code)]
            struct TrackedItems {
                #[deser(as = A)]
                items: Vec<Tracked>,
            }

            #[derive(Debug, deser::Deserialize)]
            #[allow(dead_code)]
            struct PanicsInside {
                #[deser(as = A)]
                value: Panics,
            }

            #[test]
            fn test_roundtrip() {
                let doc = sample_doc();
                let json = format!(
                    r#"{{"doc": {}, "point": {{"x": 1, "y": 2}}, "value": {{"a": [1, null]}}, "addrs": ["127.0.0.1", "::1"]}}"#,
                    serde_json::to_string(&doc).unwrap(),
                );
                let wrapper: Wrapper = deser_json::from_str(&json).unwrap();
                assert_eq!(
                    wrapper,
                    Wrapper {
                        doc: doc.clone(),
                        point: Some(Point { x: 1, y: 2 }),
                        missing: None,
                        value: serde_json::json!({"a": [1, null]}),
                        addrs: vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
                    }
                );

                let out = deser_json::to_string(&wrapper).unwrap();
                // serde_json cannot parse the 128 bit integer into a value
                assert_eq!(
                    out,
                    format!(
                        r#"{{"doc":{},"point":{{"x":1,"y":2}},"missing":null,"value":{{"a":[1,null]}},"addrs":["127.0.0.1","::1"]}}"#,
                        serde_json::to_string(&doc).unwrap()
                    )
                );

                let again: Wrapper = deser_json::from_str(&out).unwrap();
                assert_eq!(again, wrapper);
            }

            #[test]
            fn test_atoms() {
                let value: As<IpAddr, A> = deser_json::from_str(r#""10.0.0.1""#).unwrap();
                assert_eq!(value.to_string(), "10.0.0.1");
                assert_eq!(deser_json::to_string(&value).unwrap(), r#""10.0.0.1""#);

                let value: As<Shape, A> = deser_json::from_str(r#""Empty""#).unwrap();
                assert_eq!(*value, Shape::Empty);
                assert_eq!(deser_json::to_string(&value).unwrap(), r#""Empty""#);

                let value: As<Option<u32>, A> = deser_json::from_str("null").unwrap();
                assert_eq!(*value, None);

                // f32 values keep their precision in both directions
                let value: As<f32, A> = deser_json::from_str("0.1").unwrap();
                assert_eq!(*value, 0.1);
                assert_eq!(deser_json::to_string(&value).unwrap(), "0.1");
                let mut out = None::<As<Vec<f32>, A>>;
                {
                    let mut driver = deser::de::DeserializeDriver::new(&mut out);
                    for event in [
                        deser::Event::seq_start(),
                        deser::Event::Atom(deser::Atom::F32(0.1)),
                        deser::Event::SeqEnd,
                    ] {
                        driver.emit(event).unwrap();
                    }
                }
                assert_eq!(*out.unwrap(), [0.1f32]);
            }

            #[test]
            fn test_keys() {
                let value: As<BTreeMap<u32, BTreeMap<bool, String>>, A> =
                    deser_json::from_str(r#"{"1": {"true": "a"}, "2": {}}"#).unwrap();
                assert_eq!(value[&1][&true], "a");
                assert_eq!(value[&2].len(), 0);

                // serde values as keys of deser maps
                let value: BTreeMap<As<u32, A>, u32> =
                    deser_json::from_str(r#"{"1": 2}"#).unwrap();
                assert_eq!(value.values().copied().collect::<Vec<_>>(), [2]);
            }

            #[test]
            fn test_borrowed() {
                let json = r#"{"name": "hello", "parts": ["a", "b"]}"#.to_string();
                let value: As<Borrowed, A> = deser_json::from_str(&json).unwrap();
                assert_eq!(value.name, "hello");
                assert_eq!(value.parts, ["a", "b"]);
            }

            #[test]
            fn test_optionals() {
                let value: Optionals = deser_json::from_str(r#"{"b": {"x": 1, "y": 2}}"#).unwrap();
                assert_eq!(value.a, None);
                assert_eq!(
                    deser_json::to_string(&value).unwrap(),
                    r#"{"b":{"x":1,"y":2}}"#
                );
            }

            #[test]
            fn test_missing_required() {
                let err = deser_json::from_str::<Wrapper>(r#"{"value": 1, "addrs": []}"#)
                    .unwrap_err();
                assert_eq!(err.kind(), ErrorKind::MissingField);
            }

            #[test]
            fn test_error_location() {
                let json = "{\"doc\": {\n  \"shapes\": [\n    {\"Circle\": \"x\"}\n  ]}}";
                let err = deser_json::from_str::<Wrapper>(json).unwrap_err();
                assert_eq!(err.kind(), ErrorKind::Unexpected);
                assert!(
                    err.message().contains("invalid type: string \"x\", expected f64"),
                    "{}",
                    err
                );
                assert_eq!((err.line(), err.column()), (Some(3), Some(16)), "{}", err);
            }

            #[test]
            fn test_wrong_type() {
                let err = deser_json::from_str::<As<Point, A>>(r#"{"x": 1, "y": [true]}"#)
                    .unwrap_err();
                assert!(err.message().contains("expected i32"), "{}", err);
                let err = deser_json::from_str::<As<Shape, A>>(r#"{"Empty": 1}"#).unwrap_err();
                assert!(err.message().contains("expected unit"), "{}", err);
                let err =
                    deser_json::from_str::<As<Shape, A>>(r#"{"Circle": 1.0, "Empty": null}"#)
                        .unwrap_err();
                assert!(err.message().contains("expected end of enum"), "{}", err);
            }

            #[test]
            fn test_abort_drops_values() {
                LIVE.with(|x| x.set(0));
                // the value is cut off while the serde value is incomplete
                let err =
                    deser_json::from_str::<TrackedItems>(r#"{"items": [1, 2, 3"#).unwrap_err();
                assert_eq!(err.kind(), ErrorKind::EndOfFile);
                assert_eq!(LIVE.with(|x| x.get()), 0);

                // an error in deser after serde failed
                let err = deser_json::from_str::<TrackedItems>(r#"{"items": [1, "x", 3]}"#)
                    .unwrap_err();
                assert!(err.message().contains("expected u32"), "{}", err);
                assert_eq!(LIVE.with(|x| x.get()), 0);

                let value = deser_json::from_str::<TrackedItems>(r#"{"items": [1, 2]}"#).unwrap();
                assert_eq!(LIVE.with(|x| x.get()), 2);
                drop(value);
                assert_eq!(LIVE.with(|x| x.get()), 0);
            }

            #[test]
            fn test_abort_serialization() {
                GUARDS_DROPPED.with(|x| x.set(0));
                let value = As::<Guarded, A>::new(Guarded);
                {
                    let mut driver = SerializeDriver::new(&value);
                    for _ in 0..5 {
                        driver.next().unwrap().unwrap();
                    }
                }
                assert_eq!(GUARDS_DROPPED.with(|x| x.get()), 1);

                let out = deser_json::to_string(&value).unwrap();
                assert!(out.starts_with("[0,1,2,"));
                assert_eq!(GUARDS_DROPPED.with(|x| x.get()), 2);
            }

            #[test]
            fn test_panic() {
                let rv = std::panic::catch_unwind(|| {
                    deser_json::from_str::<PanicsInside>(r#"{"value": {"a": [1, 2]}}"#)
                });
                assert!(rv.is_err());
                // still works after the panic
                let value: As<Point, A> = deser_json::from_str(r#"{"x": 1, "y": 2}"#).unwrap();
                assert_eq!(*value, Point { x: 1, y: 2 });
            }

            #[test]
            fn test_deep_nesting() {
                let depth = 100;
                let json = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
                let value: As<serde_json::Value, A> = deser_json::from_str(&json).unwrap();
                assert_eq!(deser_json::to_string(&value).unwrap(), json);
            }
        }
    };
}

adapter_tests!(buffered, deser_serde::Serde);
