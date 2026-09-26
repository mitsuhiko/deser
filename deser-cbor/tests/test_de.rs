//! Tests for malformed input, error reporting and edge cases.
//!
//! Adapted from the cbor2 test suite.
use crate::common;

use deser::de::DeserializeOwned;
use std::collections::HashMap;

use common::{Value, de, hex};
use deser::ext::BigInt;
use deser::{Deserialize, ErrorKind};

#[derive(Debug, PartialEq, Deserialize)]
pub enum Enum {
    Unit,
    Newtype(u32),
    Tuple(u32, u32),
    Struct { x: u32 },
}

fn assert_syntax_error<T: DeserializeOwned + std::fmt::Debug>(s: &str, offset: usize) {
    let err = de::<T>(s).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unexpected, "{}: {}", s, err);
    let msg = err.to_string();
    assert!(msg.contains("syntax error: "), "{}: {}", s, msg);
    assert_eq!(err.offset(), Some(offset), "{}: {}", s, msg);
    assert!(msg.ends_with(&format!(" at offset {}", offset)), "{}", msg);
}

fn assert_eof<T: DeserializeOwned + std::fmt::Debug>(s: &str) {
    let err = de::<T>(s).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile, "{}: {}", s, err);
}

#[test]
fn recursion_limit() {
    let depth = if cfg!(miri) { 1000 } else { 65536 };
    // Deeply nested arrays do not exhaust the stack by default...
    let bomb = [vec![0x81u8; depth], vec![0x01]].concat();
    let value: Value = deser_cbor::from_slice(&bomb).unwrap();
    // (avoid a recursive drop)
    let mut value = value;
    while let Value::Array(mut items) = value {
        value = items.pop().unwrap();
    }
    assert_eq!(value, Value::U64(1));

    // ...but the depth can be limited.
    let bomb = vec![0x81u8; depth];
    let err = deser_cbor::DeserializerConfig::new()
        .max_depth(256)
        .from_slice::<Value>(&bomb)
        .unwrap_err();
    assert!(
        err.to_string().contains("recursion limit exceeded"),
        "{}",
        err
    );

    let shallow = [0x81, 0x81, 0x81, 0x01]; // [[[1]]]
    let config = deser_cbor::DeserializerConfig::new().max_depth(2);
    let mut de = deser_cbor::Deserializer::from_slice_with_config(&shallow, &config);
    assert!(de.deserialize::<Value>().is_err());

    let config = deser_cbor::DeserializerConfig::new().max_depth(3);
    let mut de = deser_cbor::Deserializer::from_slice_with_config(&shallow, &config);
    assert_eq!(
        de.deserialize::<Value>().unwrap(),
        array![array![array![1u64]]]
    );
}

#[test]
fn deeply_tagged_input() {
    let depth = if cfg!(miri) { 1000 } else { 65536 };
    // Tags are not processed recursively, a tag bomb is harmless.
    let bomb = [vec![0xc1u8; depth], vec![0x01]].concat();
    assert_eq!(deser_cbor::from_slice::<u64>(&bomb).unwrap(), 1);
    // Tags in front of nothing.
    let bomb = vec![0xc1u8; depth];
    assert_eof::<u64>(&common::to_hex(&bomb));
}

#[test]
fn forged_length_does_not_allocate() {
    // A string claiming u64::MAX bytes followed by EOF must fail with an
    // error quickly instead of attempting a giant allocation.
    assert_eof::<Value>("5bffffffffffffffff");
    assert_eof::<Value>("7bffffffffffffffff");
    assert_eof::<Value>("9bffffffffffffffff");
    assert_eof::<Value>("bbffffffffffffffff");
    assert_eof::<Vec<u8>>("5bffffffffffffffff");
    assert_eof::<Vec<u32>>("9bffffffffffffffff");
    assert_eof::<HashMap<u32, u32>>("bbffffffffffffffff");
    assert_eof::<Value>("5f5bffffffffffffffff");
}

#[test]
fn truncated_input() {
    let bytes = deser_cbor::to_vec(&vec![1u32, 2, 3]).unwrap();
    for n in 0..bytes.len() {
        assert!(
            deser_cbor::from_slice::<Vec<u32>>(&bytes[..n]).is_err(),
            "truncation at {} must fail",
            n
        );
    }

    let bytes = hex("bf61610161629f0203ffff");
    for n in 0..bytes.len() {
        assert!(
            deser_cbor::from_slice::<Value>(&bytes[..n]).is_err(),
            "truncation at {} must fail",
            n
        );
    }
}

