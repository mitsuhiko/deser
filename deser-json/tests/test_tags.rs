//! Variants named by integers and booleans.
use std::collections::BTreeMap;

use deser::{Deserialize, Serialize};
use deser_json::{from_str, to_string};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    #[deser(rename = 0)]
    Off,
    #[deser(rename = 1, alias = "low")]
    Low,
    #[deser(rename = -1)]
    Negative,
    #[deser(rename = "high")]
    High,
}

#[test]
fn test_unit_enums() {
    assert_eq!(to_string(&Level::Off).unwrap(), "0");
    assert_eq!(to_string(&Level::Negative).unwrap(), "-1");
    assert_eq!(to_string(&Level::High).unwrap(), r#""high""#);
    assert_eq!(from_str::<Level>("0").unwrap(), Level::Off);
    assert_eq!(from_str::<Level>("1").unwrap(), Level::Low);
    assert_eq!(from_str::<Level>("-1").unwrap(), Level::Negative);
    assert_eq!(from_str::<Level>(r#""low""#).unwrap(), Level::Low);
    assert_eq!(from_str::<Level>(r#""high""#).unwrap(), Level::High);

    // strings are not integers
    assert_eq!(
        from_str::<Level>(r#""1""#).unwrap_err().message(),
        "unknown variant `1` of Level, expected one of `0`, `1`, `-1`, `high`"
    );
    assert_eq!(
        from_str::<Level>("2").unwrap_err().message(),
        "unknown variant `2` of Level, expected one of `0`, `1`, `-1`, `high`"
    );
    assert_eq!(
        from_str::<Level>("1.5").unwrap_err().message(),
        "unexpected float, expected Level"
    );

    // the keys of JSON objects are text of unknown type
    let map: BTreeMap<Level, u32> = from_str(r#"{"0": 1, "high": 2, "low": 3}"#).unwrap();
    assert_eq!(
        map.into_iter().collect::<Vec<_>>(),
        [(Level::Off, 1), (Level::Low, 3), (Level::High, 2)]
    );
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(tag = "version")]
enum Message {
    #[deser(rename = 1)]
    V1 { text: String },
    #[deser(rename = 2)]
    V2 { text: String, lang: String },
    #[deser(other)]
    Unknown(#[deser(tag)] u64, deser::de::Recording),
}

#[test]
fn test_internally_tagged() {
    let message = Message::V1 { text: "hi".into() };
    let json = to_string(&message).unwrap();
    assert_eq!(json, r#"{"version":1,"text":"hi"}"#);
    assert_eq!(from_str::<Message>(&json).unwrap(), message);
    assert_eq!(
        from_str::<Message>(r#"{"text":"hi","lang":"en","version":2}"#).unwrap(),
        Message::V2 {
            text: "hi".into(),
            lang: "en".into()
        }
    );
    let unknown = from_str::<Message>(r#"{"version":3,"x":1}"#).unwrap();
    assert!(matches!(unknown, Message::Unknown(3, _)));
    assert_eq!(to_string(&unknown).unwrap(), r#"{"version":3,"x":1}"#);
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(tag = "ok", content = "value")]
enum Outcome {
    #[deser(rename = true)]
    Success(u32),
    #[deser(rename = false)]
    Failure(String),
}

#[test]
fn test_adjacently_tagged() {
    let json = to_string(&Outcome::Success(1)).unwrap();
    assert_eq!(json, r#"{"ok":true,"value":1}"#);
    assert_eq!(from_str::<Outcome>(&json).unwrap(), Outcome::Success(1));
    assert_eq!(
        from_str::<Outcome>(r#"{"value":"x","ok":false}"#).unwrap(),
        Outcome::Failure("x".into())
    );
    assert_eq!(
        from_str::<Outcome>(r#"{"ok":"yes","value":1}"#)
            .unwrap_err()
            .message(),
        "unknown variant `yes` of Outcome, expected `true` or `false`"
    );
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
enum Code {
    #[deser(rename = 200)]
    Ok { body: String },
    #[deser(rename = 404)]
    NotFound,
}

#[test]
fn test_externally_tagged() {
    let json = to_string(&Code::Ok { body: "x".into() }).unwrap();
    assert_eq!(json, r#"{"200":{"body":"x"}}"#);
    assert_eq!(
        from_str::<Code>(&json).unwrap(),
        Code::Ok { body: "x".into() }
    );
    assert_eq!(to_string(&Code::NotFound).unwrap(), "404");
    assert_eq!(from_str::<Code>("404").unwrap(), Code::NotFound);
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[deser(repr)]
#[repr(i16)]
enum Priority {
    Lowest = -2,
    Low,
    Normal,
    High = 10,
    #[deser(alias = "urgent")]
    Urgent,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(repr)]
enum ReprCode {
    A,
    B,
    #[deser(other)]
    Unknown,
}

#[test]
fn test_repr() {
    for (value, json) in [
        (Priority::Lowest, "-2"),
        (Priority::Low, "-1"),
        (Priority::Normal, "0"),
        (Priority::High, "10"),
        (Priority::Urgent, "11"),
    ] {
        assert_eq!(to_string(&value).unwrap(), json);
        assert_eq!(from_str::<Priority>(json).unwrap(), value);
    }
    // aliases can be strings, names are not used
    assert_eq!(
        from_str::<Priority>(r#""urgent""#).unwrap(),
        Priority::Urgent
    );
    assert_eq!(
        from_str::<Priority>(r#""Normal""#).unwrap_err().message(),
        "unknown variant `Normal` of Priority, expected one of `-2`, `-1`, `0`, `10`, `11`"
    );
    assert_eq!(
        from_str::<Priority>("1").unwrap_err().message(),
        "unknown variant `1` of Priority, expected one of `-2`, `-1`, `0`, `10`, `11`"
    );
    // keys of objects are parsed as integers
    let map: BTreeMap<Priority, u32> = from_str(r#"{"10": 1, "-2": 2}"#).unwrap();
    assert_eq!(
        map,
        BTreeMap::from([(Priority::Lowest, 2), (Priority::High, 1)])
    );

    assert_eq!(to_string(&ReprCode::Unknown).unwrap(), "2");
    assert_eq!(from_str::<ReprCode>("1").unwrap(), ReprCode::B);
    assert_eq!(from_str::<ReprCode>("7").unwrap(), ReprCode::Unknown);
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(repr, tag = "version")]
#[repr(u8)]
enum Versioned {
    V1 { text: String } = 1,
    V2 { text: String, lang: String } = 2,
}

#[test]
fn test_repr_data_enum() {
    let value = Versioned::V2 {
        text: "a".into(),
        lang: "en".into(),
    };
    let json = r#"{"version":2,"text":"a","lang":"en"}"#;
    assert_eq!(to_string(&value).unwrap(), json);
    assert_eq!(from_str::<Versioned>(json).unwrap(), value);
    assert_eq!(
        from_str::<Versioned>(r#"{"version":1,"text":"a"}"#).unwrap(),
        Versioned::V1 { text: "a".into() }
    );
}
