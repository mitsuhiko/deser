use std::borrow::Cow;
use std::collections::BTreeMap;

use deser::de::{DeserializeDriver, DeserializeOwned, LexicalRules};
use deser::{Atom, Deserialize, Error, ErrorKind, Event, Text};

fn lexical(value: &str) -> Event<'_> {
    Event::Atom(Atom::Lexical(Text::borrowed(value)))
}

/// Deserializes with the lenient rules of formats where everything is
/// text (like query strings).
fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    deserialize_with_policy(events, deser::de::DuplicateKeys::default())
}

fn deserialize_with_policy<T: DeserializeOwned>(
    events: Vec<Event<'_>>,
    policy: deser::de::DuplicateKeys,
) -> Result<T, Error> {
    deserialize_with_rules(events, policy, LexicalRules::LENIENT)
}

/// Deserializes with the default (strict) rules, like JSON keys.
fn deserialize_strict<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    deserialize_with_rules(
        events,
        deser::de::DuplicateKeys::default(),
        LexicalRules::STRICT,
    )
}

fn deserialize_with_rules<T: DeserializeOwned>(
    events: Vec<Event<'_>>,
    policy: deser::de::DuplicateKeys,
    rules: LexicalRules,
) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        *driver.state_mut().get_mut::<deser::de::DuplicateKeys>() = policy;
        rules.set(driver.state_mut());
        for event in events {
            driver.emit(event)?;
        }
    }
    Ok(out.unwrap())
}

/// The start of a multimap, like the maps of query strings.
fn multimap_start<'a>() -> Event<'a> {
    Event::MapStart(deser::ContainerShape::new().with_multimap(true))
}

