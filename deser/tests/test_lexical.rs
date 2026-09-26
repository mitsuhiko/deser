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
