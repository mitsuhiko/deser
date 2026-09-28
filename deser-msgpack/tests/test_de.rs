//! Tests for malformed input, error reporting and edge cases.
use crate::common;

use deser::de::{DeserializeOwned, Limits};
use std::collections::HashMap;

use common::{Value, de, hex};
use deser::{Deserialize, ErrorKind};
use deser_msgpack::Ext;

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
    // beyond the limit that is checked, miri is slow
    let depth = if cfg!(miri) { 300 } else { 65536 };
    // Deeply nested arrays do not exhaust the stack by default...
    let bomb = [vec![0x91u8; depth], vec![0x01]].concat();
    let value: Value = deser_msgpack::from_slice(&bomb).unwrap();
    // (avoid a recursive drop)
    let mut value = value;
    while let Value::Array(mut items) = value {
        value = items.pop().unwrap();
    }
    assert_eq!(value, Value::U64(1));

    // ...but the depth can be limited with a layer.
    let limited = |input: &[u8], max_depth| {
        deser_msgpack::Deserializer::from_slice(input).deserialize_with::<Value, _>(|driver| {
            driver.push_layer(Limits::new().max_depth(max_depth))
        })
    };
    let bomb = vec![0x91u8; depth];
    let err = limited(&bomb, 256).unwrap_err();
    assert!(
        err.to_string().contains("recursion limit exceeded"),
        "{}",
        err
    );

    let shallow = [0x91, 0x91, 0x91, 0x01]; // [[[1]]]
    assert!(limited(&shallow, 2).is_err());
    assert_eq!(limited(&shallow, 3).unwrap(), array![array![array![1u64]]]);
}

#[test]
fn forged_length_does_not_allocate() {
    // Items claiming u32::MAX bytes or items followed by EOF must fail with
    // an error quickly instead of attempting a giant allocation.
    for prefix in ["c6", "db", "c9"] {
        assert_eof::<Value>(&format!("{}ffffffff01", prefix));
    }
    assert_eof::<Value>("ddffffffff");
    assert_eof::<Value>("dfffffffff");
    assert_eof::<Vec<u8>>("c6ffffffff");
    assert_eof::<String>("dbffffffff");
    assert_eof::<Vec<u32>>("ddffffffff");
    assert_eof::<HashMap<u32, u32>>("dfffffffff");
}

#[test]
fn truncated_input() {
    let bytes = deser_msgpack::to_vec(&vec![1u32, 200, 70000]).unwrap();
    for n in 0..bytes.len() {
        assert!(
            deser_msgpack::from_slice::<Vec<u32>>(&bytes[..n]).is_err(),
            "truncation at {} must fail",
            n
        );
    }

    // {"a": 1, "b": [2, 3.5], "c": bin, "d": ext, "e": timestamp}
    let bytes =
        hex("85 a161 01 a162 9202cb400c000000000000 a163 c4020102 a164 d40701 a165 d6ff00000001");
    assert!(deser_msgpack::from_slice::<Value>(&bytes).is_ok());
    for n in 0..bytes.len() {
        let err = deser_msgpack::from_slice::<Value>(&bytes[..n]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::EndOfFile, "truncation at {}", n);
    }
}

#[test]
fn truncated_arguments_and_bodies() {
    // Truncated multi-byte arguments.
    for s in [
        "cc",
        "cd",
        "cdff",
        "ce",
        "ceffffff",
        "cf",
        "cf01020304050607",
        "d0",
        "d1",
        "d2",
        "d3",
        "ca",
        "ca000000",
        "cb",
        "cb00000000000000",
    ] {
        assert_eof::<Value>(s);
    }

    // Truncated lengths and bodies.
    for s in [
        "a1",
        "d9",
        "d901",
        "da00",
        "db000000",
        "c4",
        "c401",
        "c500",
        "c6000000",
        "d4",
        "d401",
        "d50101",
        "c7",
        "c701",
        "c70101",
        "c800",
        "c9000000",
        "c900000001",
    ] {
        assert_eof::<Value>(s);
    }
    assert_eof::<char>("a1");
    assert_eof::<Ext>("d4");
    assert_eof::<deser::ext::Timestamp>("d6ff000000");

    // Truncated containers and enum payloads.
    assert_eof::<Vec<u8>>("9201");
    assert_eof::<Vec<u32>>("dc00");
    assert_eof::<HashMap<String, u8>>("81a161");
    assert_eof::<HashMap<String, u8>>("de0001");
    assert_eof::<Enum>("81");
    assert_eof::<Enum>("81a74e657774797065");
    assert_eof::<Value>("91");
    assert_eof::<Value>("81");
    assert_eof::<Value>("81a161");
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
    assert_eof::<Ext>("");
    assert_eof::<Value>("");
}

