//! Runs the library of this example on the host.  The library itself does
//! not use the standard library, see `src/lib.rs`.
use no_std::{Message, Reading, decode, encode, from_json, to_json};

fn main() {
    let reading = Message::Reading(Reading {
        sensor_id: 7,
        unit: "celsius",
        values: vec![21.5, 21.75],
        scale: 1.0,
        raw: [0xde, 0xad, 0xbe, 0xef],
    });
    let bytes = encode(&reading).unwrap();
    println!("CBOR: {} bytes", bytes.len());
    assert_eq!(decode(&bytes).unwrap(), reading);

    let json = to_json(&reading).unwrap();
    println!("JSON: {json}");
    assert_eq!(
        json,
        r#"{"type":"reading","sensorId":7,"unit":"celsius","values":[21.5,21.75],"scale":1.0,"raw":"3q2+7w=="}"#
    );

    // a message of newer firmware round trips
    let input = r#"{"type":"calibration","offset":-0.5}"#;
    let message = from_json(input).unwrap();
    assert!(matches!(message, Message::Unknown(ref tag, _) if tag == "calibration"));
    let bytes = encode(&message).unwrap();
    let forwarded = decode(&bytes).unwrap();
    assert_eq!(to_json(&forwarded).unwrap(), input);
    println!("forwarded: {input}");

    let err = from_json(r#"{"type":"status","uptime":-1}"#).unwrap_err();
    println!("error: {err}");
    assert_eq!(err.line(), Some(1));
}
