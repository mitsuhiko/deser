//! Tests for CBOR tag support.
//!
//! Adapted from the cbor2 test suite.
mod common;

use std::collections::BTreeMap;

use common::{Value, de, hex, ser};
use deser::{Deserialize, Serialize};
use deser_cbor::Tagged;

#[test]
fn tagged_roundtrip() {
    // Tag 0: standard date/time string.
    let datetime = Tagged::new(0, String::from("2013-03-21T20:04:00Z"));
    assert_eq!(
        ser(&datetime),
        "c074323031332d30332d32315432303a30343a30305a"
    );
    // valid date/time strings are handled by the format (see
    // test_well_known.rs), invalid ones are passed on as tagged strings
    let back: Tagged<String> = de("c074323031332d30332d32315432303a30343a30305a").unwrap();
    assert_eq!(back, Tagged::untagged(String::from("2013-03-21T20:04:00Z")));
    let back: Tagged<String> = de("c06178").unwrap();
    assert_eq!(back, Tagged::new(0, String::from("x")));

    // Tag 1: epoch based date/time.
    assert_eq!(ser(&Tagged::new(1, 1363896240u64)), "c11a514b67b0");
    let back: Tagged<u64> = de("c11a514b67b0").unwrap();
    assert_eq!(back, Tagged::new(1, 1363896240));

    // Large tag numbers.
    assert_eq!(ser(&Tagged::new(262, "x")), "d901066178");
    assert_eq!(ser(&Tagged::new(u64::MAX, ())), "dbfffffffffffffffff6");
    let back: Tagged<()> = de("dbfffffffffffffffff6").unwrap();
    assert_eq!(back.tag, Some(u64::MAX));
}

#[test]
fn untagged_values() {
    // An untagged value has no tag...
    let back: Tagged<u64> = de("1a514b67b0").unwrap();
    assert_eq!(back, Tagged::untagged(1363896240));
    // ...and is written without one.
    assert_eq!(ser(&Tagged::untagged(1363896240u64)), "1a514b67b0");
}

#[test]
fn tagged_containers() {
    // Tags on arrays and maps.
    let value = Tagged::new(1000, vec![1u32, 2]);
    assert_eq!(ser(&value), "d903e8820102");
    assert_eq!(de::<Tagged<Vec<u32>>>("d903e8820102").unwrap(), value);

    let map: BTreeMap<String, u32> = [("a".to_string(), 1)].into_iter().collect();
    let value = Tagged::new(55799, map);
    assert_eq!(ser(&value), "d9d9f7a1616101");
    assert_eq!(
        de::<Tagged<BTreeMap<String, u32>>>("d9d9f7a1616101").unwrap(),
        value
    );
    // indefinite length
    assert_eq!(
        de::<Tagged<BTreeMap<String, u32>>>("d9d9f7bf616101ff").unwrap(),
        value
    );

    // Tags on bytes.
    let value = Tagged::new(24, vec![1u8, 2]);
    assert_eq!(ser(&value), "d818420102");
    assert_eq!(de::<Tagged<Vec<u8>>>("d818420102").unwrap(), value);
}

#[test]
fn nested_tags() {
    // Tagged<Tagged<T>> captures two tags from the outside in.
    let value = Tagged::new(1, Tagged::new(2000, "x"));
    assert_eq!(ser(&value), "c1d907d06178");
    let back: Tagged<Tagged<String>> = de("c1d907d06178").unwrap();
    assert_eq!(back.tag, Some(1));
    assert_eq!(back.value.tag, Some(2000));
    assert_eq!(back.value.value, "x");

    // A single wrapper takes the outermost tag, the inner one is ignored.
    let back: Tagged<String> = de("c1d907d06178").unwrap();
    assert_eq!(back, Tagged::new(1, "x".to_string()));

    // Value captures all tags.
    let nested = Value::tag(1, Value::tag(0, Value::from("x")));
    assert_eq!(ser(&nested), "c1c06178");
    assert_eq!(de::<Value>("c1c06178").unwrap(), nested);
}

