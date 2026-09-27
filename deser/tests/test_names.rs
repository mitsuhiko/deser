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

const KIND: &str = "kind";

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(tag = "type", tag_alias = KIND, tag_alias = "t")]
enum Internal {
    Circle { radius: u32 },
    Empty,
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(tag = "outer")]
enum Outer {
    Inner(Internal),
}

#[test]
fn test_tag_aliases() {
    let circle = Internal::Circle { radius: 1 };
    // the name is used for serialization
    assert_eq!(
        serialize(&circle),
        map(&[("type", "Circle".into()), ("radius", 1u64.into())])
    );
    for tag in ["type", "kind", "t"] {
        // the tag first and last
        let rv = deserialize::<Internal>(map(&[(tag, "Circle".into()), ("radius", 1u64.into())]));
        assert_eq!(rv.unwrap(), circle);
        let rv = deserialize::<Internal>(map(&[("radius", 1u64.into()), (tag, "Circle".into())]));
        assert_eq!(rv.unwrap(), circle);
        let rv = deserialize::<Internal>(map(&[(tag, "Empty".into())]));
        assert_eq!(rv.unwrap(), Internal::Empty);
    }
    // errors name the tag by its name
    let rv = deserialize::<Internal>(map(&[("radius", 1u64.into())]));
    assert_eq!(rv.unwrap_err().message(), "missing tag `type`");
    let rv = deserialize::<Internal>(map(&[
        ("radius", 1u64.into()),
        ("kind", "Circle".into()),
        ("type", "Circle".into()),
    ]));
    assert_eq!(rv.unwrap_err().message(), "duplicate tag `type`");
    // also once the variant is known
    let events = map(&[
        ("kind", "Circle".into()),
        ("radius", 1u64.into()),
        ("t", "Empty".into()),
    ]);
    let rv = deserialize::<Internal>(events.clone());
    assert_eq!(rv.unwrap_err().message(), "duplicate tag `type`");
    // and in the content of internally tagged enums: the keys before
    // the outer tag are passed to the inner enum by `next_key`
    let rv = deserialize::<Outer>(map(&[
        ("kind", "Circle".into()),
        ("radius", 1u64.into()),
        ("t", "Empty".into()),
        ("outer", "Inner".into()),
    ]));
    assert_eq!(rv.unwrap_err().message(), "duplicate tag `type`");
    let rv = deserialize::<Outer>(map(&[
        ("kind", "Circle".into()),
        ("radius", 1u64.into()),
        ("outer", "Inner".into()),
    ]));
    assert_eq!(rv.unwrap(), Outer::Inner(circle));
}

#[derive(Debug, Deserialize, PartialEq)]
struct Flattened {
    name: String,
    #[deser(flatten)]
    shape: Internal,
}

#[test]
fn test_tag_aliases_flattened() {
    let expected = Flattened {
        name: "a".into(),
        shape: Internal::Circle { radius: 1 },
    };
    for events in [
        map(&[
            ("name", "a".into()),
            ("kind", "Circle".into()),
            ("radius", 1u64.into()),
        ]),
        map(&[
            ("radius", 1u64.into()),
            ("t", "Circle".into()),
            ("name", "a".into()),
        ]),
    ] {
        assert_eq!(deserialize::<Flattened>(events).unwrap(), expected);
    }
    let rv = deserialize::<Flattened>(map(&[
        ("name", "a".into()),
        ("t", "Circle".into()),
        ("type", "Circle".into()),
        ("radius", 1u64.into()),
    ]));
    assert_eq!(rv.unwrap_err().message(), "duplicate tag `type`");
}

mod keys {
    pub const TAG: &str = "t";
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(
    tag = keys::TAG,
    tag_alias = "type",
    content = "c",
    content_alias = "data",
    deny_unknown_fields
)]
enum Adjacent {
    Value(u32),
    Empty,
}

#[test]
fn test_content_aliases() {
    assert_eq!(
        serialize(&Adjacent::Value(1)),
        map(&[("t", "Value".into()), ("c", 1u64.into())])
    );
    for (tag, content) in [("t", "c"), ("type", "data"), ("t", "data")] {
        let rv = deserialize::<Adjacent>(map(&[(tag, "Value".into()), (content, 1u64.into())]));
        assert_eq!(rv.unwrap(), Adjacent::Value(1));
        let rv = deserialize::<Adjacent>(map(&[(content, 1u64.into()), (tag, "Value".into())]));
        assert_eq!(rv.unwrap(), Adjacent::Value(1));
        let rv = deserialize::<Adjacent>(map(&[(tag, "Empty".into())]));
        assert_eq!(rv.unwrap(), Adjacent::Empty);
    }
    // errors name the keys by their names
    let rv = deserialize::<Adjacent>(map(&[("data", 1u64.into())]));
    assert_eq!(rv.unwrap_err().message(), "missing tag `t`");
    let rv = deserialize::<Adjacent>(map(&[
        ("type", "Value".into()),
        ("c", 1u64.into()),
        ("data", 2u64.into()),
    ]));
    assert_eq!(rv.unwrap_err().message(), "duplicate field `c`");
    let rv = deserialize::<Adjacent>(map(&[
        ("t", "Value".into()),
        ("type", "Value".into()),
        ("c", 1u64.into()),
    ]));
    assert_eq!(rv.unwrap_err().message(), "duplicate field `t`");
    let rv = deserialize::<Adjacent>(map(&[("t", "Empty".into()), ("x", 1u64.into())]));
    assert_eq!(
        rv.unwrap_err().message(),
        "unknown field `x`, expected `t` or `c`"
    );
}