#[test]
fn truncated_arguments_and_bodies() {
    // Truncated multi-byte arguments.
    for s in [
        "18",
        "19",
        "1a",
        "1b",
        "1901",
        "1a010203",
        "1b01020304050607",
    ] {
        assert_eof::<u64>(s);
    }

    // A bignum tag with nothing behind it.
    assert_eof::<u64>("c2");
    assert_eof::<Value>("c2");
    assert_eof::<u64>("c241");

    // Truncated definite and segmented strings.
    assert_eof::<Vec<u8>>("42ff");
    assert_eof::<Vec<u8>>("5f");
    assert_eof::<Vec<u8>>("5f4101");
    assert_eof::<Vec<u8>>("5f41");
    assert_eof::<String>("7f");
    assert_eof::<String>("7f6161");
    assert_eof::<char>("61");

    // Truncated containers and enum payloads.
    assert_eof::<Vec<u8>>("8201");
    assert_eof::<Vec<u32>>("41");
    assert_eof::<HashMap<String, u8>>("a16161");
    assert_eof::<HashMap<String, u8>>("bf");
    assert_eof::<Enum>("a1");
    assert_eof::<Enum>("a1674e657774797065");
    assert_eof::<Option<u8>>("c1");
    assert_eof::<()>("c1");
    assert_eof::<Value>("9f");
    assert_eof::<Value>("9f01");
    assert_eof::<Value>("bf");
    assert_eof::<Value>("bf6161");
}

#[test]
fn empty_input_fails_everywhere() {
    assert_eof::<bool>("");
    assert_eof::<u64>("");
    assert_eof::<i64>("");
    assert_eof::<u128>("");
    assert_eof::<i128>("");
    assert_eof::<f32>("");
    assert_eof::<f64>("");
    assert_eof::<char>("");
    assert_eof::<String>("");
    assert_eof::<Vec<u8>>("");
    assert_eof::<(u8, u8)>("");
    assert_eof::<HashMap<String, u8>>("");
    assert_eof::<()>("");
    assert_eof::<Option<u8>>("");
    assert_eof::<Enum>("");
    assert_eof::<deser_cbor::Tagged<u8>>("");
    assert_eof::<Value>("");
}

#[test]
fn invalid_utf8_is_rejected() {
    // 0x62 (text of length 2) followed by invalid UTF-8.
    assert_syntax_error::<String>("62fffe", 1);
    assert_syntax_error::<char>("62fffe", 1);
    assert_syntax_error::<Value>("62fffe", 1);

    // Each segment of an indefinite text item must be valid on its own:
    // splitting a multi-byte character across segments is not well-formed,
    // even though the joined bytes "e2 82 ac" spell "€".
    assert_syntax_error::<String>("7f62e28261acff", 2);

    // Also in keys and in ignored values.
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct F {
        a: u8,
    }
    assert_syntax_error::<F>("a162fffe01", 2);
    assert_syntax_error::<F>("a2616101617862fffe", 7);
}

#[test]
fn malformed_breaks_and_arguments() {
    // A lone break.
    assert_syntax_error::<Value>("ff", 0);
    // A break in a definite length array.
    assert_syntax_error::<Value>("8201ff", 2);
    // A break in place of a map value.
    assert_syntax_error::<Value>("bf01ff", 2);
    // A break after a tag.
    assert_syntax_error::<Value>("9fc1ffff", 2);

    // Reserved additional information values 28-30.
    for prefix in [0x1c, 0x1d, 0x1e, 0x3c, 0x5c, 0x7c, 0x9c, 0xbc, 0xdc, 0xfc] {
        let err = deser_cbor::from_slice::<Value>(&[prefix, 0]).unwrap_err();
        assert!(
            err.to_string().contains("syntax error: ") && err.offset() == Some(0),
            "{:02x}: {}",
            prefix,
            err
        );
    }

    // An indefinite-length integer or tag is not well-formed.
    for s in ["1f", "3f", "df"] {
        assert_syntax_error::<Value>(s, 0);
    }

    // Nested indefinite string segments are not well-formed.
    assert_syntax_error::<Value>("5f5f4101ffff", 1);
    // Segments must have the major type of the string.
    assert_syntax_error::<Value>("5f6161ff", 1);
    assert_syntax_error::<String>("7f4161ff", 1);
    assert_syntax_error::<String>("7f01ff", 1);
}

