//! Demonstrates exact numbers and date-times with `deser-json`.
//!
//! JSON numbers that cannot be represented exactly as `f64` are passed on
//! as [`Number`] which holds the text of the number (and its value as
//! `f64`).  Fields of type `Number` keep that text (no matter how long it
//! is) so it can be printed, inspected or written out again verbatim.
//! Deserializing into a `Number` copies the text, so a `Number<'static>`
//! can be kept around after the input is gone.  Fields of type `f64` still
//! work and receive the (approximated) value.
//!
//! Well-known types like `jiff::Timestamp` (with the `jiff` feature of
//! `deser`) are deserialized from RFC 3339 strings.
use deser::ext::Number;
use deser::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct Measurement {
    /// An exact number: keeps the text of the JSON number.
    value: Number<'static>,
    /// A regular float: receives the value as `f64`.
    approximate: f64,
    /// A jiff timestamp, parsed from an RFC 3339 string.
    recorded_at: jiff::Timestamp,
}

const INPUT: &str = r#"[
    {
        "value": 0.10000000000000000001,
        "approximate": 0.10000000000000000001,
        "recorded_at": "2024-06-01T12:30:00.123456789Z"
    },
    {
        "value": 3.14159265358979323846264338327950288419716939937510582097494459230781640628620899862803482534211706798214808651328230664709384460955058223172535940812848111745028410270193852110555964462294895493038196,
        "approximate": 3.14159265358979323846264338327950288419716939937510582097494459230781640628620899862803482534211706798214808651328230664709384460955058223172535940812848111745028410270193852110555964462294895493038196,
        "recorded_at": "2024-06-01T14:30:00+02:00"
    },
    {
        "value": 123456789012345678901234567890123456789012345678901234567890,
        "approximate": 123456789012345678901234567890123456789012345678901234567890,
        "recorded_at": "1970-01-01T00:00:00Z"
    },
    {
        "value": 1.5,
        "approximate": 1.5,
        "recorded_at": "2000-02-29T23:59:59.5-08:00"
    }
]"#;

fn main() {
    // the input is only borrowed while parsing, the measurements own their
    // numbers and can outlive it.
    let input = INPUT.to_string();
    let measurements: Vec<Measurement> = deser_json::from_str(&input).unwrap();
    drop(input);

    println!("Debug output:");
    println!("{:#?}", measurements);
    println!();

    for measurement in &measurements {
        println!("value:       {}", measurement.value);
        println!("  length:    {}", measurement.value.as_str().len());
        println!("  integer:   {}", measurement.value.is_integer());
        println!("  as f64:    {:?}", measurement.value.value());
        println!("approximate: {:?}", measurement.approximate);
        println!("recorded at: {}", measurement.recorded_at);
        println!(
            "  in Vienna: {}",
            measurement.recorded_at.in_tz("Europe/Vienna").unwrap()
        );
        println!();
    }

    // the exact numbers are written out verbatim, the floats lose precision
    println!("Serialized again:");
    println!("{}", deser_json::to_string(&measurements).unwrap());
}
