use std::borrow::Cow;
use std::collections::BTreeMap;

use deser::adapters::{As, Borrowed};
use deser::de::{DeserializeDriver, Recording};
use deser::{Atom, Deserialize, ErrorKind, Event};

/// Emits the events borrowed.
fn borrowed<'de, T: Deserialize<'de>>(events: Vec<Event<'de>>) -> Result<T, deser::Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in events {
            driver.emit_borrowed(event)?;
        }
    }
    Ok(out.unwrap())
}

/// Emits the events transient.
fn transient<'de, T: Deserialize<'de>>(events: Vec<Event<'_>>) -> Result<T, deser::Error> {
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
fn test_borrowed_primitives() {
    let input = String::from("hello");
    let value: &str = borrowed(vec![input.as_str().into()]).unwrap();
    assert_eq!(value, "hello");
    assert!(std::ptr::eq(value, input.as_str()));

    // `Cow` is owned unless borrowed with the adapter
    let value: Cow<'_, str> = borrowed(vec![input.as_str().into()]).unwrap();
    assert!(matches!(value, Cow::Owned(_)));
    let value: As<Cow<'_, str>, Borrowed> = borrowed(vec![input.as_str().into()]).unwrap();
    assert!(matches!(*value, Cow::Borrowed("hello")));

    let bytes = vec![1u8, 2, 3];
    let value: &[u8] = borrowed(vec![bytes.as_slice().into()]).unwrap();
    assert_eq!(value, [1, 2, 3]);
    let value: As<Cow<'_, [u8]>, Borrowed> = borrowed(vec![bytes.as_slice().into()]).unwrap();
    assert!(matches!(*value, Cow::Borrowed(&[1, 2, 3])));

    // types that do not borrow accept borrowed atoms
    let value: String = borrowed(vec![input.as_str().into()]).unwrap();
    assert_eq!(value, "hello");
}

#[test]
fn test_transient_data_cannot_be_borrowed() {
    let err = transient::<&str>(vec!["hello".into()]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unexpected);
    assert!(err.to_string().contains("expected a borrowed string"));
    // owned data in a borrowed atom cannot be borrowed either
    let err = borrowed::<&str>(vec![Event::Atom(Atom::Str(Cow::Owned("x".into())))]).unwrap_err();
    assert!(err.to_string().contains("expected a borrowed string"));

    // but it can go into a `Cow`, even with the adapter
    let value: As<Cow<'_, str>, Borrowed> = transient(vec!["hello".into()]).unwrap();
    assert!(matches!(*value, Cow::Owned(_)));
    assert_eq!(*value, "hello");
    let value: As<Cow<'_, [u8]>, Borrowed> = transient(vec![Event::from(&b"hi"[..])]).unwrap();
    assert!(matches!(*value, Cow::Owned(_)));
}

#[test]
fn test_borrowed_containers() {
    let input = String::from("a b c");
    let words: Vec<&str> = input.split(' ').collect();

    let events = vec![
        Event::seq_start(),
        words[0].into(),
        words[1].into(),
        words[2].into(),
        Event::SeqEnd,
    ];
    let value: Vec<&str> = borrowed(events.clone()).unwrap();
    assert_eq!(value, ["a", "b", "c"]);
    let value: (&str, As<Cow<'_, str>, Borrowed>, String) = borrowed(events.clone()).unwrap();
    assert_eq!(value.0, "a");
    assert!(matches!(*value.1, Cow::Borrowed("b")));
    assert_eq!(value.2, "c");
    let value: [&str; 3] = borrowed(events).unwrap();
    assert_eq!(value, ["a", "b", "c"]);

    let events = vec![
        Event::map_start(),
        words[0].into(),
        words[1].into(),
        words[2].into(),
        Event::Atom(Atom::Null),
        Event::MapEnd,
    ];
    let value: BTreeMap<&str, Option<&str>> = borrowed(events).unwrap();
    assert_eq!(value["a"], Some("b"));
    assert_eq!(value["c"], None);
}

#[derive(Debug, PartialEq, Deserialize)]
struct User<'a> {
    name: &'a str,
    #[deser(as = Borrowed)]
    nick: Cow<'a, str>,
    #[deser(as = Vec<Borrowed>)]
    aliases: Vec<Cow<'a, str>>,
    tags: Vec<&'a str>,
    #[deser(default)]
    bio: Option<&'a str>,
    address: Address<'a>,
    #[deser(flatten)]
    extra: Extra<'a>,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Address<'a> {
    city: &'a str,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Extra<'a> {
    note: &'a str,
}

#[derive(Debug, PartialEq, Deserialize)]
struct Name<'a>(&'a str);

#[derive(Debug, PartialEq, Deserialize)]
struct Generic<'a, T> {
    key: &'a str,
    value: T,
}

#[test]
fn test_derive() {
    let input = String::from("name nick tag city note");
    let parts: Vec<&str> = input.split(' ').collect();
    let events = vec![
        Event::map_start(),
        "name".into(),
        parts[0].into(),
        "nick".into(),
        parts[1].into(),
        "tags".into(),
        Event::seq_start(),
        parts[2].into(),
        Event::SeqEnd,
        "aliases".into(),
        Event::seq_start(),
        parts[1].into(),
        Event::SeqEnd,
        "address".into(),
        Event::map_start(),
        "city".into(),
        parts[3].into(),
        Event::MapEnd,
        "note".into(),
        parts[4].into(),
        Event::MapEnd,
    ];
    let user: User = borrowed(events).unwrap();
    assert_eq!(
        user,
        User {
            name: "name",
            nick: Cow::Borrowed("nick"),
            aliases: vec![Cow::Borrowed("nick")],
            tags: vec!["tag"],
            bio: None,
            address: Address { city: "city" },
            extra: Extra { note: "note" },
        }
    );
    assert!(std::ptr::eq(user.name, parts[0]));
    assert!(matches!(user.nick, Cow::Borrowed(_)));
    assert!(matches!(user.aliases[0], Cow::Borrowed(_)));

    let name: Name = borrowed(vec![parts[0].into()]).unwrap();
    assert_eq!(name, Name("name"));

    let value: Generic<'_, u32> = borrowed(vec![
        Event::map_start(),
        "key".into(),
        parts[0].into(),
        "value".into(),
        42u64.into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(
        value,
        Generic {
            key: "name",
            value: 42
        }
    );
}

#[test]
fn test_recordings_do_not_borrow() {
    // recorded values are detached from the input, only types that can hold
    // owned data can be deserialized from them.
    let input = String::from("hello");
    let recording: Recording = borrowed(vec![input.as_str().into()]).unwrap();
    let mut out = None::<Cow<'_, str>>;
    {
        let mut driver_out = None::<()>;
        let mut driver = DeserializeDriver::new(&mut driver_out);
        recording
            .replay(Deserialize::deserialize_into(&mut out), driver.state_mut())
            .unwrap();
    }
    assert_eq!(out.as_deref(), Some("hello"));

    let mut out = None::<&str>;
    let mut driver_out = None::<()>;
    let mut driver = DeserializeDriver::new(&mut driver_out);
    assert!(
        recording
            .replay(Deserialize::deserialize_into(&mut out), driver.state_mut())
            .is_err()
    );
}

#[test]
fn test_deserialize_owned() {
    fn owned<T: deser::de::DeserializeOwned>(json: String) -> T {
        transient(vec![json.as_str().into()]).unwrap()
    }
    assert_eq!(owned::<String>("x".into()), "x");
    // `Cow<'static, str>` does not borrow, so it can be deserialized from
    // any data
    assert_eq!(owned::<Cow<'static, str>>("x".into()), "x");

    #[derive(Deserialize)]
    struct Config {
        name: Cow<'static, str>,
    }
    let config: Config = owned_map(String::from("demo"));
    assert_eq!(config.name, "demo");

    fn owned_map<T: deser::de::DeserializeOwned>(value: String) -> T {
        transient(vec![
            Event::map_start(),
            "name".into(),
            value.as_str().into(),
            Event::MapEnd,
        ])
        .unwrap()
    }
}

#[test]
fn test_borrowed_enums() {
    use deser::Serialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum External<'a> {
        Unit,
        Newtype(&'a str),
        Tuple(&'a str, u32),
        Struct {
            name: &'a str,
            #[deser(as = Borrowed)]
            text: Cow<'a, str>,
        },
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(tag = "type")]
    enum Internal<'a> {
        Struct {
            name: &'a str,
        },
        #[deser(other)]
        Other(#[deser(tag)] &'a str),
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(tag = "t", content = "c")]
    enum Adjacent<'a> {
        Newtype(&'a str),
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(untagged)]
    enum Untagged<'a> {
        Number(u32),
        Text(&'a str),
    }

    let input = String::from("hello");
    let s = input.as_str();

    let value: External<'_> = borrowed(vec![
        Event::map_start(),
        "Newtype".into(),
        s.into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value, External::Newtype("hello"));
    let value: External<'_> = borrowed(vec![
        Event::map_start(),
        "Tuple".into(),
        Event::seq_start(),
        s.into(),
        1u64.into(),
        Event::SeqEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value, External::Tuple("hello", 1));
    let value: External<'_> = borrowed(vec![
        Event::map_start(),
        "Struct".into(),
        Event::map_start(),
        "name".into(),
        s.into(),
        "text".into(),
        s.into(),
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    match value {
        External::Struct { name, text } => {
            assert!(std::ptr::eq(name, s));
            assert!(matches!(text, Cow::Borrowed("hello")));
        }
        other => panic!("unexpected {:?}", other),
    }
    let value: External<'_> = borrowed(vec!["Unit".into()]).unwrap();
    assert_eq!(value, External::Unit);

    // the tag is recorded before it's known, the values after it are
    // borrowed
    let value: Internal<'_> = borrowed(vec![
        Event::map_start(),
        "type".into(),
        "Struct".into(),
        "name".into(),
        s.into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value, Internal::Struct { name: "hello" });

    let value: Adjacent<'_> = borrowed(vec![
        Event::map_start(),
        "t".into(),
        "Newtype".into(),
        "c".into(),
        s.into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value, Adjacent::Newtype("hello"));

    // untagged enums replay recorded values which are not borrowed
    let err = borrowed::<Untagged<'_>>(vec![s.into()]).unwrap_err();
    assert!(err.to_string().contains("did not match any variant"));
    let value: Untagged<'_> = borrowed(vec![42u64.into()]).unwrap();
    assert_eq!(value, Untagged::Number(42));
}

