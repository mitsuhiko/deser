use std::borrow::Cow;
use std::collections::BTreeMap;

use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::{Atom, Deserialize, Error, ErrorKind, Event};

fn lexical(value: &str) -> Event<'_> {
    Event::Atom(Atom::Lexical(Cow::Borrowed(value)))
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

/// Events of a map where keys and values are lexical, like a query string.
fn lexical_map<'a>(pairs: &[(&'a str, &'a str)]) -> Vec<Event<'a>> {
    let mut events = vec![Event::map_start()];
    for (key, value) in pairs {
        events.push(lexical(key));
        events.push(lexical(value));
    }
    events.push(Event::MapEnd);
    events
}

#[test]
fn test_numbers() {
    assert_eq!(deserialize::<u8>(vec![lexical("42")]).unwrap(), 42);
    assert_eq!(deserialize::<i64>(vec![lexical("-42")]).unwrap(), -42);
    assert_eq!(deserialize::<u32>(vec![lexical("+7")]).unwrap(), 7);
    assert_eq!(
        deserialize::<u128>(vec![lexical("340282366920938463463374607431768211455")]).unwrap(),
        u128::MAX
    );
    assert_eq!(deserialize::<f64>(vec![lexical("1.5")]).unwrap(), 1.5);
    assert_eq!(deserialize::<f32>(vec![lexical("0.1")]).unwrap(), 0.1f32);
    assert_eq!(deserialize::<f64>(vec![lexical("1e3")]).unwrap(), 1000.0);
}

#[test]
fn test_number_errors() {
    let err = deserialize::<u8>(vec![lexical("300")]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);

    let err = deserialize::<u8>(vec![lexical("abc")]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unexpected);
    assert_eq!(err.message(), "invalid value \"abc\", expected u8");

    let err = deserialize::<u32>(vec![lexical("")]).unwrap_err();
    assert_eq!(err.message(), "invalid value \"\", expected u32");

    let err = deserialize::<f64>(vec![lexical("1,5")]).unwrap_err();
    assert_eq!(err.message(), "invalid value \"1,5\", expected f64");

    // strings are strings, they are not parsed
    let err = deserialize::<u32>(vec![Event::Atom(Atom::Str("42".into()))]).unwrap_err();
    assert_eq!(err.message(), "unexpected string, expected u32");
}

#[test]
fn test_bool() {
    for value in ["true", "True", "yes", "ON", "1"] {
        assert!(
            deserialize::<bool>(vec![lexical(value)]).unwrap(),
            "{}",
            value
        );
    }
    for value in ["false", "no", "Off", "0"] {
        assert!(
            !deserialize::<bool>(vec![lexical(value)]).unwrap(),
            "{}",
            value
        );
    }
    let err = deserialize::<bool>(vec![lexical("")]).unwrap_err();
    assert_eq!(
        err.message(),
        "invalid value \"\", expected bool (true, yes, on, 1, false, no, off or 0)"
    );
}

#[test]
fn test_strings() {
    assert_eq!(deserialize::<String>(vec![lexical("42")]).unwrap(), "42");
    assert_eq!(deserialize::<char>(vec![lexical("x")]).unwrap(), 'x');
    assert_eq!(
        deserialize::<std::net::IpAddr>(vec![lexical("127.0.0.1")]).unwrap(),
        std::net::IpAddr::from([127, 0, 0, 1])
    );
    assert_eq!(
        deserialize::<std::path::PathBuf>(vec![lexical("/tmp")]).unwrap(),
        std::path::PathBuf::from("/tmp")
    );
    assert_eq!(deserialize::<()>(vec![lexical("")]).unwrap(), ());
    assert!(deserialize::<()>(vec![lexical("x")]).is_err());
}

#[test]
fn test_borrowed() {
    #[derive(Debug, Deserialize)]
    struct Borrowing<'a> {
        name: &'a str,
        #[deser(as = deser::adapters::Borrowed)]
        cow: Cow<'a, str>,
    }

    let input = String::from("name=Jane&cow=moo");
    let mut out = None::<Borrowing>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit(Event::map_start()).unwrap();
        for (key, value) in [(&input[..4], &input[5..9]), (&input[10..13], &input[14..])] {
            driver
                .emit_borrowed(Atom::Lexical(Cow::Borrowed(key)))
                .unwrap();
            driver
                .emit_borrowed(Atom::Lexical(Cow::Borrowed(value)))
                .unwrap();
        }
        driver.emit(Event::MapEnd).unwrap();
    }
    let out = out.unwrap();
    assert_eq!(out.name, "Jane");
    assert!(matches!(out.cow, Cow::Borrowed("moo")));
}

