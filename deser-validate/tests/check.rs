//! Tests for the `Check` adapter on fields and types.
use std::collections::BTreeMap;

use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::{Deserialize, Error, Event, Serialize};
use deser_validate::{Check, NonEmpty, Validator, Violation, validator};

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

validator!(NonZero(value: &u16) => *value != 0, "must not be zero");
validator!(OrderedRange(range: &Range) = ordered);

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
#[deser(deserialize_as = Check<OrderedRange, _>)]
struct Range {
    min: u32,
    max: u32,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Server {
    #[deser(as = Check<NonZero>)]
    port: u16,
    #[deser(as = Check<NonEmpty>, default)]
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

    // defaults of missing values are not validated
    let value: Server = deserialize(map(&[("port", 1u64.into())])).unwrap();
    assert!(value.hosts.is_empty());
}

#[test]
fn test_containers() {
    let value: Range = deserialize(map(&[("min", 1u64.into()), ("max", 2u64.into())])).unwrap();
    assert_eq!(value, Range { min: 1, max: 2 });
    let rv = deserialize::<Range>(map(&[("min", 3u64.into()), ("max", 2u64.into())]));
    assert_eq!(message(rv), "invalid value: min 3 is larger than max 2");

    // types are validated wherever they are used
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

validator!(EvenNumber(value: &Even) => value.0.is_multiple_of(2), "odd");
validator!(EvenLength(value: &EvenList) => value.0.len().is_multiple_of(2), "odd length");

#[derive(Debug, Deserialize, PartialEq)]
#[deser(deserialize_as = Check<EvenNumber, _>)]
struct Even(u32);

#[derive(Debug, Deserialize, PartialEq)]
#[deser(deserialize_as = Check<EvenLength, _>)]
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

validator!(Supported(value: &Mode) => *value != Mode::Legacy, "legacy is no longer supported");

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[deser(deserialize_as = Check<Supported, _>, rename_all = "lowercase")]
enum Mode {
    Fast,
    Legacy,
}

validator!(PositiveRadius(value: &Shape) => !matches!(value, Shape::Circle { radius: 0 }), "radius must be positive");

#[derive(Debug, Deserialize, PartialEq)]
#[deser(tag = "type", deserialize_as = Check<PositiveRadius, _>)]
enum Shape {
    Circle {
        radius: u32,
    },
    Rect {
        #[deser(as = Check<NonZero>)]
        width: u16,
    },
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(untagged, deserialize_as = Check<NonEmptyRight, _>)]
enum Either<T> {
    Left(T),
    Right(String),
}

/// Validators of generic types implement the trait themselves.
struct NonEmptyRight;

impl<T> Validator<Either<T>> for NonEmptyRight {
    fn validate(value: &Either<T>) -> Result<(), Violation> {
        match value {
            Either::Right(s) if s.is_empty() => Err(Violation::new("empty", "empty")),
            _ => Ok(()),
        }
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

fn update<T: for<'de> Deserialize<'de>>(
    value: &mut T,
    events: Vec<Event<'_>>,
) -> Result<(), Error> {
    let mut driver = DeserializeDriver::update(value);
    for event in events {
        driver.emit(event)?;
    }
    Ok(())
}

#[test]
fn test_borrowed() {
    // validators of types with lifetimes implement the trait themselves
    struct NonEmptyName;

    impl<'a> Validator<Name<'a>> for NonEmptyName {
        fn validate(value: &Name<'a>) -> Result<(), Violation> {
            match value {
                Name::Plain("") => Err(Violation::new("empty", "empty name")),
                _ => Ok(()),
            }
        }
    }

    #[derive(Debug, PartialEq, Deserialize)]
    #[deser(deserialize_as = Check<NonEmptyName, _>, tag = "kind", content = "value")]
    enum Name<'a> {
        Plain(&'a str),
        #[deser(default)]
        Missing,
    }

    let input = String::from("");
    let mut out = None::<Name<'_>>;
    let mut driver = DeserializeDriver::new(&mut out);
    driver.emit(Event::map_start()).unwrap();
    driver.emit("kind").unwrap();
    driver.emit("Plain").unwrap();
    driver.emit("value").unwrap();
    driver.emit_borrowed(input.as_str()).unwrap();
    let err = driver.emit(Event::MapEnd).unwrap_err();
    assert_eq!(err.message(), "invalid value: empty name");
    drop(driver);

    let mut out = None::<Name<'_>>;
    let mut driver = DeserializeDriver::new(&mut out);
    driver.emit(Event::map_start()).unwrap();
    driver.emit(Event::MapEnd).unwrap();
    drop(driver);
    assert_eq!(out, Some(Name::Missing));

    // tuple structs that borrow
    struct NonEmptyNamed;

    impl<'a> Validator<Named<'a>> for NonEmptyNamed {
        fn validate(value: &Named<'a>) -> Result<(), Violation> {
            match value.0 {
                "" => Err(Violation::new("empty", "empty name")),
                _ => Ok(()),
            }
        }
    }

    #[derive(Debug, PartialEq, Deserialize)]
    #[deser(deserialize_as = Check<NonEmptyNamed, _>)]
    struct Named<'a>(&'a str, u32);

    let mut out = None::<Named<'_>>;
    let mut driver = DeserializeDriver::new(&mut out);
    driver.emit(Event::seq_start()).unwrap();
    driver.emit_borrowed(input.as_str()).unwrap();
    driver.emit(1u64).unwrap();
    let err = driver.emit(Event::SeqEnd).unwrap_err();
    assert_eq!(err.message(), "invalid value: empty name");
}

#[test]
fn test_shapes() {
    validator!(OrderedSpan(value: &Span) => value.0 <= value.1, "start is after end");

    #[derive(Debug, PartialEq, Deserialize)]
    #[deser(deserialize_as = Check<OrderedSpan, _>)]
    struct Span(u32, u32);

    let seq = |a: u64, b: u64| vec![Event::seq_start(), a.into(), b.into(), Event::SeqEnd];
    assert_eq!(deserialize::<Span>(seq(1, 2)).unwrap(), Span(1, 2));
    assert_eq!(
        message(deserialize::<Span>(seq(2, 1))),
        "invalid value: start is after end"
    );

    validator!(PositiveAmount(value: &Amount) => value.0 > 0, "not positive");

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(transparent, deserialize_as = Check<PositiveAmount, _>)]
    struct Amount(i32, #[deser(skip)] ());

    assert_eq!(
        deserialize::<Amount>(vec![1i64.into()]).unwrap(),
        Amount(1, ())
    );
    assert_eq!(
        message(deserialize::<Amount>(vec![0i64.into()])),
        "invalid value: not positive"
    );
}

validator!(Positive(value: &u32) => *value > 0, "must be positive");
validator!(OrderedBounds(value: &Bounds) => value.min <= value.max, "min is larger than max");

#[derive(Debug, Deserialize, PartialEq)]
#[deser(deserialize_as = Check<OrderedBounds, _>, deny_unknown_fields)]
struct Bounds {
    #[deser(as = Check<Positive>)]
    min: u32,
    max: u32,
}

#[test]
fn test_updates() {
    let mut bounds = Bounds { min: 1, max: 5 };
    update(&mut bounds, map(&[("max", 3u64.into())])).unwrap();
    assert_eq!(bounds, Bounds { min: 1, max: 3 });

    // fields are checked after they were updated
    let rv = update(&mut bounds, map(&[("min", 0u64.into())]));
    assert_eq!(message(rv), "invalid value: must be positive");

    // the whole value is checked once the update is complete
    let mut bounds = Bounds { min: 1, max: 3 };
    let rv = update(&mut bounds, map(&[("min", 4u64.into())]));
    assert_eq!(message(rv), "invalid value: min is larger than max");

    // unknown fields
    let rv = update(&mut bounds, map(&[("x", 4u64.into())]));
    assert_eq!(message(rv), "unknown field `x`, expected `min` or `max`");
}

validator!(UniqueName(value: &Settings) => !value.extra.contains_key(&value.name), "extra conflicts with name");

#[derive(Debug, Deserialize, PartialEq)]
struct Limits {
    timeout: u32,
}

#[derive(Debug, Deserialize, PartialEq)]
#[deser(deserialize_as = Check<UniqueName, _>)]
struct Settings {
    name: String,
    #[deser(flatten)]
    limits: Limits,
    #[deser(flatten)]
    extra: BTreeMap<String, u32>,
}

#[test]
fn test_flattened_updates() {
    let mut settings = Settings {
        name: "a".into(),
        limits: Limits { timeout: 5 },
        extra: BTreeMap::from([("x".to_string(), 1)]),
    };
    update(
        &mut settings,
        map(&[("timeout", 1u64.into()), ("y", 2u64.into())]),
    )
    .unwrap();
    assert_eq!(settings.limits.timeout, 1);
    assert_eq!(settings.extra.len(), 2);

    // the whole value is checked once the flattened fields are updated
    let rv = update(&mut settings, map(&[("a", 1u64.into())]));
    assert_eq!(message(rv), "invalid value: extra conflicts with name");
}

#[test]
fn test_locations() {
    // errors of compound values point at their start
    let err = deser_json::Deserializer::from_str(r#"{"port": 1, "hosts": []}"#)
        .deserialize_with::<Server, _>(|driver| driver.push_layer(deser_path::PathLayer::new()))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: invalid value: must not be empty at line 1 column 22 (path: hosts)"
    );
    let err = deser_json::Deserializer::from_str(r#"{"port": 1, "range": {"min": 3, "max": 2}}"#)
        .deserialize_with::<Server, _>(|driver| driver.push_layer(deser_path::PathLayer::new()))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: invalid value: min 3 is larger than max 2 at line 1 column 22 (path: range)"
    );
}

#[test]
fn test_codes() {
    use deser_validate::{Email, Len, Max, MaxLen, Min, MinLen, Range};

    // the code of a violation is the name of the validator
    let code = |rv: Result<(), Violation>| rv.unwrap_err().code().to_string();
    assert_eq!(code(Len::<2, 3>::validate("a")), "len");
    assert_eq!(code(MinLen::<2>::validate("a")), "min_len");
    assert_eq!(code(MaxLen::<2>::validate("abc")), "max_len");
    assert_eq!(code(Range::<2, 3>::validate(&1u32)), "range");
    assert_eq!(code(Min::<2>::validate(&1u32)), "min");
    assert_eq!(code(Max::<2>::validate(&3u32)), "max");
    assert_eq!(code(NonEmpty::validate("")), "non_empty");
    assert_eq!(code(Email::validate("x")), "email");
    assert_eq!(code(NonZero::validate(&0u16)), "non_zero");

    // or the code it was given
    validator!(Port(port: &u16) => *port != 0, "must not be zero", code = "port");
    assert_eq!(code(Port::validate(&0u16)), "port");
    fn check_name(_value: &str) -> Result<(), &'static str> {
        Err("invalid")
    }
    validator!(Name(value: &str) = check_name, code = "name");
    assert_eq!(code(Name::validate("x")), "name");
    validator!(Block(value: &str) { Err(value.to_string()) }, code = "block");
    assert_eq!(code(Block::validate("x")), "block");
}
