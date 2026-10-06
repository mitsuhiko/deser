//! A type which uses many features of the derive.
//!
//! The fuzz targets deserialize their input into it to exercise the code
//! of the derive (and the buffering of tagged and untagged enums) with the
//! events of every format.
use std::borrow::Cow;
use std::collections::BTreeMap;

use deser::de::Recording;
use deser::{Deserialize, Serialize};
use deser_value::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct Typed<'a> {
    #[deser(default)]
    pub id: Option<u64>,
    #[deser(default, alias = "Name")]
    pub name: Option<Cow<'a, str>>,
    #[deser(default)]
    pub small: Option<i8>,
    #[deser(default)]
    pub big: Option<i128>,
    #[deser(default)]
    pub float: Option<f32>,
    #[deser(default)]
    pub flag: Option<bool>,
    #[deser(default)]
    pub chr: Option<char>,
    #[deser(default)]
    pub data: Option<Vec<u8>>,
    #[deser(default)]
    pub list: Vec<Item<'a>>,
    #[deser(default)]
    pub tuple: Option<(u8, String, Option<bool>)>,
    #[deser(default)]
    pub map: BTreeMap<i32, Shape>,
    #[deser(default)]
    pub external: Option<External>,
    #[deser(default)]
    pub adjacent: Option<Adjacent>,
    #[deser(default)]
    pub untagged: Vec<Untagged>,
    #[deser(default)]
    pub level: Option<Level>,
    #[deser(default)]
    pub value: Option<Value>,
    #[deser(flatten)]
    pub nested: Nested,
    #[deser(flatten)]
    pub rest: BTreeMap<String, Recording>,
}

#[derive(Debug, Deserialize, Serialize)]
#[deser(deny_unknown_fields)]
pub struct Item<'a> {
    pub key: Cow<'a, str>,
    #[deser(default)]
    pub value: Option<f64>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Nested {
    #[deser(default)]
    pub enabled: Option<bool>,
    #[deser(default, rename = "port-number")]
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize, Serialize)]
#[deser(tag = "type", rename_all = "snake_case")]
pub enum Shape {
    Circle {
        radius: f64,
    },
    Rect {
        width: u32,
        height: u32,
    },
    #[deser(default)]
    Point,
    #[deser(other)]
    Other(#[deser(tag)] String, Recording),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum External {
    Unit,
    Newtype(i64),
    Tuple(u8, u8),
    Struct {
        a: String,
        b: Option<Vec<u32>>,
    },
    #[deser(rename = 1)]
    One,
}

#[derive(Debug, Deserialize, Serialize)]
#[deser(tag = "t", content = "c")]
pub enum Adjacent {
    A(String),
    B { x: i32 },
    C,
}

#[derive(Debug, Deserialize, Serialize)]
#[deser(untagged)]
pub enum Untagged {
    Int(i64),
    Pair(String, u8),
    Struct { x: f64, y: f64 },
    Shape(Shape),
    Text(String),
    Bytes(Vec<u8>),
    Any(Recording),
}

#[derive(Debug, Deserialize, Serialize)]
#[deser(repr)]
#[repr(u8)]
pub enum Level {
    Low = 1,
    Normal,
    High = 10,
}