#[test]
fn tags_do_not_leak() {
    // The tag of a container does not apply to its elements and a tag of
    // an element does not apply to the next element.
    let back: Tagged<Vec<Tagged<u32>>> = de("c1 83 d9030001 02 03").unwrap();
    assert_eq!(back.tag, Some(1));
    assert_eq!(
        back.value,
        [
            Tagged::new(768, 1),
            Tagged::untagged(2),
            Tagged::untagged(3)
        ]
    );

    // Map keys and values: {1("a"): 2, "b": 42(3)}
    let back: BTreeMap<Tagged<String>, Tagged<u32>> = de("a2 c16161 02 6162 d82a03").unwrap();
    assert_eq!(
        back.into_iter().collect::<Vec<_>>(),
        [
            (Tagged::untagged("b".to_string()), Tagged::new(42, 3)),
            (Tagged::new(1, "a".to_string()), Tagged::untagged(2)),
        ]
    );
}

#[test]
fn tags_in_structs() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Event {
        name: String,
        at: Tagged<u64>,
        payload: Option<Tagged<Vec<u8>>>,
    }

    let event = Event {
        name: "x".into(),
        at: Tagged::new(1, 42),
        payload: Some(Tagged::new(24, vec![0xf6])),
    };
    let bytes = deser_cbor::to_vec(&event).unwrap();
    assert_eq!(
        common::to_hex(&bytes),
        "a3646e616d6561786261 74c1182a677061796c6f6164d81841f6".replace(' ', "")
    );
    assert_eq!(deser_cbor::from_slice::<Event>(&bytes).unwrap(), event);
}

#[test]
fn tags_survive_buffering() {
    // Internally tagged enums buffer the content until they see the tag.
    // The tags of buffered values are replayed with them.
    #[derive(Debug, PartialEq, Deserialize)]
    #[deser(tag = "type")]
    enum Message {
        Stamp { at: Tagged<u64>, other: Tagged<u64> },
    }

    let bytes = hex("a3 626174 c1182a 656f74686572 07 6474797065 655374616d70");
    let msg: Message = deser_cbor::from_slice(&bytes).unwrap();
    assert_eq!(
        msg,
        Message::Stamp {
            at: Tagged::new(1, 42),
            other: Tagged::untagged(7),
        }
    );

    // Untagged enums replay the value into the variants.
    #[derive(Debug, PartialEq, Deserialize)]
    #[deser(untagged)]
    enum Either {
        Num(Tagged<u64>),
        Text(Tagged<String>),
    }
    assert_eq!(
        de::<Vec<Either>>("82 c1 6178 d82a 05").unwrap(),
        vec![
            Either::Text(Tagged::new(1, "x".into())),
            Either::Num(Tagged::new(42, 5)),
        ]
    );
}

#[test]
fn value_tags() {
    // Tags survive a round trip through the dynamic value.
    let value = Value::tag(262, Value::from("x"));
    assert_eq!(ser(&value), "d901066178");
    assert_eq!(de::<Value>("d901066178").unwrap(), value);

    // Tagged and Value interconvert.
    let bytes = deser_cbor::to_vec(&value).unwrap();
    let wrapped: Tagged<String> = deser_cbor::from_slice(&bytes).unwrap();
    assert_eq!(wrapped, Tagged::new(262, "x".into()));
    assert_eq!(deser_cbor::to_vec(&wrapped).unwrap(), bytes);
}

#[test]
fn other_formats_ignore_tags() {
    // Tags are dropped when serializing to other formats: the tagged value
    // serializes like the value.
    let value = vec![Tagged::new(1, 1u32), Tagged::untagged(2)];
    let mut events = Vec::new();
    deser::ser::SerializeDriver::new(&value)
        .drive(|event, _, _| {
            events.push(format!("{:?}", event));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        events,
        ["SeqStart", "Atom(U64(1))", "Atom(U64(2))", "SeqEnd"]
    );

    // A tagged value in a later CBOR serialization is not affected by a
    // previous serialization.
    assert_eq!(ser(&1u32), "01");
}