#[test]
fn test_struct_and_map_keys() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Query {
        page: u32,
        verbose: bool,
        name: String,
    }

    assert_eq!(
        deserialize::<Query>(lexical_map(&[
            ("page", "2"),
            ("verbose", "on"),
            ("name", "42"),
        ]))
        .unwrap(),
        Query {
            page: 2,
            verbose: true,
            name: "42".into(),
        }
    );

    // keys of any type parse from lexical keys
    let map =
        deserialize::<BTreeMap<u16, bool>>(lexical_map(&[("80", "yes"), ("443", "no")])).unwrap();
    assert_eq!(map, BTreeMap::from([(80, true), (443, false)]));
    let map = deserialize::<BTreeMap<bool, u8>>(lexical_map(&[("true", "1")])).unwrap();
    assert_eq!(map, BTreeMap::from([(true, 1)]));
}

#[test]
fn test_enums() {
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(rename_all = "lowercase")]
    enum Order {
        Asc,
        Desc,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    enum Filter {
        Name(String),
        Limit(u32),
    }

    assert_eq!(
        deserialize::<Order>(vec![lexical("desc")]).unwrap(),
        Order::Desc
    );
    let err = deserialize::<Order>(vec![lexical("up")]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unexpected);

    assert_eq!(
        deserialize::<Filter>(lexical_map(&[("Limit", "10")])).unwrap(),
        Filter::Limit(10)
    );
}

/// Lexical atoms stay lexical when values are buffered.
///
/// This is where serde loses the information that a string is untyped
/// (serde-rs/serde#1183): numbers in flattened structs and in internally
/// tagged and untagged enums fail to parse.
#[test]
fn test_buffering() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Paginate {
        limit: u64,
        offset: u64,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Query {
        q: String,
        #[deser(flatten)]
        paginate: Paginate,
    }

    assert_eq!(
        deserialize::<Query>(lexical_map(&[("limit", "10"), ("q", "x"), ("offset", "0")])).unwrap(),
        Query {
            q: "x".into(),
            paginate: Paginate {
                limit: 10,
                offset: 0,
            },
        }
    );

    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "tag")]
    enum Tagged {
        Item { field: i32, flag: bool },
    }

    // the tag comes last, the fields are buffered
    assert_eq!(
        deserialize::<Tagged>(lexical_map(&[
            ("field", "42"),
            ("flag", "true"),
            ("tag", "Item"),
        ]))
        .unwrap(),
        Tagged::Item {
            field: 42,
            flag: true,
        }
    );

    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(untagged)]
    enum Untagged {
        A { foo: String },
        B { bar: i32 },
    }

    assert_eq!(
        deserialize::<Untagged>(lexical_map(&[("bar", "123")])).unwrap(),
        Untagged::B { bar: 123 }
    );
}

fn repeated<'a>(values: &[&'a str]) -> Vec<Event<'a>> {
    let mut events = vec![Event::SeqStart(
        deser::ContainerShape::new().with_repeated(true),
    )];
    events.extend(values.iter().map(|x| lexical(x)));
    events.push(Event::SeqEnd);
    events
}

/// Events of a map where values are lexical, repeated if there is more than
/// one of them.
fn query<'a>(pairs: &[(&'a str, &[&'a str])]) -> Vec<Event<'a>> {
    let mut events = vec![Event::map_start()];
    for (key, values) in pairs {
        events.push(lexical(key));
        match values {
            [value] => events.push(lexical(value)),
            values => events.extend(repeated(values)),
        }
    }
    events.push(Event::MapEnd);
    events
}

#[test]
fn test_single_value_sequences() {
    use std::collections::{BTreeSet, HashSet, VecDeque};

    // a single lexical value is a sequence of one
    assert_eq!(deserialize::<Vec<u32>>(vec![lexical("42")]).unwrap(), [42]);
    assert_eq!(
        deserialize::<VecDeque<String>>(vec![lexical("x")]).unwrap(),
        ["x"]
    );
    assert_eq!(
        deserialize::<BTreeSet<bool>>(vec![lexical("on")]).unwrap(),
        BTreeSet::from([true])
    );
    assert_eq!(
        deserialize::<HashSet<u8>>(vec![lexical("1")]).unwrap(),
        HashSet::from([1])
    );
    assert_eq!(deserialize::<[u16; 1]>(vec![lexical("7")]).unwrap(), [7]);
    assert!(deserialize::<[u16; 2]>(vec![lexical("7")]).is_err());
    assert_eq!(
        deserialize::<Option<Vec<u32>>>(vec![lexical("1")]).unwrap(),
        Some(vec![1])
    );

    // strings are strings, not sequences
    assert!(deserialize::<Vec<String>>(vec![Event::from("x")]).is_err());

    // bytes are still decoded from lexical atoms
    assert_eq!(
        deserialize::<Vec<u8>>(vec![lexical("aGk=")]).unwrap(),
        b"hi"
    );

    // borrowed values are borrowed
    let input = String::from("hello");
    let mut out = None::<Vec<&str>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver
            .emit_borrowed(Atom::Lexical(Cow::Borrowed(&input)))
            .unwrap();
    }
    assert_eq!(out.unwrap(), ["hello"]);
}

