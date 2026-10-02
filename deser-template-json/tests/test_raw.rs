use std::collections::BTreeMap;

use deser::adapters::Borrowed;
use deser::de::Recording;
use deser::{Deserialize, Serialize};
use deser_json::RawJson;

use super::{DIALECT, KEEPS_INPUT, RawText, dialect};

/// The dialect is JSON.
const IS_JSON: bool = !DIALECT.comments && !DIALECT.json5 && !DIALECT.hjson;

#[derive(Deserialize, Debug, PartialEq)]
struct Payload {
    a: Vec<f64>,
    b: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct Envelope<'a> {
    kind: String,
    #[deser(as = Borrowed)]
    payload: RawText<'a>,
    after: u32,
}

const ENVELOPE: &str = r#"{"kind": "x", "payload": { "a" : [1, 2.50, 1e3],
    "b": "\u0041\n" }, "after": 42}"#;
const PAYLOAD: &str = r#"{ "a" : [1, 2.50, 1e3],
    "b": "\u0041\n" }"#;
/// The payload encoded as JSON.
const ENCODED: &str = r#"{"a":[1,2.50,1e3],"b":"A\n"}"#;

#[test]
fn test_borrowed() {
    let envelope: Envelope = dialect::from_str(ENVELOPE).unwrap();
    assert_eq!(envelope.kind, "x");
    assert_eq!(envelope.after, 42);
    if KEEPS_INPUT {
        assert_eq!(envelope.payload.get(), PAYLOAD);
        assert!(envelope.payload.is_borrowed());
    } else {
        assert_eq!(envelope.payload.get(), ENCODED);
        assert!(!envelope.payload.is_borrowed());
    }
    // the value can be deserialized later
    let payload: Payload = envelope.payload.deserialize().unwrap();
    assert_eq!(
        payload,
        Payload {
            a: vec![1.0, 2.5, 1000.0],
            b: "A\n".into()
        }
    );
}

#[test]
fn test_owned() {
    #[derive(Deserialize, Debug)]
    struct Owned {
        payload: RawText<'static>,
        after: u32,
    }

    let input = ENVELOPE.to_string();
    let owned: Owned = dialect::from_str(&input).unwrap();
    drop(input);
    assert_eq!(owned.after, 42);
    assert!(!owned.payload.is_borrowed());
    assert_eq!(
        owned.payload.get(),
        if KEEPS_INPUT { PAYLOAD } else { ENCODED }
    );
    assert_eq!(owned.payload.deserialize::<Payload>().unwrap().b, "A\n");
}

#[test]
fn test_root_and_scalars() {
    for (input, raw) in [
        ("  [1, 2]  ", "[1, 2]"),
        ("{}", "{}"),
        ("[ ]", "[ ]"),
        ("\"a\\u0041\"", "\"a\\u0041\""),
        ("1.50", "1.50"),
        ("-0", "-0"),
        ("1E+5", "1E+5"),
        (
            "123456789012345678901234567890",
            "123456789012345678901234567890",
        ),
        ("true", "true"),
        ("false", "false"),
        ("null", "null"),
        ("[[[[{\"a\": [{}]}]]]]", "[[[[{\"a\": [{}]}]]]]"),
    ] {
        if DIALECT.hjson && !input.trim_start().starts_with(['[', '{', '"']) {
            continue;
        }
        let value: RawText = dialect::from_str(input).unwrap();
        if KEEPS_INPUT {
            assert_eq!(value.get(), raw, "{input}");
        }
        let value: RawText = dialect::from_slice(input.as_bytes()).unwrap();
        if KEEPS_INPUT {
            assert_eq!(value.get(), raw, "{input}");
        }
    }
}

