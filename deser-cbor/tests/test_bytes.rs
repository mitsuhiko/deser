//! Formats with native bytes ignore the bytes formats requested by values.
use crate::common;

use common::{de, ser};
use deser::adapters::bytes::{BytesFallback, Hex, IntSeq};
use deser::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Blob {
    #[deser(as = BytesFallback<Hex>)]
    hint: Vec<u8>,
    #[deser(as = BytesFallback<IntSeq>)]
    seq: [u8; 2],
    #[deser(as = Hex)]
    forced: Vec<u8>,
}

#[test]
fn test_bytes_formats() {
    let value = Blob {
        hint: vec![1],
        seq: [2, 3],
        forced: vec![255],
    };
    let cbor = ser(&value);
    assert_eq!(
        cbor,
        concat!(
            "a3",
            "6468696e74",     // "hint"
            "4101",           // h'01'
            "63736571",       // "seq"
            "420203",         // h'0203'
            "66666f72636564", // "forced"
            "626666",         // "ff"
        )
    );
    assert_eq!(de::<Blob>(&cbor).unwrap(), value);

    // strings are decoded as base64 (and the encodings of the adapters)
    let value: Vec<u8> = de("644166383d").unwrap(); // "Af8="
    assert_eq!(value, [1, 255]);
}
