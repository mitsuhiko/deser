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