#[test]
fn test_containers() {
    #[derive(Deserialize, Debug)]
    struct Containers<'a> {
        items: Vec<RawText<'static>>,
        map: BTreeMap<String, RawText<'static>>,
        optional: Option<RawText<'static>>,
        null: Option<RawText<'static>>,
        missing: Option<RawText<'static>>,
        boxed: Box<RawText<'static>>,
        #[deser(as = Vec<Borrowed>)]
        borrowed: Vec<RawText<'a>>,
        empty: Vec<RawText<'static>>,
        // the values after raw values are not raw
        after: Vec<u32>,
    }

    let input = r#"{
        "items": [1, [2], {"x": 3}],
        "map": {"a": [ 1 ], "b": "c"},
        "optional": {"y": null},
        "null": null,
        "boxed": [true],
        "borrowed": [1, 2],
        "empty": [],
        "after": [1, 2]
    }"#;
    let value: Containers = dialect::from_str(input).unwrap();
    assert_eq!(value.after, [1, 2]);
    assert!(value.null.is_none());
    assert!(value.missing.is_none());
    assert!(value.empty.is_empty());
    let texts = |values: &[RawText]| {
        values
            .iter()
            .map(|value| value.get().to_string())
            .collect::<Vec<_>>()
    };
    if KEEPS_INPUT {
        assert_eq!(texts(&value.items), ["1", "[2]", "{\"x\": 3}"]);
        assert_eq!(value.map["a"].get(), "[ 1 ]");
        assert_eq!(value.optional.unwrap().get(), "{\"y\": null}");
        assert_eq!(value.boxed.get(), "[true]");
    } else {
        assert_eq!(texts(&value.items), ["1", "[2]", "{\"x\":3}"]);
        assert_eq!(value.map["a"].get(), "[1]");
    }
    assert_eq!(value.map["b"].get(), "\"c\"");
    assert_eq!(texts(&value.borrowed), ["1", "2"]);
    assert_eq!(value.items[1].deserialize::<Vec<u32>>().unwrap(), [2]);
}