#[test]
fn error_offsets() {
    // The syntax error offset points at the offending item.
    assert_syntax_error::<Value>("83011cff", 2);
    assert_syntax_error::<Vec<u32>>("83011cff", 2);
    // Trailing data after the item.
    assert_syntax_error::<u32>("0102", 1);
}

#[test]
fn type_mismatches() {
    // Every typed entry point rejects a fundamentally wrong item.
    assert!(de::<bool>("01").is_err());
    assert!(de::<f64>("6161").is_err());
    assert!(de::<u64>("6161").is_err()); // "a"
    assert!(de::<u64>("f93c00").is_err()); // 1.0
    assert!(de::<i64>("f4").is_err()); // false
    assert!(de::<char>("01").is_err());
    assert!(de::<String>("01").is_err());
    assert!(de::<Vec<u8>>("01").is_err());
    assert!(de::<Vec<u8>>("f4").is_err());
    assert!(de::<HashMap<String, u8>>("01").is_err());
    assert!(de::<()>("01").is_err());
    assert!(de::<Enum>("01").is_err());

    // The error messages name the unexpected item.
    let msg = de::<u64>("44deadbeef").unwrap_err().to_string();
    assert!(msg.contains("bytes"), "{}", msg);
    let msg = de::<u64>("80").unwrap_err().to_string();
    assert!(msg.contains("sequence"), "{}", msg);
    let msg = de::<u64>("a0").unwrap_err().to_string();
    assert!(msg.contains("map"), "{}", msg);
    let msg = de::<u64>("f6").unwrap_err().to_string();
    assert!(msg.contains("null"), "{}", msg);
    let msg = de::<u64>("f4").unwrap_err().to_string();
    assert!(msg.contains("bool"), "{}", msg);
    let msg = de::<u64>("20").unwrap_err().to_string();
    assert!(msg.contains("out of range"), "{}", msg);
    let msg = de::<bool>("f0").unwrap_err().to_string();
    assert!(msg.contains("expected bool"), "{}", msg);
}

#[test]
fn tags_are_skipped_in_typed_positions() {
    assert!(de::<bool>("c1f5").unwrap());
    assert_eq!(de::<f64>("c1f93c00").unwrap(), 1.0);
    assert_eq!(de::<f32>("c1f93c00").unwrap(), 1.0);
    assert_eq!(de::<char>("c16161").unwrap(), 'a');
    assert_eq!(de::<String>("c16161").unwrap(), "a");
    assert_eq!(de::<Vec<u8>>("c14101").unwrap(), vec![1]);
    assert_eq!(de::<Vec<u8>>("c18101").unwrap(), vec![1]);
    assert_eq!(
        de::<HashMap<String, u8>>("c1a1616101").unwrap(),
        [("a".to_string(), 1u8)].into_iter().collect()
    );
    de::<()>("c1f6").unwrap();
    assert_eq!(de::<Option<u64>>("c101").unwrap(), Some(1));
    assert_eq!(de::<Option<u64>>("c1f6").unwrap(), None);
    assert_eq!(
        de::<Enum>("c1a1674e657774797065182a").unwrap(),
        Enum::Newtype(42)
    );
    // ...even several tags deep, and on keys and elements.
    assert_eq!(de::<u64>("c1c1c101").unwrap(), 1);
    assert_eq!(
        de::<HashMap<String, u8>>("a1c16161c101").unwrap(),
        [("a".to_string(), 1u8)].into_iter().collect()
    );
    assert_eq!(de::<Vec<u8>>("82c101d9ffff02").unwrap(), vec![1, 2]);
    // A tag on a container does not leak onto its elements.
    assert_eq!(de::<Value>("c18101").unwrap(), Value::tag(1, array![1u64]));
}

#[test]
fn chars() {
    assert_eq!(de::<char>("63e6b0b4").unwrap(), '水');
    assert_eq!(de::<char>("7f6161ff").unwrap(), 'a');
    // Two characters do not make a char, and neither do zero.
    assert!(de::<char>("626162").is_err());
    assert!(de::<char>("7f61616162ff").is_err());
    assert!(de::<char>("60").is_err());
    // A text item longer than four bytes cannot be a char.
    assert!(de::<char>("6568656c6c6f").is_err());
}

