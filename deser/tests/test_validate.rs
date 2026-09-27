use std::collections::BTreeMap;

use deser::de::{DeserializeDriver, DeserializeOwned};
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

fn map<'a>(pairs: &[(&'a str, Event<'a>)]) -> Vec<Event<'a>> {
    let mut events = vec![Event::map_start()];
    for (key, value) in pairs {
        events.push((*key).into());
        events.push(value.clone());
    }
    events.push(Event::MapEnd);
    events
}

fn message<T: std::fmt::Debug>(rv: Result<T, Error>) -> String {
    rv.unwrap_err().message().to_string()
}

fn non_zero(value: &u16) -> Result<(), &'static str> {
    if *value == 0 {
        Err("must not be zero")
    } else {
        Ok(())
    }
}

fn not_empty<T>(value: &[T]) -> Result<(), String> {
    if value.is_empty() {
        Err("must not be empty".into())
    } else {
        Ok(())
    }
}

fn ordered(value: &Range) -> Result<(), String> {
    if value.min > value.max {
        Err(format!(
            "min {} is larger than max {}",
            value.min, value.max
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(validate = ordered)]
struct Range {
    min: u32,
    max: u32,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Server {
    #[deser(validate = non_zero)]
    port: u16,
    #[deser(validate = not_empty, default)]
    hosts: Vec<String>,
    range: Option<Range>,
}

fn server(port: u64, hosts: &[&'static str]) -> Vec<Event<'static>> {
    let mut events = vec![
        Event::map_start(),
        "port".into(),
        port.into(),
        "hosts".into(),
    ];
    events.push(Event::seq_start());
    events.extend(hosts.iter().map(|x| Event::from(*x)));
    events.push(Event::SeqEnd);
    events.push(Event::MapEnd);
    events
}

#[test]
fn test_fields() {
    let value: Server = deserialize(server(80, &["a"])).unwrap();
    assert_eq!(value.port, 80);
    assert_eq!(value.hosts, ["a"]);
    assert_eq!(
        message(deserialize::<Server>(server(0, &["a"]))),
        "invalid value: must not be zero"
    );
    assert_eq!(
        message(deserialize::<Server>(server(1, &[]))),
        "invalid value: must not be empty"
    );

    // missing values are not validated
    let value: Server = deserialize(map(&[("port", 1u64.into())])).unwrap();
    assert!(value.hosts.is_empty());
}

#[test]
fn test_containers() {
    let value: Range = deserialize(map(&[("min", 1u64.into()), ("max", 2u64.into())])).unwrap();
    assert_eq!(value, Range { min: 1, max: 2 });
    let rv = deserialize::<Range>(map(&[("min", 3u64.into()), ("max", 2u64.into())]));
    assert_eq!(message(rv), "invalid value: min 3 is larger than max 2");

    // containers are validated wherever they are used
    let mut events = map(&[("port", 1u64.into())]);
    events.pop();
    events.push("range".into());
    events.extend(map(&[("min", 3u64.into()), ("max", 2u64.into())]));
    events.push(Event::MapEnd);
    let rv = deserialize::<Server>(events);
    assert_eq!(message(rv), "invalid value: min 3 is larger than max 2");

    let rv = deserialize::<BTreeMap<String, Range>>(
        std::iter::once(Event::map_start())
            .chain(["a".into()])
            .chain(map(&[("min", 3u64.into()), ("max", 2u64.into())]))
            .chain([Event::MapEnd])
            .collect(),
    );
    assert_eq!(message(rv), "invalid value: min 3 is larger than max 2");
}

fn even(value: &Even) -> Result<(), &'static str> {
    if value.0.is_multiple_of(2) {
        Ok(())
    } else {
        Err("odd")
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(validate = even)]
struct Even(u32);

fn even_list(value: &EvenList) -> Result<(), &'static str> {
    if value.0.len().is_multiple_of(2) {
        Ok(())
    } else {
        Err("odd length")
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(validate = even_list)]
struct EvenList(Vec<u32>);

#[test]
fn test_newtypes() {
    assert_eq!(deserialize::<Even>(vec![2u64.into()]).unwrap(), Even(2));
    assert_eq!(
        message(deserialize::<Even>(vec![3u64.into()])),
        "invalid value: odd"
    );
    // atoms in structs
    #[derive(Debug, Deserialize)]
    struct Holder {
        #[allow(unused)]
        value: Even,
    }
    let rv = deserialize::<Holder>(map(&[("value", 3u64.into())]));
    assert_eq!(message(rv), "invalid value: odd");

    assert_eq!(
        deserialize::<EvenList>(vec![
            Event::seq_start(),
            1u64.into(),
            2u64.into(),
            Event::SeqEnd
        ])
        .unwrap(),
        EvenList(vec![1, 2])
    );
    let rv = deserialize::<EvenList>(vec![Event::seq_start(), 1u64.into(), Event::SeqEnd]);
    assert_eq!(message(rv), "invalid value: odd length");
}

fn not_legacy(value: &Mode) -> Result<(), &'static str> {
    if *value == Mode::Legacy {
        Err("legacy is no longer supported")
    } else {
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[deser(validate = not_legacy, rename_all = "lowercase")]
enum Mode {
    Fast,
    Legacy,
}

fn positive(value: &Shape) -> Result<(), &'static str> {
    match value {
        Shape::Circle { radius } if *radius == 0 => Err("radius must be positive"),
        _ => Ok(()),
    }
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(tag = "type", validate = positive)]
enum Shape {
    Circle {
        radius: u32,
    },
    Rect {
        #[deser(validate = non_zero)]
        width: u16,
    },
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(untagged, validate = generic_ok)]
enum Either<T> {
    Left(T),
    Right(String),
}

fn generic_ok<T>(value: &Either<T>) -> Result<(), &'static str> {
    match value {
        Either::Right(s) if s.is_empty() => Err("empty"),
        _ => Ok(()),
    }
}

#[test]
fn test_enums() {
    assert_eq!(
        deserialize::<Mode>(vec!["fast".into()]).unwrap(),
        Mode::Fast
    );
    assert_eq!(
        message(deserialize::<Mode>(vec!["legacy".into()])),
        "invalid value: legacy is no longer supported"
    );

    assert_eq!(
        deserialize::<Shape>(map(&[("type", "Circle".into()), ("radius", 1u64.into())])).unwrap(),
        Shape::Circle { radius: 1 }
    );
    let rv = deserialize::<Shape>(map(&[("radius", 0u64.into()), ("type", "Circle".into())]));
    assert_eq!(message(rv), "invalid value: radius must be positive");
    // fields of struct variants
    let rv = deserialize::<Shape>(map(&[("type", "Rect".into()), ("width", 0u64.into())]));
    assert_eq!(message(rv), "invalid value: must not be zero");

    assert_eq!(
        deserialize::<Either<u32>>(vec![1u64.into()]).unwrap(),
        Either::Left(1)
    );
    assert_eq!(
        message(deserialize::<Either<u32>>(vec!["".into()])),
        "invalid value: empty"
    );
}
