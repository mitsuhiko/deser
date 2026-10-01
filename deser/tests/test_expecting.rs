//! The sinks of values report what the values expect.
//!
//! [`Deserialize::expecting`] is what a value expects, the sink a value
//! creates reports the same with `Sink::expecting` (for slots this is
//! where it comes from).
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::net::IpAddr;
use std::num::NonZero;
use std::ops::{Bound, Range};
use std::path::PathBuf;
use std::sync::Arc;

use deser::adapters::{
    As, Base64, Borrowed, DefaultOnError, DisplayFromStr, Flag, FromInto, MapSkipError, Separated,
    SkipBlank, TrimWhitespace, VecSkipError,
};
use deser::de::{RecordBuf, Recording, Sink};
use deser::{Deserialize, State, Streamed};

/// Checks that the sink of the adapter reports what the adapter expects.
#[track_caller]
fn check_as<T: Send, A: Deserialize<'static, T>>(expected: &str) {
    let mut out = None;
    let mut state = State::new();
    let handle = A::deserialize_into(&mut out, &mut state);
    assert_eq!(handle.expecting(), A::expecting());
    assert_eq!(A::expecting(), expected);
}

#[track_caller]
fn check<T: Deserialize<'static>>(expected: &str) {
    check_as::<T, T>(expected);
}

#[derive(Deserialize)]
struct Point {
    #[allow(dead_code)]
    x: u32,
}

#[derive(Deserialize)]
#[deser(expecting = "a point in time")]
struct Instant {
    #[allow(dead_code)]
    at: u64,
}

#[derive(Deserialize)]
struct Meters(#[allow(dead_code)] f64);

#[derive(Deserialize)]
struct Pair(#[allow(dead_code)] u8, #[allow(dead_code)] u8);

#[derive(Deserialize)]
struct Unit;

#[derive(Deserialize)]
#[deser(transparent)]
struct Name {
    #[allow(dead_code)]
    name: String,
}

#[derive(Deserialize)]
enum Color {
    Red,
}

#[derive(Deserialize)]
enum Shape {
    #[allow(dead_code)]
    Circle { radius: f64 },
}

#[derive(Deserialize)]
#[deser(untagged)]
enum Either {
    #[allow(dead_code)]
    Number(u32),
    #[allow(dead_code)]
    Text(String),
}

#[derive(Deserialize)]
#[deser(tag = "type")]
enum Tagged {
    #[allow(dead_code)]
    Circle { radius: f64 },
}

#[derive(Deserialize)]
enum WithFallback {
    #[allow(dead_code)]
    Number(u32),
    #[deser(untagged)]
    #[allow(dead_code)]
    Text(String),
}

#[derive(Deserialize)]
struct Flattened {
    #[allow(dead_code)]
    id: u32,
    #[deser(flatten)]
    #[allow(dead_code)]
    point: Point,
}

#[derive(Deserialize)]
#[deser(deserialize_as = FromInto<u32>)]
struct Id(#[allow(dead_code)] u32);

impl From<u32> for Id {
    fn from(value: u32) -> Self {
        Id(value)
    }
}

#[test]
fn test_primitives() {
    check::<bool>("bool");
    check::<u8>("u8");
    check::<i64>("i64");
    check::<f32>("f32");
    check::<char>("char");
    check::<String>("string");
    check::<&str>("str");
    check::<()>("null");
    check::<PathBuf>("path");
    check::<IpAddr>("IP address");
    check::<NonZero<u32>>("u32");
}

#[test]
fn test_containers() {
    check::<Vec<u32>>("vec");
    check::<Vec<u8>>("bytes");
    check::<VecDeque<u32>>("VecDeque");
    check::<Box<[u32]>>("slice");
    check::<BTreeMap<String, u32>>("BTreeMap");
    check::<HashMap<String, u32>>("HashMap");
    check::<BTreeSet<u32>>("BTreeSet");
    check::<HashSet<u32>>("HashSet");
    check::<(u8, String)>("tuple");
    check::<[u32; 2]>("array");
    check::<Result<u32, String>>("Result");
    check::<Range<u32>>("range");
    check::<Bound<u32>>("Bound");
    check::<Streamed<u32>>("sequence");
}

#[test]
fn test_any_value() {
    check::<Recording>("any value");
    check::<RecordBuf<'static>>("any value");
}

#[test]
fn test_wrappers() {
    // wrappers expect what their value expects
    check::<Option<u32>>("u32");
    check::<Box<u32>>("u32");
    check::<Arc<String>>("string");
    check::<Cow<'static, str>>("string");
    check::<Option<Vec<u32>>>("vec");
}

#[test]
fn test_adapters() {
    check_as::<u32, DisplayFromStr>("string");
    check_as::<bool, Flag>("flag");
    check_as::<Vec<u8>, Base64>("bytes or base64 string");
    check_as::<Cow<'static, str>, Borrowed>("string");
    check_as::<u64, FromInto<u32>>("u32");
    check_as::<u32, DefaultOnError>("u32");
    check_as::<u32, TrimWhitespace>("u32");
    check_as::<u32, SkipBlank>("u32");
    check_as::<Vec<u32>, VecSkipError>("vec");
    check_as::<BTreeMap<String, u32>, MapSkipError>("BTreeMap");
    check_as::<Vec<u32>, Separated>("vec");
    check::<As<u32, DisplayFromStr>>("string");
}

#[test]
fn test_derive() {
    check::<Point>("Point");
    check::<Instant>("a point in time");
    check::<Meters>("f64");
    check::<Pair>("tuple");
    check::<Unit>("Unit");
    check::<Name>("string");
    check::<Color>("Color");
    check::<Shape>("Shape");
    check::<Either>("Either");
    check::<Tagged>("Tagged");
    check::<WithFallback>("WithFallback");
    check::<Flattened>("Flattened");
    check::<Id>("u32");
}
