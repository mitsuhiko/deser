//! This example shows the different enum representations that deser
//! supports.  They follow the ones known from serde.
//!
//! Beyond the representations this shows untagged variants in tagged enums
//! as fallbacks, renaming the fields of struct variants and flattening
//! shared fields into variants.
//!
//! Every input is parsed from JSON, dumped with `Debug` and serialized back
//! to JSON.  Note that the inputs do not need to be in the canonical form:
//! tags can come after the content, deser buffers the content until it
//! knows the variant.
use std::fmt::Debug;

use deser::{Deserialize, Serialize};

/// Externally tagged (the default): unit variants are strings, all other
/// variants are maps with the variant name as single key.
#[derive(Debug, Serialize, Deserialize)]
#[deser(rename_all = "snake_case")]
enum Command {
    Quit,
    Move { x: i32, y: i32 },
    Write(String),
    Color(u8, u8, u8),
}

/// Internally tagged: the variant name is stored in a field next to the
/// fields of the variant.  The fields of the struct variants are renamed
/// with `rename_all_fields`.
#[derive(Debug, Serialize, Deserialize)]
#[deser(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
enum Event {
    Login {
        user_name: String,
    },
    Logout {
        user_name: String,
        logout_reason: Option<String>,
    },
    // the inner struct's fields are merged with the tag
    Purchase(Purchase),
    Heartbeat,
    // unknown event types end up here
    #[deser(other)]
    Unknown,
}

#[derive(Debug, Serialize, Deserialize)]
struct Purchase {
    item: String,
    price: f64,
}

/// Adjacently tagged: the variant name and the content are stored in
/// separate fields.
#[derive(Debug, Serialize, Deserialize)]
#[deser(tag = "kind", content = "data", rename_all = "snake_case")]
enum Message {
    Text(String),
    Point(i32, i32),
    Ping,
}

/// Struct variants can flatten shared fields.  Variants marked `untagged`
/// are not tagged: they are tried in order if the tagged representation
/// fails, here for values in an older format.
#[derive(Debug, Serialize, Deserialize)]
#[deser(tag = "shape", rename_all = "snake_case")]
enum Shape {
    Circle {
        #[deser(flatten)]
        style: Style,
        radius: f64,
    },
    Square {
        #[deser(flatten)]
        style: Style,
        side: f64,
    },
    /// the old format was just the radius of a circle
    #[deser(untagged)]
    LegacyCircle(f64),
}

#[derive(Debug, Serialize, Deserialize)]
struct Style {
    color: String,
    #[deser(default)]
    filled: bool,
}

/// Untagged: there is no tag at all.  The variants are tried in order and
/// the first one that accepts the value wins.
#[derive(Debug, Serialize, Deserialize)]
#[deser(untagged)]
enum Value<T> {
    Null,
    Number(T),
    Text(String),
    List(Vec<Value<T>>),
    Object { name: String, value: Box<Value<T>> },
}

fn show<'de, T: Deserialize<'de> + Serialize + Debug>(title: &str, json: &'de str) {
    println!("== {}", title);
    println!("input:  {}", json);
    let value: T = deser_json::from_str(json).unwrap();
    println!("parsed: {:?}", value);
    println!("output: {}", deser_json::to_string(&value).unwrap());
    println!();
}

fn main() {
    show::<Vec<Command>>(
        "externally tagged",
        r#"["quit", {"move": {"x": 1, "y": -1}}, {"write": "hello"}, {"color": [255, 0, 0]}]"#,
    );

    show::<Vec<Event>>(
        "internally tagged",
        r#"[
            {"type": "login", "userName": "jane"},
            {"userName": "jane", "logoutReason": "timeout", "type": "logout"},
            {"item": "book", "type": "purchase", "price": 9.5},
            {"type": "heartbeat"},
            {"type": "something_new", "whatever": [1, 2, 3]}
        ]"#,
    );

    show::<Vec<Message>>(
        "adjacently tagged",
        r#"[
            {"kind": "text", "data": "hi"},
            {"data": [1, 2], "kind": "point"},
            {"kind": "ping"}
        ]"#,
    );

    show::<Vec<Shape>>(
        "flattened fields and untagged variants",
        r#"[
            {"shape": "circle", "radius": 1.5, "color": "red"},
            {"color": "blue", "filled": true, "side": 2.0, "shape": "square"},
            4.0
        ]"#,
    );

    show::<Value<u64>>(
        "untagged",
        r#"[null, 42, "42", {"name": "answer", "value": [42]}]"#,
    );
}