#[test]
fn test_not_requested() {
    // flattened fields and the variants of untagged enums are encoded
    #[derive(Deserialize, Debug)]
    struct Outer {
        #[deser(flatten)]
        inner: Inner,
    }

    #[derive(Deserialize, Debug)]
    struct Inner {
        raw: RawText<'static>,
    }

    #[derive(Deserialize, Debug)]
    #[deser(untagged)]
    enum Untagged {
        Raw { raw: RawText<'static>, x: u32 },
    }

    let value: Outer = dialect::from_str(r#"{"raw": [1,  2]}"#).unwrap();
    assert_eq!(value.inner.raw.get(), "[1,2]");
    assert_eq!(value.inner.raw.deserialize::<Vec<u32>>().unwrap(), [1, 2]);

    let Untagged::Raw { raw, x } = dialect::from_str(r#"{"raw": {"a":  1}, "x": 2}"#).unwrap();
    assert_eq!(x, 2);
    assert_eq!(raw.get(), r#"{"a":1}"#);
}

#[test]
fn test_other_format() {
    // raw JSON in a dialect is encoded as JSON (without the comments)
    #[derive(Deserialize, Debug)]
    struct Values {
        own: RawText<'static>,
        json: RawJson<'static>,
    }

    let input = if DIALECT.comments {
        "{\"own\": [1, /* one */ 2], \"json\": [1, /* one */ 2]}"
    } else {
        "{\"own\": [1,  2], \"json\": [1,  2]}"
    };
    let value: Values = dialect::from_str(input).unwrap();
    if KEEPS_INPUT {
        assert_eq!(value.own.get(), &input[8..input.find(']').unwrap() + 1]);
    }
    if IS_JSON {
        assert_eq!(value.json.get(), "[1,  2]");
    } else {
        assert_eq!(value.json.get(), "[1,2]");
    }

    // other raw values in a recording are encoded
    let recording: Recording = dialect::from_str(input).unwrap();
    let json: BTreeMap<String, RawJson> = recording_into(&recording);
    assert_eq!(json["own"].get(), "[1,2]");
}

fn recording_into<T: deser::de::DeserializeOwned>(recording: &Recording) -> T {
    let mut out = None;
    let mut state = deser::State::new();
    recording
        .replay(T::deserialize_into(&mut out, &mut state), &mut state)
        .unwrap();
    out.unwrap()
}

#[test]
fn test_invalid() {
    for input in [
        r#"{"kind": "x", "payload": [1, }, "after": 1}"#,
        r#"{"kind": "x", "payload": [1 2], "after": 1}"#,
        r#"{"kind": "x", "payload": {"a" 1}, "after": 1}"#,
        r#"{"kind": "x", "payload": {1: 1}, "after": 1}"#,
        r#"{"kind": "x", "payload": 01, "after": 1}"#,
        r#"{"kind": "x", "payload": 1., "after": 1}"#,
        r#"{"kind": "x", "payload": "\x", "after": 1}"#,
        r#"{"kind": "x", "payload": [1"#,
        r#"{"kind": "x", "payload": tru, "after": 1}"#,
    ] {
        if !IS_JSON {
            continue;
        }
        let err = dialect::from_str::<Envelope>(input).unwrap_err();
        assert!(err.line().is_some(), "{input}: {err}");
    }
    let err = dialect::from_str::<Envelope>(r#"{"kind": "x", "payload": [1, }, "after": 1}"#)
        .unwrap_err();
    assert_eq!(err.column(), Some(30), "{err}");
    // strings are validated in byte slices
    let err = dialect::from_slice::<Envelope>(
        b"{\"kind\": \"x\", \"payload\": [\"\xff\"], \"after\": 1}",
    )
    .unwrap_err();
    assert!(err.to_string().contains("utf-8"), "{err}");
}

#[test]
fn test_serialize() {
    let envelope: Envelope = dialect::from_str(ENVELOPE).unwrap();
    let json = deser_json::to_string(&envelope).unwrap();
    if IS_JSON {
        // the input is written as it is
        assert_eq!(
            json,
            format!("{{\"kind\":\"x\",\"payload\":{PAYLOAD},\"after\":42}}")
        );
    } else {
        // the value is written (the JSON serializer does not write the
        // other dialects as they are)
        assert_eq!(
            json,
            format!("{{\"kind\":\"x\",\"payload\":{ENCODED},\"after\":42}}")
        );
    }
}

#[test]
fn test_serialize_events() {
    // serializers that do not write the raw text as it is receive the value
    let raw: RawText = dialect::from_str(r#"{"a": [1, "x"]}"#).unwrap();
    let mut events = Vec::new();
    deser::ser::SerializeDriver::new(&raw)
        .drive(|event, _state| {
            events.push(event.to_static());
            Ok(())
        })
        .unwrap();
    let events = events
        .iter()
        .map(|event| format!("{event:?}"))
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 7, "{events:?}");
    assert!(events[0].starts_with("MapStart"), "{events:?}");
    assert!(events[3].contains("U64(1)"), "{events:?}");

    // pretty JSON writes raw JSON as it is too
    let pretty = deser_json::SerializerConfig::new()
        .pretty(deser_json::Indent::Spaces(2))
        .to_string(&vec![raw])
        .unwrap();
    if IS_JSON {
        assert_eq!(pretty, "[\n  {\"a\": [1, \"x\"]}\n]");
    }
}

#[test]
#[cfg(feature = "io")]
fn test_reader_in_chunks() {
    #[derive(Deserialize, Debug)]
    struct Owned {
        payload: RawText<'static>,
        after: u32,
    }

    for size in super::common::chunk_sizes(ENVELOPE.len()) {
        let value: Owned = dialect::from_reader(super::common::Chunked {
            input: ENVELOPE.as_bytes(),
            size,
        })
        .unwrap();
        assert_eq!(value.after, 42);
        assert_eq!(
            value.payload.get(),
            if KEEPS_INPUT { PAYLOAD } else { ENCODED },
            "{size}"
        );
    }
    // the items of a sequence of raw values are requested after the item
    // before them, the input can end in between
    let input = "[ 1 , {\"a\": [2]} ,\n [3, \"x\"] ]";
    for size in super::common::chunk_sizes(input.len()) {
        let items: Vec<RawText<'static>> = dialect::from_reader(super::common::Chunked {
            input: input.as_bytes(),
            size,
        })
        .unwrap();
        let items = items.iter().map(|item| item.get()).collect::<Vec<_>>();
        if KEEPS_INPUT {
            assert_eq!(items, ["1", "{\"a\": [2]}", "[3, \"x\"]"], "{size}");
        } else {
            assert_eq!(items, ["1", "{\"a\":[2]}", "[3,\"x\"]"], "{size}");
        }
    }
}

#[test]
fn test_dialect_syntax() {
    // the syntax of the dialect is kept and validated
    if !DIALECT.json5 {
        return;
    }
    #[derive(Deserialize, Debug)]
    struct Values {
        v: RawText<'static>,
        after: u32,
    }

    let value: Values =
        dialect::from_str("{v: {a: 'x', // comment\n b: 0x10, c: [+Infinity,],}, after: 1}")
            .unwrap();
    assert_eq!(
        value.v.get(),
        "{a: 'x', // comment\n b: 0x10, c: [+Infinity,],}"
    );
    assert_eq!(value.after, 1);
    let inner: BTreeMap<String, Recording> = value.v.deserialize().unwrap();
    assert_eq!(inner.len(), 3);
    assert!(dialect::from_str::<Values>("{v: {a: 'x' b: 1}, after: 1}").is_err());
    assert_eq!(
        deser_json::to_string(&value.v).unwrap(),
        r#"{"a":"x","b":16,"c":[null]}"#
    );
}

#[test]
fn test_large_struct() {
    // fields from the 64th on want raw values too
    #[derive(Deserialize, Debug)]
    struct Large {
        f0: RawText<'static>,
        f1: u32,
        f2: u32,
        f3: u32,
        f4: u32,
        f5: u32,
        f6: u32,
        f7: u32,
        f8: u32,
        f9: u32,
        f10: u32,
        f11: u32,
        f12: u32,
        f13: u32,
        f14: u32,
        f15: u32,
        f16: u32,
        f17: u32,
        f18: u32,
        f19: u32,
        f20: u32,
        f21: u32,
        f22: u32,
        f23: u32,
        f24: u32,
        f25: u32,
        f26: u32,
        f27: u32,
        f28: u32,
        f29: u32,
        f30: u32,
        f31: u32,
        f32: u32,
        f33: u32,
        f34: u32,
        f35: u32,
        f36: u32,
        f37: u32,
        f38: u32,
        f39: u32,
        f40: u32,
        f41: u32,
        f42: u32,
        f43: u32,
        f44: u32,
        f45: u32,
        f46: u32,
        f47: u32,
        f48: u32,
        f49: u32,
        f50: u32,
        f51: u32,
        f52: u32,
        f53: u32,
        f54: u32,
        f55: u32,
        f56: u32,
        f57: u32,
        f58: u32,
        f59: u32,
        f60: u32,
        f61: u32,
        f62: RawText<'static>,
        f63: RawText<'static>,
        f64: u32,
        f65: u32,
        f66: u32,
        f67: u32,
        f68: u32,
        f69: RawText<'static>,
    }

    let input = r#"{"f0": [ 0 ], "f1": 1, "f2": 2, "f3": 3, "f4": 4, "f5": 5, "f6": 6, "f7": 7, "f8": 8, "f9": 9, "f10": 10, "f11": 11, "f12": 12, "f13": 13, "f14": 14, "f15": 15, "f16": 16, "f17": 17, "f18": 18, "f19": 19, "f20": 20, "f21": 21, "f22": 22, "f23": 23, "f24": 24, "f25": 25, "f26": 26, "f27": 27, "f28": 28, "f29": 29, "f30": 30, "f31": 31, "f32": 32, "f33": 33, "f34": 34, "f35": 35, "f36": 36, "f37": 37, "f38": 38, "f39": 39, "f40": 40, "f41": 41, "f42": 42, "f43": 43, "f44": 44, "f45": 45, "f46": 46, "f47": 47, "f48": 48, "f49": 49, "f50": 50, "f51": 51, "f52": 52, "f53": 53, "f54": 54, "f55": 55, "f56": 56, "f57": 57, "f58": 58, "f59": 59, "f60": 60, "f61": 61, "f62": [ 62 ], "f63": [ 63 ], "f64": 64, "f65": 65, "f66": 66, "f67": 67, "f68": 68, "f69": [ 69 ]}"#;
    let value: Large = dialect::from_str(input).unwrap();
    for (raw, index) in [
        (&value.f0, 0),
        (&value.f62, 62),
        (&value.f63, 63),
        (&value.f69, 69),
    ] {
        if KEEPS_INPUT {
            assert_eq!(raw.get(), format!("[ {index} ]"));
        } else {
            assert_eq!(raw.get(), format!("[{index}]"));
        }
    }
    assert_eq!(value.f1, 1);
    assert_eq!(value.f64, 64);
    assert_eq!(value.f68, 68);
}

#[test]
fn test_input_in_other_types() {
    use deser::ext::{ExtValue, RawFormat, RawInput};
    use deser::{Atom, Event};

    // the input of a raw value is parsed into types that do not take it
    let json = r#"{"a": [1, 2], "b": "x"}"#;
    // SAFETY: the input is valid "f0": [ 0 ], "f1": 1, "f2": 2, "f3": 3, "f4": 4, "f5": 5, "f6": 6, "f7": 7, "f8": 8, "f9": 9, "f10": 10, "f11": 11, "f12": 12, "f13": 13, "f14": 14, "f15": 15, "f16": 16, "f17": 17, "f18": 18, "f19": 19, "f20": 20, "f21": 21, "f22": 22, "f23": 23, "f24": 24, "f25": 25, "f26": 26, "f27": 27, "f28": 28, "f29": 29, "f30": 30, "f31": 31, "f32": 32, "f33": 33, "f34": 34, "f35": 35, "f36": 36, "f37": 37, "f38": 38, "f39": 39, "f40": 40, "f41": 41, "f42": 42, "f43": 43, "f44": 44, "f45": 45, "f46": 46, "f47": 47, "f48": 48, "f49": 49, "f50": 50, "f51": 51, "f52": 52, "f53": 53, "f54": 54, "f55": 55, "f56": 56, "f57": 57, "f58": 58, "f59": 59, "f60": 60, "f61": 61, "f62": [ 62 ], "f63": [ 63 ], "f64": 64, "f65": 65, "f66": 66, "f67": 67, "f68": 68, "f69": [ 69 ]
    let input = unsafe { RawInput::new(json.as_bytes(), deser_json::Json::info()) };

    #[derive(Deserialize, Debug, PartialEq)]
    struct Target {
        a: Vec<u32>,
        b: String,
    }

    let mut out = None::<Target>;
    {
        let mut driver = deser::de::DeserializeDriver::new(&mut out);
        driver
            .emit(Atom::Ext(ExtValue::borrowed_value::<RawInput>(&input)))
            .unwrap();
    }
    assert_eq!(
        out.unwrap(),
        Target {
            a: vec![1, 2],
            b: "x".into()
        }
    );

    // also in containers
    let mut out = None::<Vec<Target>>;
    {
        let mut driver = deser::de::DeserializeDriver::new(&mut out);
        driver.emit(Event::seq_start()).unwrap();
        driver
            .emit(Atom::Ext(ExtValue::borrowed_value::<RawInput>(&input)))
            .unwrap();
        driver
            .emit(Atom::Ext(ExtValue::borrowed_value::<RawInput>(&input)))
            .unwrap();
        driver.emit(Event::SeqEnd).unwrap();
    }
    assert_eq!(out.unwrap().len(), 2);

    // null is null for optionals
    // SAFETY: the input is valid "f0": [ 0 ], "f1": 1, "f2": 2, "f3": 3, "f4": 4, "f5": 5, "f6": 6, "f7": 7, "f8": 8, "f9": 9, "f10": 10, "f11": 11, "f12": 12, "f13": 13, "f14": 14, "f15": 15, "f16": 16, "f17": 17, "f18": 18, "f19": 19, "f20": 20, "f21": 21, "f22": 22, "f23": 23, "f24": 24, "f25": 25, "f26": 26, "f27": 27, "f28": 28, "f29": 29, "f30": 30, "f31": 31, "f32": 32, "f33": 33, "f34": 34, "f35": 35, "f36": 36, "f37": 37, "f38": 38, "f39": 39, "f40": 40, "f41": 41, "f42": 42, "f43": 43, "f44": 44, "f45": 45, "f46": 46, "f47": 47, "f48": 48, "f49": 49, "f50": 50, "f51": 51, "f52": 52, "f53": 53, "f54": 54, "f55": 55, "f56": 56, "f57": 57, "f58": 58, "f59": 59, "f60": 60, "f61": 61, "f62": [ 62 ], "f63": [ 63 ], "f64": 64, "f65": 65, "f66": 66, "f67": 67, "f68": 68, "f69": [ 69 ]
    let null = unsafe { RawInput::new(&b"null"[..], deser_json::Json::info()) };
    let mut out = None::<Option<Target>>;
    {
        let mut driver = deser::de::DeserializeDriver::new(&mut out);
        driver
            .emit(Atom::Ext(ExtValue::borrowed_value::<RawInput>(&null)))
            .unwrap();
    }
    assert_eq!(out.unwrap(), None);
}
