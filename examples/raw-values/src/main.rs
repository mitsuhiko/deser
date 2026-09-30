//! Demonstrates raw values: values that are kept in the encoding of a
//! format instead of being deserialized.
//!
//! [`RawJson`] holds the JSON text of a value.  When the value comes from
//! JSON its text is kept exactly as it was (whitespace, escapes and number
//! formatting included): the value is only validated, which is faster than
//! parsing it, and it's written out again unchanged.  The value can be
//! deserialized later, once it's known what it is.
//!
//! * With the [`Borrowed`] adapter the raw value borrows its text from the
//!   input.  Without it the text is copied, so a `RawJson<'static>` can
//!   outlive the input.
//! * A raw value of another format (YAML here) is encoded as JSON, so a
//!   `RawJson` always holds JSON.
//! * Serializing into another format writes the value the raw value holds,
//!   not its text.
//! * [`RawCbor`] is the same for CBOR: it keeps the encoding of the value
//!   (including tags and the lengths as they were encoded).
//!
//! Unlike `serde_json::value::RawValue` this does not rely on a magic name
//! the format has to recognize: the input is passed on as an extension
//! value, so nothing in between (buffers, other formats) sees a fake
//! struct.
use deser::adapters::Borrowed;
use deser::{Deserialize, Serialize};
use deser_cbor::RawCbor;
use deser_json::RawJson;

/// A message whose payload depends on its kind.
#[derive(Debug, Deserialize, Serialize)]
struct Envelope<'a> {
    kind: String,
    /// Borrowed from the input, the text is not copied.
    #[deser(as = Borrowed)]
    payload: RawJson<'a>,
}

/// A stored event that outlives the input (the text is copied).
#[derive(Debug, Deserialize, Serialize)]
struct Stored {
    id: u32,
    payload: RawJson<'static>,
}

#[derive(Debug, Deserialize)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Debug, Deserialize)]
struct Message<'a> {
    /// has escapes, so it cannot be borrowed
    text: String,
    tags: Vec<&'a str>,
}

const INPUT: &str = r#"[
    {"kind": "point", "payload": {"x": 1.50, "y": -2e3}},
    {"kind": "message", "payload": {
        "text": "Gr\u00fc\u00dfe",
        "tags": ["a", "b"]
    }},
    {"kind": "unknown", "payload": [1, 2, 3]}
]"#;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn main() {
    let envelopes: Vec<Envelope> = deser_json::from_str(INPUT).unwrap();
    for envelope in &envelopes {
        println!("{}:", envelope.kind);
        // the text of the payload, exactly as it was in the input
        println!("  raw:      {}", envelope.payload.get());
        println!("  borrowed: {}", envelope.payload.is_borrowed());
        // the payload is deserialized once its type is known.  It can
        // borrow from the raw value (the message borrows its tags).
        match envelope.kind.as_str() {
            "point" => {
                let point: Point = envelope.payload.deserialize().unwrap();
                println!("  value:    {:?}", point);
            }
            "message" => {
                let message: Message = envelope.payload.deserialize().unwrap();
                println!("  value:    {:?}", message);
            }
            _ => println!("  value:    (not deserialized)"),
        }
    }
    println!();

    // written out again, the payloads keep their text
    println!("As JSON:");
    println!("{}", deser_json::to_string(&envelopes).unwrap());
    println!();

    // other formats write the values the payloads hold
    println!("As YAML:");
    print!("{}", deser_yaml::to_string(&envelopes).unwrap());
    println!();

    // a raw value that owns its text outlives the input
    let input = String::from(r#"{"id": 1, "payload": {"b": 2, "a": [ 1 ]}}"#);
    let stored: Stored = deser_json::from_str(&input).unwrap();
    drop(input);
    println!("Owned:          {}", stored.payload.get());

    // values of other formats are encoded as JSON
    let from_yaml: Stored = deser_yaml::from_str("id: 2\npayload:\n  b: 2\n  a: [1]\n").unwrap();
    println!("From YAML:      {}", from_yaml.payload.get());
    println!();

    // raw CBOR keeps the encoding of the value, with its tags and lengths:
    // the array is encoded with an indefinite length and a two byte 1
    #[derive(Debug, Deserialize, Serialize)]
    struct Record {
        id: u32,
        payload: RawCbor<'static>,
    }
    let cbor = [
        0xa2, 0x62, b'i', b'd', 0x01, 0x67, b'p', b'a', b'y', b'l', b'o', b'a', b'd', 0x9f, 0x18,
        0x01, 0x02, 0xff,
    ];
    let record: Record = deser_cbor::from_slice(&cbor).unwrap();
    println!("CBOR payload:   {}", hex(record.payload.as_bytes()));
    println!(
        "Back to CBOR:   {}",
        hex(&deser_cbor::to_vec(&record).unwrap())
    );
    println!(
        "As JSON:        {}",
        deser_json::to_string(&record).unwrap()
    );
}