/// Events of a map where keys and values are lexical, like a query string.
fn lexical_map<'a>(pairs: &[(&'a str, &'a str)]) -> Vec<Event<'a>> {
    let mut events = vec![multimap_start()];
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
fn test_strict_rules() {
    // the default rules are the ones of text that happens to be text,
    // like the keys of JSON objects
    assert!(deserialize_strict::<bool>(vec![lexical("true")]).unwrap());
    assert!(!deserialize_strict::<bool>(vec![lexical("false")]).unwrap());
    for value in ["True", "yes", "on", "1", "0", "off", ""] {
        let err = deserialize_strict::<bool>(vec![lexical(value)]).unwrap_err();
        assert_eq!(
            err.message(),
            format!("invalid value {:?}, expected bool (true or false)", value)
        );
    }
    assert_eq!(deserialize_strict::<u32>(vec![lexical("42")]).unwrap(), 42);
    assert_eq!(deserialize_strict::<String>(vec![lexical("")]).unwrap(), "");

    // empty text is not a missing value
    assert!(deserialize_strict::<()>(vec![lexical("")]).is_err());
    assert!(deserialize_strict::<Option<u32>>(vec![lexical("")]).is_err());
    assert_eq!(
        deserialize_strict::<Option<String>>(vec![lexical("")]).unwrap(),
        Some(String::new())
    );

    // text is not a sequence
    let err = deserialize_strict::<Vec<u32>>(vec![lexical("42")]).unwrap_err();
    assert_eq!(err.message(), "unexpected string, expected vec");
    assert!(deserialize_strict::<std::collections::BTreeSet<u32>>(vec![lexical("42")]).is_err());
    assert!(deserialize_strict::<[u32; 1]>(vec![lexical("42")]).is_err());

    // map keys parse the same way
    let map = deserialize_strict::<BTreeMap<bool, u32>>(lexical_map(&[("true", "1")])).unwrap();
    assert_eq!(map, BTreeMap::from([(true, 1)]));
    assert!(deserialize_strict::<BTreeMap<bool, u32>>(lexical_map(&[("on", "1")])).is_err());
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
                .emit_borrowed(Atom::Lexical(Text::borrowed(key)))
                .unwrap();
            driver
                .emit_borrowed(Atom::Lexical(Text::borrowed(value)))
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

/// Events of a multimap where values are lexical, keys with more than one
/// value are repeated.
fn query<'a>(pairs: &[(&'a str, &[&'a str])]) -> Vec<Event<'a>> {
    let mut events = vec![multimap_start()];
    for (key, values) in pairs {
        for value in *values {
            events.push(lexical(key));
            events.push(lexical(value));
        }
    }
    events.push(Event::MapEnd);
    events
}

#[test]
fn test_single_value_sequences() {
    use std::collections::{BTreeSet, HashSet, VecDeque};

    #[derive(Debug, Deserialize, PartialEq)]
    struct Collections {
        vec: Vec<u32>,
        deque: VecDeque<String>,
        btree: BTreeSet<bool>,
        hash: HashSet<u8>,
        optional: Option<Vec<u32>>,
        boxed: Box<[u16]>,
        bytes: Vec<u8>,
    }

    // a key given once in a multimap is a collection of one value
    assert_eq!(
        deserialize::<Collections>(lexical_map(&[
            ("vec", "42"),
            ("deque", "x"),
            ("btree", "on"),
            ("hash", "1"),
            ("optional", "1"),
            ("boxed", "7"),
            // bytes are still decoded from lexical atoms
            ("bytes", "aGk="),
        ]))
        .unwrap(),
        Collections {
            vec: vec![42],
            deque: VecDeque::from(["x".to_string()]),
            btree: BTreeSet::from([true]),
            hash: HashSet::from([1]),
            optional: Some(vec![1]),
            boxed: Box::new([7]),
            bytes: b"hi".to_vec(),
        }
    );

    // a missing key is an empty collection (optionals are `None`)
    #[derive(Debug, Deserialize, PartialEq)]
    struct Missing {
        vec: Vec<u32>,
        optional: Option<Vec<u32>>,
        #[deser(default = vec![1])]
        default: Vec<u32>,
    }
    assert_eq!(
        deserialize::<Missing>(lexical_map(&[])).unwrap(),
        Missing {
            vec: vec![],
            optional: None,
            default: vec![1],
        }
    );

    // required collections are not empty
    #[derive(Debug, Deserialize, PartialEq)]
    struct Required {
        #[deser(required)]
        vec: Vec<u32>,
    }
    assert!(deserialize::<Required>(lexical_map(&[])).is_err());

    // in maps that are not multimaps collections are not collected
    let mut events = lexical_map(&[("vec", "42")]);
    events[0] = Event::map_start();
    assert!(deserialize::<Missing>(events).is_err());
    let mut events = lexical_map(&[]);
    events[0] = Event::map_start();
    assert_eq!(
        deserialize::<Missing>(events).unwrap_err().kind(),
        ErrorKind::MissingField
    );

    // a lexical value on its own is not a sequence
    assert!(deserialize::<Vec<u32>>(vec![lexical("42")]).is_err());

    // borrowed values are borrowed
    #[derive(Debug, Deserialize, PartialEq)]
    struct Borrowed<'a> {
        values: Vec<&'a str>,
    }
    let input = String::from("hello");
    let mut out = None::<Borrowed>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        LexicalRules::LENIENT.set(driver.state_mut());
        driver.emit(multimap_start()).unwrap();
        driver.emit("values").unwrap();
        driver
            .emit_borrowed(Atom::Lexical(Text::borrowed(&input)))
            .unwrap();
        driver.emit(Event::MapEnd).unwrap();
    }
    assert_eq!(out.unwrap().values, ["hello"]);
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
        deserialize_with_policy::<Query>(events(), DuplicateKeys::Last).unwrap(),
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

    assert_eq!(
        deserialize_with_policy::<Query>(events(), DuplicateKeys::First).unwrap(),
        Query {
            tags: vec!["a".into()],
            page: 1,
            sort: Some("name".into()),
        }
    );
    // the default
    let err = deserialize::<Query>(events()).unwrap_err();
    assert_eq!(err.message(), "duplicate field `page`");

    // keys do not need to be next to each other
    assert_eq!(
        deserialize::<Query>(lexical_map(&[("tags", "a"), ("page", "1"), ("tags", "b"),]))
            .unwrap()
            .tags,
        ["a", "b"]
    );

    // the values of maps collect too
    let map = deserialize::<BTreeMap<String, Vec<u32>>>(lexical_map(&[
        ("a", "1"),
        ("b", "2"),
        ("a", "3"),
    ]))
    .unwrap();
    assert_eq!(map["a"], [1, 3]);
    assert_eq!(map["b"], [2]);
    // other values follow the policy
    let err =
        deserialize::<BTreeMap<String, u32>>(lexical_map(&[("a", "1"), ("a", "3")])).unwrap_err();
    assert_eq!(err.message(), "duplicate key in map");

    // containers are collected too
    #[derive(Debug, Deserialize, PartialEq)]
    struct Item {
        id: u32,
    }
    #[derive(Debug, Deserialize, PartialEq)]
    struct Items {
        item: Vec<Item>,
    }
    let item = |id| {
        vec![
            lexical("item"),
            multimap_start(),
            lexical("id"),
            lexical(id),
            Event::MapEnd,
        ]
    };
    let mut events = vec![multimap_start()];
    events.extend(item("1"));
    events.extend(item("2"));
    events.push(Event::MapEnd);
    assert_eq!(
        deserialize::<Items>(events).unwrap(),
        Items {
            item: vec![Item { id: 1 }, Item { id: 2 }]
        }
    );
    let mut events = vec![multimap_start()];
    events.extend(item("1"));
    events.push(Event::MapEnd);
    assert_eq!(
        deserialize::<Items>(events).unwrap(),
        Items {
            item: vec![Item { id: 1 }]
        }
    );
}

#[test]
fn test_repeated_update() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Config {
        hosts: Vec<String>,
        other: Vec<String>,
        port: u16,
    }

    // the first value of a key replaces the collection, the others are
    // added
    let mut config = Config {
        hosts: vec!["default".into()],
        other: vec!["kept".into()],
        port: 80,
    };
    {
        let mut driver = DeserializeDriver::update(&mut config);
        LexicalRules::LENIENT.set(driver.state_mut());
        for event in lexical_map(&[("hosts", "a"), ("port", "8080"), ("hosts", "b")]) {
            driver.emit(event).unwrap();
        }
    }
    assert_eq!(
        config,
        Config {
            hosts: vec!["a".into(), "b".into()],
            other: vec!["kept".into()],
            port: 8080,
        }
    );
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
        deserialize_with_policy::<Search>(
            query(&[
                ("limit", &["10", "20"]),
                ("tags", &["1"]),
                ("kind", &["Items"]),
            ]),
            deser::de::DuplicateKeys::Last
        )
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