#[test]
fn invalid_utf8_is_rejected() {
    // a string of length 2 followed by invalid UTF-8
    assert_syntax_error::<String>("a2fffe", 1);
    assert_syntax_error::<char>("a2fffe", 1);
    assert_syntax_error::<Value>("a2fffe", 1);
    assert_syntax_error::<String>("d902fffe", 2);
    // a truncated multi-byte character
    assert_syntax_error::<String>("a2e282", 1);

    // Also in keys and in ignored values.
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct F {
        a: u8,
    }
    assert_syntax_error::<F>("81a2fffe01", 2);
    assert_syntax_error::<F>("82a16101a178a2fffe", 7);

    // binary data does not need to be UTF-8
    assert_eq!(de::<Vec<u8>>("c402fffe").unwrap(), [0xff, 0xfe]);
}

#[test]
fn reserved_byte() {
    // 0xc1 is never used
    assert_syntax_error::<Value>("c1", 0);
    assert_syntax_error::<Value>("9201c1", 2);
    assert_syntax_error::<Vec<u32>>("9301c102", 2);
    assert_syntax_error::<Value>("81c101", 1);
    assert_syntax_error::<Value>("81a161c1", 3);
    // Trailing data after the item.
    assert_syntax_error::<u32>("0102", 1);
}

#[test]
fn type_mismatches() {
    // Every typed entry point rejects a fundamentally wrong item.
    assert!(de::<bool>("01").is_err());
    assert!(de::<f64>("a161").is_err());
    assert!(de::<u64>("a161").is_err()); // "a"
    assert!(de::<u64>("ca3f800000").is_err()); // 1.0
    assert!(de::<i64>("c2").is_err()); // false
    assert!(de::<char>("01").is_err());
    assert!(de::<String>("01").is_err());
    assert!(de::<Vec<u8>>("01").is_err());
    assert!(de::<Vec<u8>>("c2").is_err());
    assert!(de::<HashMap<String, u8>>("01").is_err());
    assert!(de::<()>("01").is_err());
    assert!(de::<Enum>("01").is_err());
    assert!(de::<Ext>("c40101").is_err());

    // The error messages name the unexpected item.
    let msg = de::<u64>("c404deadbeef").unwrap_err().to_string();
    assert!(msg.contains("bytes"), "{}", msg);
    let msg = de::<u64>("90").unwrap_err().to_string();
    assert!(msg.contains("sequence"), "{}", msg);
    let msg = de::<u64>("80").unwrap_err().to_string();
    assert!(msg.contains("map"), "{}", msg);
    let msg = de::<u64>("c0").unwrap_err().to_string();
    assert!(msg.contains("null"), "{}", msg);
    let msg = de::<u64>("c2").unwrap_err().to_string();
    assert!(msg.contains("bool"), "{}", msg);
    let msg = de::<u64>("ff").unwrap_err().to_string();
    assert!(msg.contains("invalid value -1, expected u64"), "{}", msg);
    let msg = de::<Ext>("01").unwrap_err().to_string();
    assert!(msg.contains("expected msgpack extension"), "{}", msg);
}

#[test]
fn integers() {
    // signed and unsigned encodings of all widths
    assert_eq!(de::<u8>("d07f").unwrap(), 127);
    assert_eq!(de::<u64>("d37fffffffffffffff").unwrap(), i64::MAX as u64);
    assert_eq!(de::<i64>("cf7fffffffffffffff").unwrap(), i64::MAX);
    assert_eq!(de::<i8>("e0").unwrap(), -32);
    assert_eq!(de::<i8>("d080").unwrap(), -128);
    assert_eq!(de::<i16>("d18000").unwrap(), i16::MIN);
    assert_eq!(de::<i32>("d280000000").unwrap(), i32::MIN);
    assert_eq!(de::<i64>("d38000000000000000").unwrap(), i64::MIN);
    assert_eq!(de::<u64>("cfffffffffffffffff").unwrap(), u64::MAX);
    // positive signed integers are unsigned integers
    assert_eq!(de::<Value>("d001").unwrap(), Value::U64(1));
    assert_eq!(de::<Value>("d3ffffffffffffffff").unwrap(), Value::I64(-1));
    // out of range
    assert!(de::<u8>("cd0100").is_err());
    assert!(de::<i8>("cc80").is_err());
    assert!(de::<u64>("d0ff").is_err());
    assert!(de::<i64>("cfffffffffffffffff").is_err());
    assert_eq!(de::<i128>("cfffffffffffffffff").unwrap(), u64::MAX as i128);
}

#[test]
fn floats() {
    // floats keep their precision
    assert_eq!(de::<Value>("ca3f800000").unwrap(), Value::F32(1.0));
    assert_eq!(de::<Value>("cb3ff0000000000000").unwrap(), Value::F64(1.0));
    assert_eq!(de::<f64>("ca3dcccccd").unwrap(), f64::from(0.1f32));
    assert_eq!(de::<f32>("cb3fb999999999999a").unwrap(), 0.1f32);
    assert!(de::<f64>("cb7ff8000000000000").unwrap().is_nan());
    assert!(de::<f32>("ca7fc00000").unwrap().is_nan());
    assert_eq!(de::<f64>("cbfff0000000000000").unwrap(), f64::NEG_INFINITY);
}

