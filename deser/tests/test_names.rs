use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ser::SerializeDriver;
use deser::{Deserialize, Error, Event, Serialize};

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

fn serialize<T: Serialize>(value: &T) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(value);
    while let Some((event, _, _)) = driver.next().unwrap() {
        // the shapes are not compared
        events.push(match event {
            Event::MapStart(_) => Event::map_start(),
            event => event.to_static(),
        });
    }
    events
}

fn map<'a>(pairs: &[(&'a str, Event<'a>)]) -> Vec<Event<'a>> {
    let mut events = vec![Event::map_start()];
    for (key, value) in pairs {
        events.push((*key).into());
        events.push(value.clone());
    }
    events.push(Event::MapEnd);
    events
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(
    rename_all = "camelCase",
    alias_all = "snake_case",
    alias_all = "PascalCase"
)]
struct Settings {
    max_items: u32,
    #[deser(rename = "enabled")]
    is_enabled: bool,
}

#[test]
fn test_alias_all() {
    let expected = Settings {
        max_items: 1,
        is_enabled: true,
    };
    for (max_items, is_enabled) in [
        ("maxItems", "enabled"),
        ("max_items", "is_enabled"),
        ("MaxItems", "IsEnabled"),
    ] {
        let value: Settings =
            deserialize(map(&[(max_items, 1u64.into()), (is_enabled, true.into())])).unwrap();
        assert_eq!(value, expected);
    }
    // the name is used for serialization
    assert_eq!(
        serialize(&expected),
        map(&[("maxItems", 1u64.into()), ("enabled", true.into())])
    );
    // the aliases are the same field
    let rv = deserialize::<Settings>(map(&[
        ("maxItems", 1u64.into()),
        ("max_items", 2u64.into()),
        ("enabled", true.into()),
    ]));
    assert_eq!(rv.unwrap_err().message(), "duplicate field `maxItems`");
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(alias_all = "lowercase", tag = "type")]
enum Command {
    Start { delay: u32 },
    Stop,
}

#[test]
fn test_alias_all_variants() {
    assert_eq!(
        deserialize::<Command>(map(&[("type", "start".into()), ("delay", 1u64.into())])).unwrap(),
        Command::Start { delay: 1 }
    );
    assert_eq!(
        deserialize::<Command>(map(&[("type", "Stop".into())])).unwrap(),
        Command::Stop
    );
    assert_eq!(serialize(&Command::Stop), map(&[("type", "Stop".into())]));
}

mod names {
    pub const ID: &str = "@id";
    pub const PREFIX_NAME: &str = concat!("x-", "name");
}

const TYPE_NAME: &str = "Resource";

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(rename = TYPE_NAME)]
struct Resource {
    #[deser(rename = names::ID)]
    id: u32,
    #[deser(rename = names::PREFIX_NAME, alias = concat!("x-", "title"))]
    name: String,
    #[deser(rename = stringify!(r#type))]
    kind: String,
}

#[test]
fn test_expressions() {
    let value = Resource {
        id: 1,
        name: "a".into(),
        kind: "b".into(),
    };
    let events = map(&[
        ("@id", 1u64.into()),
        ("x-name", "a".into()),
        ("r#type", "b".into()),
    ]);
    assert_eq!(serialize(&value), events);
    assert_eq!(deserialize::<Resource>(events).unwrap(), value);
    assert_eq!(
        deserialize::<Resource>(map(&[
            ("@id", 1u64.into()),
            ("x-title", "a".into()),
            ("r#type", "b".into()),
        ]))
        .unwrap(),
        value
    );
    let rv = deserialize::<Resource>(map(&[("@id", 1u64.into())]));
    assert_eq!(rv.unwrap_err().message(), "missing field `x-name`");
    let rv = deserialize::<Resource>(map(&[("@id", true.into())]));
    assert_eq!(rv.unwrap_err().message(), "unexpected bool, expected u32");
    // the type name is used in errors
    let rv = deserialize::<Resource>(vec![true.into()]);
    assert_eq!(
        rv.unwrap_err().message(),
        "unexpected bool, expected Resource"
    );
}

const LOWER: &str = "lower";

#[derive(Debug, Serialize, Deserialize, PartialEq)]
enum Case {
    #[deser(rename = LOWER)]
    Lower,
    #[deser(rename = concat!("UP", "PER"))]
    Upper { value: u32 },
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(tag = "t")]
enum TaggedCase {
    #[deser(rename = LOWER, alias = "l")]
    Lower,
    #[deser(rename = concat!("UP", "PER"))]
    Upper { value: u32 },
}

#[test]
fn test_variant_expressions() {
    assert_eq!(serialize(&Case::Lower), vec![Event::from("lower")]);
    assert_eq!(
        deserialize::<Case>(vec!["lower".into()]).unwrap(),
        Case::Lower
    );
    let upper = Case::Upper { value: 1 };
    let mut events = vec![Event::map_start(), "UPPER".into()];
    events.extend(map(&[("value", 1u64.into())]));
    events.push(Event::MapEnd);
    assert_eq!(serialize(&upper), events);
    assert_eq!(deserialize::<Case>(events).unwrap(), upper);
    assert_eq!(
        deserialize::<Case>(vec!["x".into()]).unwrap_err().message(),
        "unknown variant `x` of Case, expected `lower` or `UPPER`"
    );

    assert_eq!(
        deserialize::<TaggedCase>(map(&[("t", "l".into())])).unwrap(),
        TaggedCase::Lower
    );
    assert_eq!(
        serialize(&TaggedCase::Upper { value: 1 }),
        map(&[("t", "UPPER".into()), ("value", 1u64.into())])
    );
    assert_eq!(
        deserialize::<TaggedCase>(map(&[("t", "UPPER".into()), ("value", 1u64.into())])).unwrap(),
        TaggedCase::Upper { value: 1 }
    );
}
