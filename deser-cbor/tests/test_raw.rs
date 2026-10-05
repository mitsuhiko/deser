//! Raw CBOR values.
use std::collections::BTreeMap;

use crate::common::{hex, to_hex};
use deser::adapters::Borrowed;
use deser::{Deserialize, Serialize};
use deser_cbor::{RawCbor, SerializerConfig};
use deser_json::RawJson;

#[derive(Debug, Deserialize, Serialize)]
struct Envelope<'a> {
    kind: String,
    #[deser(as = Borrowed)]
    payload: RawCbor<'a>,
    after: u32,
}

/// `{"kind": "x", "payload": 42([_ 1, 2]), "after": 42}` with the tagged
/// indefinite length array encoding 1 in two bytes.
const ENVELOPE: &str = "a3 64 6b696e64 61 78 67 7061796c6f6164 d82a 9f 1801 02 ff \
                        65 6166746572 182a";
const PAYLOAD: &str = "d82a9f180102ff";

#[test]
fn test_borrowed() {
    let input = hex(ENVELOPE);
    let envelope: Envelope = deser_cbor::from_slice(&input).unwrap();
    assert_eq!(envelope.kind, "x");
    assert_eq!(envelope.after, 42);
    // the encoding is kept as it is (with the tag and the lengths)
    assert_eq!(to_hex(envelope.payload.as_bytes()), PAYLOAD);
    assert!(envelope.payload.is_borrowed());
    assert_eq!(envelope.payload.deserialize::<Vec<u64>>().unwrap(), [1, 2]);
}

#[test]
fn test_owned() {
    #[derive(Debug, Deserialize)]
    struct Owned {
        payload: RawCbor<'static>,
    }

    let input = hex(ENVELOPE);
    let owned: Owned = deser_cbor::from_slice(&input).unwrap();
    drop(input);
    assert_eq!(to_hex(owned.payload.as_bytes()), PAYLOAD);
    assert!(!owned.payload.is_borrowed());
}

#[test]
fn test_serialize() {
    let input = hex(ENVELOPE);
    let envelope: Envelope = deser_cbor::from_slice(&input).unwrap();
    // written as it is
    assert_eq!(deser_cbor::to_vec(&envelope).unwrap(), input);
    // canonical output encodes the value again (with its tag)
    let canonical = SerializerConfig::builder()
        .canonical(true)
        .build()
        .to_vec(&envelope)
        .unwrap();
    assert!(
        to_hex(&canonical).contains("d82a820102"),
        "{}",
        to_hex(&canonical)
    );
    // other formats write the value
    assert_eq!(
        deser_json::to_string(&envelope).unwrap(),
        r#"{"kind":"x","payload":[1,2],"after":42}"#
    );
}