#[test]
fn test_repeated() {
    use deser::de::DuplicateKeys;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Query {
        tags: Vec<String>,
        page: u32,
        sort: Option<String>,
    }

    let events = || {
        query(&[
            ("tags", &["a"]),
            ("page", &["1", "2"]),
            ("sort", &["name", "date"]),
        ])
    };
    assert_eq!(
        deserialize::<Query>(events()).unwrap(),
        Query {
            tags: vec!["a".into()],
            page: 2,
            sort: Some("date".into()),
        }
    );
    assert_eq!(
        deserialize::<Query>(query(&[("tags", &["a", "b"]), ("page", &["1"])])).unwrap(),
        Query {
            tags: vec!["a".into(), "b".into()],
            page: 1,
            sort: None,
        }
    );

    let with_policy = |policy| {
        let mut out = None::<Query>;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            driver.state_mut().set_duplicate_keys(policy);
            for event in events() {
                driver.emit(event)?;
            }
        }
        Ok::<_, Error>(out.unwrap())
    };
    assert_eq!(
        with_policy(DuplicateKeys::First).unwrap(),
        Query {
            tags: vec!["a".into()],
            page: 1,
            sort: Some("name".into()),
        }
    );
    let err = with_policy(DuplicateKeys::Error).unwrap_err();
    assert_eq!(err.message(), "duplicate key");

    // sequences that are not repeated keys are not collapsed
    let mut events = vec![Event::seq_start(), lexical("1"), lexical("2")];
    events.push(Event::SeqEnd);
    assert!(deserialize::<u32>(events).is_err());

    // repeated keys consist of atoms
    let mut events = repeated(&["1"]);
    events.insert(1, Event::seq_start());
    events.insert(2, Event::SeqEnd);
    let err = deserialize::<u32>(events).unwrap_err();
    assert_eq!(err.message(), "the values of a repeated key must be atoms");
}

#[test]
fn test_repeated_buffered() {
    // repeated values stay repeated when they are buffered
    #[derive(Debug, Deserialize, PartialEq)]
    struct Paginate {
        limit: u32,
        tags: Vec<u32>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "kind")]
    enum Search {
        Items {
            #[deser(flatten)]
            paginate: Paginate,
        },
    }

    assert_eq!(
        deserialize::<Search>(query(&[
            ("limit", &["10", "20"]),
            ("tags", &["1"]),
            ("kind", &["Items"]),
        ]))
        .unwrap(),
        Search::Items {
            paginate: Paginate {
                limit: 20,
                tags: vec![1],
            }
        }
    );
}

#[test]
fn test_empty_optionals() {
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(rename_all = "lowercase")]
    enum Order {
        Asc,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Form {
        age: Option<u32>,
        name: Option<String>,
        flag: Option<bool>,
        order: Option<Order>,
        ids: Option<Vec<u32>>,
        names: Option<Vec<String>>,
        nested: Option<Option<u32>>,
    }

    // empty values are `None` if the type does not accept them
    assert_eq!(
        deserialize::<Form>(lexical_map(&[
            ("age", ""),
            ("name", ""),
            ("flag", ""),
            ("order", ""),
            ("ids", ""),
            ("names", ""),
            ("nested", ""),
        ]))
        .unwrap(),
        Form {
            age: None,
            name: Some("".into()),
            flag: None,
            order: None,
            ids: None,
            names: Some(vec!["".into()]),
            nested: Some(None),
        }
    );

    // through a sink handle (for instance of a sequence)
    assert_eq!(
        deserialize::<Vec<Option<u32>>>(vec![
            Event::seq_start(),
            lexical(""),
            lexical("1"),
            Event::SeqEnd,
        ])
        .unwrap(),
        [None, Some(1)]
    );

    // other errors than rejections are not hidden, and only empty values
    // are `None`
    assert_eq!(
        deserialize::<Option<u8>>(vec![lexical("300")])
            .unwrap_err()
            .kind(),
        ErrorKind::OutOfRange
    );
    assert!(deserialize::<Option<u32>>(vec![lexical("x")]).is_err());
    assert!(deserialize::<u32>(vec![lexical("")]).is_err());
}
