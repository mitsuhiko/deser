use std::collections::BTreeMap;

use deser::{Deserialize, ErrorKind, Serialize};
use deser_urlencoded::{
    ArrayFormat, DeserializerConfig, Nesting, Serializer, SerializerConfig, from_str, to_string,
};

#[test]
fn test_serde_url_params() {
    // the example of serde_url_params
    #[derive(Debug, Serialize)]
    #[allow(dead_code)]
    enum Filter {
        New,
        Registered,
        Blocked,
    }

    #[derive(Debug, Serialize)]
    struct Params {
        cursor: Option<usize>,
        per_page: Option<usize>,
        username: String,
        filter: Vec<Filter>,
    }

    let params = Params {
        cursor: Some(42),
        per_page: None,
        username: String::from("boxdot"),
        filter: vec![Filter::New, Filter::Blocked],
    };
    assert_eq!(
        to_string(&params).unwrap(),
        "cursor=42&username=boxdot&filter=New&filter=Blocked"
    );
}

#[test]
fn test_atoms() {
    #[derive(Serialize)]
    struct Atoms<'a> {
        t: bool,
        u: u64,
        i: i64,
        f: f64,
        g: f32,
        c: char,
        s: &'a str,
        big: u128,
        null: Option<u32>,
        unit: (),
        bytes: Vec<u8>,
    }

    assert_eq!(
        to_string(&Atoms {
            t: true,
            u: 1,
            i: -1,
            f: 0.1,
            g: 0.1,
            c: '&',
            s: "a b/ä=?",
            big: u128::MAX,
            null: None,
            unit: (),
            bytes: vec![1, 255],
        })
        .unwrap(),
        "t=true&u=1&i=-1&f=0.1&g=0.1&c=%26&s=a+b%2F%C3%A4%3D%3F\
         &big=340282366920938463463374607431768211455&bytes=Af8%3D"
    );

    let config = SerializerConfig::new().space_as_plus(false);
    assert_eq!(
        config.to_string(&BTreeMap::from([("a b", "c d")])).unwrap(),
        "a%20b=c%20d"
    );
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Item {
    id: u32,
    name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Query {
    q: String,
    tags: Vec<String>,
    scores: Vec<Option<u8>>,
    filter: BTreeMap<String, u32>,
    items: Vec<Item>,
}

fn query() -> Query {
    Query {
        q: "x y".into(),
        tags: vec!["a".into(), "b".into()],
        scores: vec![Some(1), None],
        filter: BTreeMap::from([("min".into(), 1), ("max".into(), 5)]),
        items: vec![
            Item {
                id: 1,
                name: Some("one".into()),
            },
            Item { id: 2, name: None },
        ],
    }
}

#[test]
fn test_arrays_and_nesting() {
    let config = SerializerConfig::new().arrays(ArrayFormat::Indices);
    let out = config.to_string(&query()).unwrap();
    assert_eq!(
        out,
        "q=x+y&tags%5B0%5D=a&tags%5B1%5D=b&scores%5B0%5D=1&scores%5B1%5D=\
         &filter%5Bmax%5D=5&filter%5Bmin%5D=1&items%5B0%5D%5Bid%5D=1\
         &items%5B0%5D%5Bname%5D=one&items%5B1%5D%5Bid%5D=2"
    );
    assert_eq!(from_str::<Query>(&out).unwrap(), query());

    let config = config.nesting(Nesting::Dots);
    let out = config.to_string(&query()).unwrap();
    assert_eq!(
        out,
        "q=x+y&tags.0=a&tags.1=b&scores.0=1&scores.1=&filter.max=5&filter.min=1\
         &items.0.id=1&items.0.name=one&items.1.id=2"
    );
    let dots = DeserializerConfig::new().nesting(Nesting::Dots);
    assert_eq!(dots.from_str::<Query>(&out).unwrap(), query());

    // the other formats do not support sequences of maps
    for arrays in [ArrayFormat::Repeat, ArrayFormat::Brackets] {
        let err = SerializerConfig::new()
            .arrays(arrays)
            .to_string(&query())
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnsupportedType);
        assert_eq!(
            err.message(),
            "sequences of maps or sequences require ArrayFormat::Indices"
        );
    }

    // they round trip otherwise
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Simple {
        tags: Vec<String>,
        one: Vec<u32>,
        filter: BTreeMap<String, Vec<bool>>,
    }
    let simple = Simple {
        tags: vec!["a".into(), "b".into()],
        one: vec![1],
        filter: BTreeMap::from([("x".into(), vec![true, false])]),
    };
    let out = to_string(&simple).unwrap();
    assert_eq!(
        out,
        "tags=a&tags=b&one=1&filter%5Bx%5D=true&filter%5Bx%5D=false"
    );
    assert_eq!(from_str::<Simple>(&out).unwrap(), simple);
    let out = SerializerConfig::new()
        .arrays(ArrayFormat::Brackets)
        .to_string(&simple)
        .unwrap();
    assert_eq!(
        out,
        "tags%5B%5D=a&tags%5B%5D=b&one%5B%5D=1\
         &filter%5Bx%5D%5B%5D=true&filter%5Bx%5D%5B%5D=false"
    );
    assert_eq!(from_str::<Simple>(&out).unwrap(), simple);

    let err = SerializerConfig::new()
        .nesting(Nesting::Flat)
        .to_string(&simple)
        .unwrap_err();
    assert_eq!(
        err.message(),
        "nested maps are not supported with Nesting::Flat"
    );
}

