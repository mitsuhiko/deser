//! Borrowing strings and bytes from the input.
//!
//! Formats pass on strings (and CBOR byte strings) that are slices of their
//! input without copying them.  Types can borrow them:
//!
//! * `&str` and `&[u8]` always borrow.  If the format cannot hand out a
//!   slice of the input (for instance because the JSON string contains
//!   escape sequences) deserializing fails.
//! * `Cow<str>` and `Cow<[u8]>` with the `Borrowed` adapter borrow when
//!   possible and own the data otherwise.
//!
//! Structs and enums can borrow, also when values have to be recorded and
//! replayed (for instance the fields of internally tagged enums that come
//! before the tag, or the content of untagged enums).
use std::borrow::Cow;

use deser::adapters::Borrowed;
use deser::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct LogLine<'a> {
    level: &'a str,
    #[deser(as = Borrowed)]
    message: Cow<'a, str>,
    #[deser(as = Vec<Borrowed>)]
    tags: Vec<Cow<'a, str>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Packet<'a> {
    kind: &'a str,
    payload: &'a [u8],
}

#[derive(Debug, Deserialize)]
#[deser(tag = "type", rename_all = "lowercase")]
pub enum Token<'a> {
    Word { text: &'a str },
    Number { text: &'a str, value: f64 },
    Punct(Punct<'a>),
}

#[derive(Debug, Deserialize)]
pub struct Punct<'a> {
    char: &'a str,
}

/// Returns `true` if `part` points into `whole`.
fn is_within(part: &[u8], whole: &[u8]) -> bool {
    whole.as_ptr_range().contains(&part.as_ptr())
}

fn main() {
    let input = r#"{
        "level": "info",
        "message": "said \"hello\"",
        "tags": ["web", "caf\u00e9"]
    }"#;
    let line: LogLine = deser_json::from_str(input).unwrap();
    println!("{:#?}", line);
    assert!(is_within(line.level.as_bytes(), input.as_bytes()));

    // strings with escape sequences have to be unescaped into a new string
    for (name, value) in [
        ("message", &line.message),
        ("tags[0]", &line.tags[0]),
        ("tags[1]", &line.tags[1]),
    ] {
        let borrowed = matches!(value, Cow::Borrowed(_));
        println!("{}: {}", name, if borrowed { "borrowed" } else { "owned" });
    }
    assert!(matches!(line.message, Cow::Owned(_)));
    assert!(matches!(line.tags[0], Cow::Borrowed(_)));
    assert!(matches!(line.tags[1], Cow::Owned(_)));

    // a `&str` cannot hold an unescaped string
    let err =
        deser_json::from_str::<LogLine>(r#"{"level": "\u0069nfo", "message": "", "tags": []}"#)
            .unwrap_err();
    println!("\nerror: {}", err);

    // enums borrow like structs, also the fields that come before the tag
    // (they are recorded until the variant is known)
    let input = r#"[
        {"type": "word", "text": "pi"},
        {"text": "3.14", "value": 3.14, "type": "number"},
        {"char": "!", "type": "punct"}
    ]"#;
    let tokens: Vec<Token> = deser_json::from_str(input).unwrap();
    println!("\n{:?}", tokens);
    for token in &tokens {
        let text = match token {
            Token::Word { text } | Token::Number { text, .. } => text,
            Token::Punct(punct) => punct.char,
        };
        assert!(is_within(text.as_bytes(), input.as_bytes()));
    }

    // CBOR has byte strings which are borrowed as well
    let cbor = deser_cbor::to_vec(&Packet {
        kind: "ping",
        payload: b"\x00\x01\x02\x03",
    })
    .unwrap();
    let packet: Packet = deser_cbor::from_slice(&cbor).unwrap();
    println!("\n{:?}", packet);
    assert!(is_within(packet.kind.as_bytes(), &cbor));
    assert!(is_within(packet.payload, &cbor));
}