#[test]
fn test_other_formats() {
    // raw CBOR of JSON input is encoded as CBOR
    let envelope: Envelope =
        deser_json::from_str(r#"{"kind": "x", "payload": [1,  2], "after": 42}"#).unwrap();
    assert_eq!(to_hex(envelope.payload.as_bytes()), "820102");
    assert!(!envelope.payload.is_borrowed());

    // raw JSON of CBOR input is encoded as JSON
    #[derive(Debug, Deserialize)]
    struct Json {
        payload: RawJson<'static>,
    }
    let json: Json = deser_cbor::from_slice(&hex(ENVELOPE)).unwrap();
    assert_eq!(json.payload.get(), "[1,2]");

    // and written as CBOR
    assert_eq!(
        to_hex(&deser_cbor::to_vec(&json.payload).unwrap()),
        "820102"
    );
}

#[test]
fn test_containers() {
    #[derive(Debug, Deserialize)]
    struct Containers {
        items: Vec<RawCbor<'static>>,
        map: BTreeMap<String, RawCbor<'static>>,
        null: Option<RawCbor<'static>>,
        missing: Option<RawCbor<'static>>,
        empty: Vec<RawCbor<'static>>,
        after: Vec<u32>,
    }

    // {"items": [1, [_ 2], "x"], "map": {"a": h'00'}, "null": null,
    //  "empty": [], "after": [1]}
    let input = hex(concat!(
        "a5",
        "65 6974656d73 83 01 9f 02 ff 61 78",
        "63 6d6170 a1 61 61 41 00",
        "64 6e756c6c f6",
        "65 656d707479 80",
        "65 6166746572 81 01",
    ));
    let value: Containers = deser_cbor::from_slice(&input).unwrap();
    let items: Vec<String> = value.items.iter().map(|x| to_hex(x.as_bytes())).collect();
    assert_eq!(items, ["01", "9f02ff", "6178"]);
    assert_eq!(to_hex(value.map["a"].as_bytes()), "4100");
    assert!(value.null.is_none());
    assert!(value.missing.is_none());
    assert!(value.empty.is_empty());
    assert_eq!(value.after, [1]);
}

#[test]
fn test_invalid() {
    for (payload, msg) in [
        // truncated
        ("82 01", "end of"),
        // an invalid simple value
        ("f8 10", "invalid simple value"),
        // a map without the value of its last key
        ("bf 01 ff", "missing map value"),
        // a break outside of a container
        ("ff", "unexpected break"),
        // invalid UTF-8 in text
        ("62 c328", "UTF-8"),
    ] {
        // {"kind": "x", "payload": ...}
        let input = hex(&format!("a2 64 6b696e64 61 78 67 7061796c6f6164 {payload}"));
        let err = deser_cbor::from_slice::<Envelope>(&input).unwrap_err();
        assert!(err.to_string().contains(msg), "{payload}: {err}");
    }
}

#[test]
#[cfg(feature = "io")]
fn test_reader_in_chunks() {
    struct Chunked<'a>(&'a [u8]);

    impl std::io::Read for Chunked<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let len = buf.len().min(self.0.len()).min(1);
            buf[..len].copy_from_slice(&self.0[..len]);
            self.0 = &self.0[len..];
            Ok(len)
        }
    }

    #[derive(Debug, Deserialize)]
    struct Owned {
        payload: RawCbor<'static>,
        after: u32,
    }

    let input = hex(ENVELOPE);
    let value: Owned = deser_cbor::from_reader(Chunked(&input)).unwrap();
    assert_eq!(to_hex(value.payload.as_bytes()), PAYLOAD);
    assert_eq!(value.after, 42);
}

/// A raw format with the identity of JSON (a text format) and the scanner
/// of Cbor (see `test_foreign_raw_request`).
struct Disguised;

impl deser::ext::RawFormat for Disguised {
    fn info() -> &'static deser::ext::RawFormatInfo {
        use deser::ext::RawFormatInfo;
        use deser_json::Json;

        fn replay<'de>(
            _input: &'de [u8],
            _driver: &mut deser::de::DeserializeDriver<'_, 'de>,
        ) -> Result<(), deser::Error> {
            Ok(())
        }
        fn encode(_value: deser::ser::SerializeRef<'_>) -> Result<Vec<u8>, deser::Error> {
            Ok(b"null".to_vec())
        }
        fn fallback(_input: &[u8]) -> deser::Atom<'_> {
            deser::Atom::Null
        }

        static INFO: std::sync::OnceLock<RawFormatInfo> = std::sync::OnceLock::new();
        INFO.get_or_init(|| {
            let mut info = RawFormatInfo::new(Json::info().id(), replay, encode, fallback);
            info.set_data(deser_cbor::Cbor::info().data().unwrap());
            info
        })
    }
}

/// Declares JSON as the format of the raw values before every event.
struct DeclareJson;

impl deser::de::Layer for DeclareJson {
    fn event<'de>(
        &mut self,
        event: deser::de::LayerEvent<'_, 'de>,
        next: &mut deser::de::Next<'_, 'de>,
    ) -> Result<(), deser::Error> {
        use deser::ext::RawFormat;
        next.state_mut()
            .declare_raw_format(deser_json::Json::info().id());
        next.emit(event)
    }
}

#[test]
fn test_foreign_raw_request() {
    // Requests for raw values of other formats are not passed on: the
    // input of Cbor (bytes which are not UTF-8, `[h'fffe']`) must
    // never be emitted with the description of a text format, the JSON
    // serializer would write it as text.
    let input = [0x81, 0x42, 0xff, 0xfe];
    let rv = deser_cbor::Deserializer::from_slice(&input)
        .deserialize_with::<Vec<deser::ext::Raw<'static, Disguised>>, _>(|driver| {
            driver.push_layer(DeclareJson)
        });
    match rv {
        Ok(values) => {
            let text = deser_json::to_string(&values).unwrap();
            assert!(std::str::from_utf8(text.as_bytes()).is_ok());
        }
        Err(err) => assert!(err.to_string().contains("raw value"), "{err}"),
    }
}