#[test]
fn bignum_errors() {
    // The bignum payload must be a byte string...
    assert!(de::<u64>("c201").is_err());
    assert!(de::<u64>("c2f6").is_err());
    assert!(de::<Value>("c2f6").is_err());
    assert!(de::<Value>("c36178").is_err());
    assert!(de::<Value>("c2ff").is_err());
    let msg = de::<u64>("c2c101").unwrap_err().to_string();
    assert!(msg.contains("bignum"), "{}", msg);
    // ...and small enough for the target type.
    assert!(de::<u64>("c249010101010101010101").is_err()); // 9 bytes
    assert!(de::<i64>("c349010101010101010101").is_err());
    let msg = de::<u128>(&format!("c251{}", "01".repeat(17)))
        .unwrap_err()
        .to_string();
    assert!(msg.contains("expected u128"), "{}", msg); // 17 bytes
    assert!(de::<i128>(&format!("c351{}", "01".repeat(17))).is_err());
    // A negative bignum magnitude beyond i128.
    assert!(de::<i128>(&format!("c350{}", "80".to_string() + &"00".repeat(15))).is_err());
    // A positive bignum beyond i128, requested as signed.
    assert!(de::<i128>(&format!("c250{}", "80".to_string() + &"00".repeat(15))).is_err());
    // Negative integers never fit unsigned types.
    assert!(de::<u128>("c34101").is_err());
    assert!(de::<u64>("20").is_err());
}

#[test]
fn bignums_collapse_or_become_big_integers() {
    // In range: plain integers.
    assert_eq!(de::<Value>("c24101").unwrap(), Value::from(1u64));
    assert_eq!(de::<Value>("c34101").unwrap(), Value::from(-2i64));
    // Beyond 128 bits: big integers.
    let magnitude = "80".to_string() + &"00".repeat(15);
    assert_eq!(
        de::<Value>(&format!("c350{}", magnitude)).unwrap(),
        Value::ext(
            "-170141183460469231731687303715884105729"
                .parse::<BigInt>()
                .unwrap()
        )
    );

    // Leading zeros in bignums are tolerated everywhere.
    assert_eq!(de::<u8>("c243000007").unwrap(), 7);
    assert_eq!(de::<Value>("c243000007").unwrap(), Value::from(7u64));
    assert_eq!(
        de::<u128>(&format!("c2580f{}01{}", "00".repeat(6), "00".repeat(8))).unwrap(),
        1 << 64
    );
    assert_eq!(
        de::<Value>(&format!("c25820{}01", "00".repeat(31))).unwrap(),
        Value::from(1u64)
    );

    // A bignum wider than 128 bits cannot collapse into an integer, so it
    // becomes a big integer (and round-trips).
    let payload = format!("01{}", "00".repeat(16));
    let wide = format!("c251{}", payload);
    let value = de::<Value>(&wide).unwrap();
    assert_eq!(
        value,
        Value::ext(BigInt {
            negative: false,
            magnitude: hex(&payload)
        })
    );
    assert_eq!(common::ser(&value), wide);
    assert_eq!(
        de::<String>(&wide).unwrap(),
        "340282366920938463463374607431768211456"
    );

    // A nested bignum still collapses inside the outer tag.
    assert_eq!(
        de::<Value>("c1c24101").unwrap(),
        Value::tag(1, Value::from(1u64))
    );
}

#[test]
fn identifiers() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct F {
        a: u8,
    }

    // Field names may be segmented strings...
    assert_eq!(de::<F>("a17f6161ff01").unwrap(), F { a: 1 });
    // ...or behind a tag.
    assert_eq!(de::<F>("a1c1616101").unwrap(), F { a: 1 });
    // A missing field is reported.
    let msg = de::<F>("a0").unwrap_err().to_string();
    assert!(msg.contains("Missing field"), "{}", msg);
}

#[test]
fn options_and_units() {
    assert_eq!(de::<Option<u64>>("f7").unwrap(), None);
    assert_eq!(de::<Option<u64>>("f6").unwrap(), None);
    de::<()>("f7").unwrap();
    de::<()>("f6").unwrap();
    assert!(de::<()>("01").is_err());
}

