use std::collections::BTreeMap;

use deser::de::{DeserializeDriver, DeserializeOwned, Recording};
use deser::{Atom, Deserialize, Error, ErrorKind, Event, Implicit, ImplicitValue};

fn implicit(text: &str, value: ImplicitValue) -> Event<'_> {
    Event::Atom(Atom::Implicit(Implicit::new(text, value)))
}

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in events {
            driver.emit(event)?;
        }
    }
    Ok(out.unwrap())
}

#[test]
fn test_value_or_text() {
    let hex = || implicit("0x1F", ImplicitValue::U64(31));
    assert_eq!(deserialize::<u32>(vec![hex()]).unwrap(), 31);
    assert_eq!(deserialize::<f64>(vec![hex()]).unwrap(), 31.0);
    assert_eq!(deserialize::<String>(vec![hex()]).unwrap(), "0x1F");
    assert_eq!(
        deserialize::<std::path::PathBuf>(vec![hex()]).unwrap(),
        std::path::PathBuf::from("0x1F")
    );
    let float = || implicit("1.10", ImplicitValue::F64(1.1));
    assert_eq!(deserialize::<f64>(vec![float()]).unwrap(), 1.1);
    assert_eq!(deserialize::<String>(vec![float()]).unwrap(), "1.10");
    assert_eq!(
        deserialize::<Option<String>>(vec![float()]).unwrap(),
        Some("1.10".into())
    );
}

#[test]
fn test_null() {
    let null = || implicit("~", ImplicitValue::Null);
    assert_eq!(deserialize::<Option<String>>(vec![null()]).unwrap(), None);
    assert_eq!(deserialize::<Option<u32>>(vec![null()]).unwrap(), None);
    assert_eq!(deserialize::<String>(vec![null()]).unwrap(), "~");
    deserialize::<()>(vec![null()]).unwrap();

    #[derive(Deserialize, Debug, PartialEq)]
    struct Unit;
    assert_eq!(deserialize::<Unit>(vec![null()]).unwrap(), Unit);
}

#[test]
fn test_errors() {
    // the error of the value is reported if the text is rejected too
    let err = deserialize::<bool>(vec![implicit("1", ImplicitValue::U64(1))]).unwrap_err();
    assert_eq!(err.message(), "unexpected unsigned integer, expected bool");
    let err = deserialize::<Vec<u32>>(vec![implicit("1", ImplicitValue::U64(1))]).unwrap_err();
    assert_eq!(err.message(), "unexpected unsigned integer, expected vec");
    // errors of values of the right type are not retried
    let err = deserialize::<u8>(vec![implicit("300", ImplicitValue::U64(300))]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);
}

#[test]
fn test_keys() {
    let events = vec![
        Event::map_start(),
        implicit("200", ImplicitValue::U64(200)),
        "ok".into(),
        implicit("true", ImplicitValue::Bool(true)),
        "yes".into(),
        Event::MapEnd,
    ];
    let map = deserialize::<BTreeMap<String, String>>(events.clone()).unwrap();
    assert_eq!(map["200"], "ok");
    assert_eq!(map["true"], "yes");

    #[derive(Deserialize, Debug, PartialEq)]
    struct Fields {
        #[deser(rename = "200")]
        ok: String,
        #[deser(rename = "true")]
        yes: String,
    }
    let fields = deserialize::<Fields>(events).unwrap();
    assert_eq!(fields.ok, "ok");
    assert_eq!(fields.yes, "yes");
}

#[test]
fn test_enums() {
    #[derive(Deserialize, Debug, PartialEq)]
    enum Tag {
        #[deser(rename = 1)]
        One,
        #[deser(rename = "1.0")]
        Float,
        #[deser(rename = "yes")]
        Yes,
    }
    let tag = |text, value| deserialize::<Tag>(vec![implicit(text, value)]);
    // the value is looked up first, then the text
    assert_eq!(tag("1", ImplicitValue::U64(1)).unwrap(), Tag::One);
    assert_eq!(tag("0x1", ImplicitValue::U64(1)).unwrap(), Tag::One);
    assert_eq!(tag("1.0", ImplicitValue::F64(1.0)).unwrap(), Tag::Float);
    assert_eq!(tag("yes", ImplicitValue::Bool(true)).unwrap(), Tag::Yes);
    let err = tag("no", ImplicitValue::Bool(false)).unwrap_err();
    assert_eq!(
        err.message(),
        "unknown variant `no` of Tag, expected one of `1`, `1.0`, `yes`"
    );

    #[derive(Deserialize, Debug, PartialEq)]
    #[deser(untagged)]
    enum Version {
        Number(u32),
        Text(String),
    }
    assert_eq!(
        deserialize::<Version>(vec![implicit("42", ImplicitValue::U64(42))]).unwrap(),
        Version::Number(42)
    );
    assert_eq!(
        deserialize::<Version>(vec![implicit("1.10", ImplicitValue::F64(1.1))]).unwrap(),
        Version::Text("1.10".into())
    );
}

#[test]
fn test_borrowed() {
    let input = String::from("0x1F");
    let mut out = None::<&str>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver
            .emit_borrowed(Atom::Implicit(Implicit::new(
                input.as_str(),
                ImplicitValue::U64(31),
            )))
            .unwrap();
    }
    assert_eq!(out, Some("0x1F"));
    assert!(
        input
            .as_bytes()
            .as_ptr_range()
            .contains(&out.unwrap().as_ptr())
    );
}

#[test]
fn test_recording() {
    let recording =
        deserialize::<Recording>(vec![implicit("1.10", ImplicitValue::F64(1.1))]).unwrap();
    let mut driver_out = None::<()>;
    let mut driver = DeserializeDriver::new(&mut driver_out);
    let mut text = None::<String>;
    recording
        .replay(
            Deserialize::deserialize_into(&mut text, driver.state_mut()),
            driver.state_mut(),
        )
        .unwrap();
    assert_eq!(text.as_deref(), Some("1.10"));
    let mut value = None::<f64>;
    recording
        .replay(
            Deserialize::deserialize_into(&mut value, driver.state_mut()),
            driver.state_mut(),
        )
        .unwrap();
    assert_eq!(value, Some(1.1));
}

#[test]
fn test_atom() {
    let atom = Atom::Implicit(Implicit::new("0x1F", ImplicitValue::U64(31)));
    assert_eq!(atom.name(), "unsigned integer");
    assert_eq!(atom.as_str(), None);
    assert_eq!(atom.to_static(), atom);
    assert_eq!(
        ImplicitValue::from_atom(&Atom::F64(1.5)),
        Some(ImplicitValue::F64(1.5))
    );
    assert_eq!(ImplicitValue::from_atom(&Atom::Str("x".into())), None);
    assert_eq!(std::mem::size_of::<Atom>(), 32);
}
