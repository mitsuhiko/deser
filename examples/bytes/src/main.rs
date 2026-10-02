//! Bytes in formats with and without native bytes.
//!
//! Bytes (`Vec<u8>`, `[u8; N]`, `Cow<[u8]>`) are part of the data model.
//! CBOR and MessagePack have native bytes, JSON and TOML do not.  There they
//! are written as base64 strings by default and can be configured in the
//! context or per value:
//!
//! * a plain `Vec<u8>` uses the format of the context (base64 unless
//!   configured otherwise) and native bytes in CBOR and MessagePack,
//! * `#[deser(as = Hex)]` makes it a hex string in all formats (also CBOR).
//!   deser provides the base64 encodings, `Hex` comes from `deser-encoding`,
//! * `#[deser(as = BytesFallback<Hex>)]` keeps native bytes in CBOR and
//!   MessagePack and picks hex only where bytes are not supported,
//! * `#[deser(as = BytesFallback<IntSeq>)]` writes arrays of integers
//!   instead (like `serde_json` does).
//!
//! When reading, all of them accept native bytes and strings in their
//! encoding, plain bytes also accept arrays of integers.
use deser::adapters::{Base64Url, BytesFallback, IntSeq};
use deser::{BytesFormat, Context, Deserialize, Serialize};
use deser_encoding::Hex;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Blob {
    /// base64 in JSON and TOML (configurable), bytes in CBOR and MessagePack
    data: Vec<u8>,
    /// a hex string everywhere
    #[deser(as = Hex)]
    digest: [u8; 8],
    /// hex in JSON and TOML, bytes in CBOR and MessagePack
    #[deser(as = BytesFallback<Hex>)]
    signature: Vec<u8>,
    /// arrays of integers in JSON and TOML, bytes in CBOR and MessagePack
    #[deser(as = BytesFallback<IntSeq>)]
    legacy: Vec<u8>,
}

fn main() {
    let blob = Blob {
        data: b"hello \xff".to_vec(),
        digest: *b"\x2c\xf2\x4d\xba\x5f\xb0\xa3\x0e",
        signature: vec![0xde, 0xad, 0xbe, 0xef],
        legacy: vec![1, 2, 3],
    };

    // the defaults
    let json = deser_json::to_string(&blob).unwrap();
    println!("JSON:\n{}\n", json);
    assert_eq!(
        json,
        r#"{"data":"aGVsbG8g/w==","digest":"2cf24dba5fb0a30e","signature":"deadbeef","legacy":[1,2,3]}"#
    );
    let toml = deser_toml::to_string(&blob).unwrap();
    println!("TOML:\n{}", toml);
    assert_eq!(deser_toml::from_str::<Blob>(&toml).unwrap(), blob);

    // the format of bytes in the context only changes plain bytes, the
    // values that requested a representation keep it.  The context is
    // used for reading as well.
    let url_safe = Context::new().with(BytesFormat::encoded::<Base64Url>());
    let mut ser = deser_json::Serializer::new();
    ser.serialize_in(&blob, &url_safe).unwrap();
    let json = ser.finish();
    println!("JSON with URL safe base64:\n{}\n", json);
    assert!(json.starts_with(r#"{"data":"aGVsbG8g_w==","digest":"2cf24dba5fb0a30e""#));
    let read: Blob = deser_json::Deserializer::from_str(&json)
        .deserialize_in(&url_safe)
        .unwrap();
    assert_eq!(read, blob);
    let seq = Context::new().with(BytesFormat::SEQ);
    let mut ser = deser_toml::Serializer::new();
    ser.serialize_in(&blob, &seq).unwrap();
    let toml = ser.finish();
    println!("TOML with arrays of integers:\n{}", toml);
    assert!(toml.starts_with("data = [104, 101, 108, 108, 111, 32, 255]\n"));

    // reading is lenient: base64 in both alphabets with or without padding
    // and arrays of integers are accepted for plain bytes.  The encoded
    // values accept their encoding (and arrays with `IntSeq`).
    for data in [
        r#""aGVsbG8g/w==""#,
        r#""aGVsbG8g_w""#,
        "[104,101,108,108,111,32,255]",
    ] {
        let json = format!(
            r#"{{"data":{},"digest":"2CF24DBA5FB0A30E","signature":"deadbeef","legacy":[1,2,3]}}"#,
            data
        );
        assert_eq!(deser_json::from_str::<Blob>(&json).unwrap(), blob);
    }

    // CBOR has byte strings: everything but the hex value is written as
    // bytes.  Byte strings are major type 2 (0x40 to 0x5b), text strings
    // major type 3 (0x60 to 0x7b).
    let cbor = deser_cbor::to_vec(&blob).unwrap();
    println!("CBOR:\n{}\n", hex(&cbor));
    assert!(contains(&cbor, b"\x44\xde\xad\xbe\xef"));
    assert!(contains(&cbor, b"\x70\x32\x63\x66"));
    assert_eq!(deser_cbor::from_slice::<Blob>(&cbor).unwrap(), blob);

    // the same in MessagePack: bytes are bin 8 (0xc4) and strings fixstr
    // (0xa0 to 0xbf)
    let msgpack = deser_msgpack::to_vec(&blob).unwrap();
    println!("MessagePack:\n{}\n", hex(&msgpack));
    assert!(contains(&msgpack, b"\xc4\x04\xde\xad\xbe\xef"));
    assert!(contains(&msgpack, b"\xb0\x32\x63\x66"));
    assert_eq!(deser_msgpack::from_slice::<Blob>(&msgpack).unwrap(), blob);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