#[test]
fn nested_element_errors_propagate() {
    // Errors inside container elements surface through every access path.
    assert!(de::<Vec<u64>>("81f5").is_err()); // [true]
    assert!(de::<HashMap<String, u64>>("a16161f5").is_err()); // {"a": true}
    assert!(de::<HashMap<u64, u64>>("a1f501").is_err()); // {true: 1}
    assert!(de::<(u8, bool)>("820102").is_err()); // (1, 2)
    assert!(de::<(u8, bool)>("8301f501").is_err()); // (1, true, 1)
    assert!(de::<Enum>("a1674e6577747970656161").is_err()); // {"Newtype": "a"}
    assert!(de::<Enum>("a1655475706c65820161").is_err()); // {"Tuple": [1, "a"]}
    assert!(de::<Enum>("a166537472756374a1617861").is_err()); // {"Struct": {"x": "a"}}
    assert!(de::<Option<u64>>("f5").is_err()); // Some(true)
    assert!(de::<deser_cbor::Tagged<u64>>("c1f5").is_err()); // 1(true)
}

#[test]
fn deserializer_can_continue_after_items() {
    // Tags from a failed item do not leak into the next one.
    let bytes = hex("c1c11c 02 c1c1f5 03");
    let mut de = deser_cbor::Deserializer::from_slice(&bytes);
    assert!(de.deserialize::<Value>().is_err());
    assert_eq!(de.deserialize::<Value>().unwrap(), Value::U64(2));
    assert!(de.deserialize::<u64>().is_err());
    assert_eq!(de.deserialize::<Value>().unwrap(), Value::U64(3));
    assert!(de.is_end());
}

#[test]
fn borrowing() {
    #[derive(deser::Deserialize, Debug)]
    struct Doc<'a> {
        text: &'a str,
        byte: &'a [u8],
    }

    // definite length strings and byte strings are slices of the input:
    // {"text": "abcd", "byte": h'010203'}
    let bytes = hex("a2 6474657874 6461626364 6462797465 43010203");
    let doc: Doc = deser_cbor::from_slice(&bytes).unwrap();
    assert_eq!(doc.text, "abcd");
    assert_eq!(doc.byte, [1, 2, 3]);
    assert!(bytes.as_ptr_range().contains(&doc.text.as_ptr()));
    assert!(bytes.as_ptr_range().contains(&doc.byte.as_ptr()));

    // indefinite length strings are assembled and cannot be borrowed
    let err = deser_cbor::from_slice::<&str>(&hex("7f 6161 6162 ff")).unwrap_err();
    assert!(err.to_string().contains("expected a borrowed string"));
    assert_eq!(
        deser_cbor::from_slice::<String>(&hex("7f 6161 6162 ff")).unwrap(),
        "ab"
    );
}

#[test]
fn value_error_offsets() {
    // errors of the values have the offset of the data item
    let err = de::<Vec<u32>>("830102f5").unwrap_err();
    assert_eq!(err.offset(), Some(3));
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected bool, expected u32 at offset 3"
    );

    // tagged items report the item after the tags
    let err = de::<Vec<u32>>("8201c1f5").unwrap_err();
    assert_eq!(err.offset(), Some(3));

    // the input ranges of the items are published
    use deser::de::{Deserialize, DeserializeDriver, Sink, SinkHandle};
    use deser::{Atom, Error, State};

    #[derive(Debug)]
    struct Range(std::ops::Range<usize>);

    deser::make_slot_wrapper!(SlotWrapper);

    impl<'de> Deserialize<'de> for Range {
        fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
            SlotWrapper::make_handle(out)
        }
    }

    impl<'de> Sink<'de> for SlotWrapper<Range> {
        fn atom(&mut self, _atom: Atom, state: &mut State) -> Result<(), Error> {
            **self = Some(Range(state.input_range().unwrap()));
            Ok(())
        }
    }

    let input = hex("82 01 63 616263");
    let mut out = None::<Vec<Range>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        deser_cbor::Deserializer::from_slice(&input)
            .drive(&mut driver)
            .unwrap();
    }
    let ranges: Vec<_> = out.unwrap().into_iter().map(|x| x.0).collect();
    assert_eq!(ranges, [1..2, 2..6]);
}