#[test]
fn test_borrowed_enum_in_struct() {
    #[derive(Debug, PartialEq, Deserialize)]
    enum Value<'a> {
        Text(&'a str),
        Pair { a: &'a str, b: Option<&'a str> },
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Outer<'a> {
        value: Value<'a>,
        values: Vec<Value<'a>>,
    }

    let input = String::from("x");
    let s = input.as_str();
    let value: Outer<'_> = borrowed(vec![
        Event::map_start(),
        "value".into(),
        Event::map_start(),
        "Text".into(),
        s.into(),
        Event::MapEnd,
        "values".into(),
        Event::seq_start(),
        Event::map_start(),
        "Pair".into(),
        Event::map_start(),
        "a".into(),
        s.into(),
        Event::MapEnd,
        Event::MapEnd,
        Event::SeqEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(
        value,
        Outer {
            value: Value::Text("x"),
            values: vec![Value::Pair { a: "x", b: None }],
        }
    );
}

#[test]
fn test_generic_enum_with_borrowed_parameter() {
    #[derive(Debug, PartialEq, Deserialize)]
    enum Either<L, R> {
        Left(L),
        Right { value: R },
    }

    let input = String::from("left");
    let value: Either<&str, u32> = borrowed(vec![
        Event::map_start(),
        "Left".into(),
        input.as_str().into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(value, Either::Left("left"));
}

#[test]
fn test_validated_borrowed_enum() {
    fn not_empty(value: &Name<'_>) -> Result<(), &'static str> {
        match value {
            Name::Plain("") => Err("empty name"),
            _ => Ok(()),
        }
    }

    #[derive(Debug, PartialEq, Deserialize)]
    #[deser(validate = not_empty, tag = "kind", content = "value")]
    enum Name<'a> {
        Plain(&'a str),
        #[deser(default)]
        Missing,
    }

    let input = String::from("");
    let err = borrowed::<Name<'_>>(vec![
        Event::map_start(),
        "kind".into(),
        "Plain".into(),
        "value".into(),
        input.as_str().into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.message(), "invalid value: empty name");
    let value: Name<'_> = borrowed(vec![Event::map_start(), Event::MapEnd]).unwrap();
    assert_eq!(value, Name::Missing);
}