#[test]
fn chars() {
    assert_eq!(de::<char>("a3e6b0b4").unwrap(), '水');
    assert_eq!(de::<char>("d90161").unwrap(), 'a');
    // Two characters do not make a char, and neither do zero.
    assert!(de::<char>("a26162").is_err());
    assert!(de::<char>("a0").is_err());
    // A string longer than four bytes cannot be a char.
    assert!(de::<char>("a568656c6c6f").is_err());
}

#[test]
fn identifiers() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct F {
        a: u8,
    }

    // Field names may use any string encoding...
    assert_eq!(de::<F>("81a16101").unwrap(), F { a: 1 });
    assert_eq!(de::<F>("81d9016101").unwrap(), F { a: 1 });
    assert_eq!(de::<F>("de0001da00016101").unwrap(), F { a: 1 });
    // A missing field is reported.
    let msg = de::<F>("80").unwrap_err().to_string();
    assert!(msg.contains("missing field `a`"), "{}", msg);
}

#[test]
fn options_and_units() {
    assert_eq!(de::<Option<u64>>("c0").unwrap(), None);
    assert_eq!(de::<Option<u64>>("01").unwrap(), Some(1));
    de::<()>("c0").unwrap();
    assert!(de::<()>("01").is_err());
}

#[test]
fn nested_element_errors_propagate() {
    // Errors inside container elements surface through every access path.
    assert!(de::<Vec<u64>>("91c3").is_err()); // [true]
    assert!(de::<HashMap<String, u64>>("81a161c3").is_err()); // {"a": true}
    assert!(de::<HashMap<u64, u64>>("81c301").is_err()); // {true: 1}
    assert!(de::<(u8, bool)>("920102").is_err()); // (1, 2)
    assert!(de::<(u8, bool)>("9301c301").is_err()); // (1, true, 1)
    assert!(de::<Enum>("81a74e6577747970 65a161".replace(' ', "").as_str()).is_err()); // {"Newtype": "a"}
    assert!(de::<Enum>("81a55475706c659201a161").is_err()); // {"Tuple": [1, "a"]}
    assert!(de::<Enum>("81a6537472756374 81a178a161".replace(' ', "").as_str()).is_err()); // {"Struct": {"x": "a"}}
    assert!(de::<Option<u64>>("c3").is_err()); // Some(true)
}

#[test]
fn deserializer_can_continue_after_items() {
    // after a syntax error reading continues after the offending byte,
    // after an error of a value after the item
    let bytes = hex("9201c1 02 c3 03");
    let mut de = deser_msgpack::Deserializer::from_slice(&bytes);
    assert!(de.deserialize::<Value>().is_err());
    assert_eq!(de.offset(), 3);
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

    // strings and binary data are slices of the input:
    // {"text": "abcd", "byte": bin 010203}
    let bytes = hex("82 a474657874 a461626364 a462797465 c403010203");
    let doc: Doc = deser_msgpack::from_slice(&bytes).unwrap();
    assert_eq!(doc.text, "abcd");
    assert_eq!(doc.byte, [1, 2, 3]);
    assert!(bytes.as_ptr_range().contains(&doc.text.as_ptr()));
    assert!(bytes.as_ptr_range().contains(&doc.byte.as_ptr()));
}

#[test]
fn value_error_offsets() {
    // errors of the values have the offset of the item
    let err = de::<Vec<u32>>("930102c3").unwrap_err();
    assert_eq!(err.offset(), Some(3));
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected bool, expected u32 at offset 3"
    );

    // the input ranges of the items are published
    use deser::de::{Deserialize, DeserializeDriver, Sink, SinkHandle};
    use deser::{Atom, Error, State};

    #[derive(Debug)]
    struct Range(std::ops::Range<usize>);

    deser::make_slot_wrapper!(SlotWrapper);

    impl<'de> Deserialize<'de> for Range {
        fn deserialize_into<'out>(
            out: &'out mut Option<Self>,
            _state: &mut State,
        ) -> SinkHandle<'out, 'de> {
            SlotWrapper::make_handle(out)
        }
    }

    impl<'de> Sink<'de> for SlotWrapper<Range> {
        fn atom(&mut self, _atom: Atom, state: &mut State) -> Result<(), Error> {
            **self = Some(Range(state.input_range().unwrap()));
            Ok(())
        }
    }

    let input = hex("93 01 a3616263 cd0100");
    let mut out = None::<Vec<Range>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        deser_msgpack::Deserializer::from_slice(&input)
            .drive(&mut driver)
            .unwrap();
    }
    let ranges: Vec<_> = out.unwrap().into_iter().map(|x| x.0).collect();
    assert_eq!(ranges, [1..2, 2..6, 6..9]);
}

#[test]
fn test_declared_lengths_are_not_trusted() {
    // an array and a map that claim billions of items but are empty: the
    // preallocation is capped, deserialization fails at the end of input.
    assert!(deser_msgpack::from_slice::<Vec<u64>>(&hex("ddffffffff")).is_err());
    assert!(deser_msgpack::from_slice::<HashMap<String, u64>>(&hex("dfffffffff")).is_err());
}