#[test]
fn test_enums() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    enum Filter {
        Name(String),
        Range { min: u32, max: u32 },
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[deser(tag = "type")]
    enum Tagged {
        Page { number: u32, exact: bool },
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Query {
        filter: Filter,
        names: Vec<Filter>,
    }

    let value = Query {
        filter: Filter::Range { min: 1, max: 2 },
        names: vec![Filter::Name("a".into())],
    };
    let config = SerializerConfig::new().arrays(ArrayFormat::Indices);
    let out = config.to_string(&value).unwrap();
    assert_eq!(
        out,
        "filter%5BRange%5D%5Bmin%5D=1&filter%5BRange%5D%5Bmax%5D=2&names%5B0%5D%5BName%5D=a"
    );
    assert_eq!(from_str::<Query>(&out).unwrap(), value);

    // the tag of internally tagged enums is a parameter
    let value = Tagged::Page {
        number: 3,
        exact: true,
    };
    let out = to_string(&value).unwrap();
    assert_eq!(out, "type=Page&number=3&exact=true");
    assert_eq!(from_str::<Tagged>(&out).unwrap(), value);
    assert_eq!(
        from_str::<Tagged>("exact=on&number=3&type=Page").unwrap(),
        value
    );
}

#[test]
fn test_top_level() {
    // sequences of pairs
    let pairs = vec![("a", "1"), ("b", "2"), ("a", "3")];
    assert_eq!(to_string(&pairs).unwrap(), "a=1&b=2&a=3");
    let pairs = vec![("a", vec![1, 2])];
    assert_eq!(to_string(&pairs).unwrap(), "a=1&a=2");

    // no value
    assert_eq!(to_string(&None::<BTreeMap<String, u32>>).unwrap(), "");
    assert_eq!(to_string(&BTreeMap::<String, u32>::new()).unwrap(), "");

    for value in [&42 as &dyn Serialize, &vec![1, 2], &vec![("a", 1, 2)]] {
        let err = to_string(value).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UnsupportedType);
    }

    // maps as keys are not supported
    let err = to_string(&BTreeMap::from([(vec![1], 1)])).unwrap_err();
    assert_eq!(
        err.message(),
        "keys of query strings must be strings, numbers or booleans"
    );

    // more than one value is joined
    let mut serializer = Serializer::new();
    serializer.serialize(&BTreeMap::from([("a", 1)])).unwrap();
    serializer
        .serialize(&BTreeMap::<String, u32>::new())
        .unwrap();
    serializer.serialize(&vec![("b", 2)]).unwrap();
    assert_eq!(serializer.finish(), "a=1&b=2");
}
