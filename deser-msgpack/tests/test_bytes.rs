//! Formats with native bytes ignore the bytes formats requested by values.
use crate::common;

use common::{de, ser};
use deser::adapters::{BytesFallback, IntSeq};
use deser::{Deserialize, Serialize};
use deser_encoding::Hex;

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
    let msgpack = ser(&value);
    assert_eq!(
        msgpack,
        concat!(
            "83",
            "a468696e74",     // "hint"
            "c40101",         // bin 01
            "a3736571",       // "seq"
            "c4020203",       // bin 0203
            "a6666f72636564", // "forced"
            "a26666",         // "ff"
        )
    );
    assert_eq!(de::<Blob>(&msgpack).unwrap(), value);

    // strings are decoded as base64 (and the encodings of the adapters)
    let value: Vec<u8> = de("a44166383d").unwrap(); // "Af8="
    assert_eq!(value, [1, 255]);
}
