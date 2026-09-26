//! Many of these tests are the cases that people reported against the
//! serde based libraries (serde_urlencoded, serde_qs, serde_html_form).
use std::collections::{BTreeMap, HashMap};

use deser::de::DuplicateKeys;
use deser::{Deserialize, ErrorKind};
use deser_path::{Path, PathLayer};
use deser_urlencoded::{Deserializer, DeserializerConfig, Nesting, from_slice, from_str};

#[test]
fn test_basics() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Query<'a> {
        q: &'a str,
        page: u32,
        exact: bool,
        ratio: f32,
    }

    let input = String::from("?q=deser&page=2&exact=on&ratio=0.5&unknown=1");
    let query: Query = from_str(&input).unwrap();
    assert_eq!(
        query,
        Query {
            q: "deser",
            page: 2,
            exact: true,
            ratio: 0.5,
        }
    );
    // values without escapes are borrowed
    assert!(input.as_bytes().as_ptr_range().contains(&query.q.as_ptr()));

    // decoding
    let map: BTreeMap<String, String> = from_str("a+b=c+d&e=%C3%A4%20%26&f&g=&=h&&i==").unwrap();
    assert_eq!(
        map,
        BTreeMap::from([
            ("a b".into(), "c d".into()),
            ("e".into(), "\u{e4} &".into()),
            ("f".into(), "".into()),
            ("g".into(), "".into()),
            ("".into(), "h".into()),
            ("i".into(), "=".into()),
        ])
    );
    let map: BTreeMap<String, String> = from_str("").unwrap();
    assert!(map.is_empty());
}

#[test]
fn test_numbers_in_buffered_values() {
    // serde_urlencoded#33, serde_qs#14, serde_qs#159
    #[derive(Debug, Deserialize, PartialEq)]
    struct Paginate {
        limit: u64,
        offset: u64,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Query {
        #[deser(flatten)]
        paginate: Paginate,
    }

    assert_eq!(
        from_str::<Query>("limit=10&offset=0").unwrap(),
        Query {
            paginate: Paginate {
                limit: 10,
                offset: 0
            }
        }
    );

    // serde_urlencoded#26, serde_qs#153
    #[derive(Debug, Deserialize, PartialEq)]
    struct ItemStruct {
        field: i32,
        option: bool,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "tag")]
    enum Test {
        Item(ItemStruct),
    }

    assert_eq!(
        from_str::<Test>("field=42&option=true&tag=Item").unwrap(),
        Test::Item(ItemStruct {
            field: 42,
            option: true
        })
    );

    // serde_urlencoded#66
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(untagged)]
    enum Q2 {
        A { foo: String },
        B { bar: i32 },
    }

    assert_eq!(from_str::<Q2>("bar=123").unwrap(), Q2::B { bar: 123 });
    assert_eq!(
        from_str::<Q2>("foo=123").unwrap(),
        Q2::A { foo: "123".into() }
    );
}

#[test]
fn test_sequences() {
    // serde_urlencoded#6, #109, #123: `<select multiple>` sends repeated keys
    #[derive(Debug, Deserialize, PartialEq)]
    struct Form {
        #[deser(default)]
        multi: Vec<String>,
        #[deser(default)]
        ids: Vec<u64>,
    }

    assert_eq!(
        from_str::<Form>("multi=hello&ids=1&multi=world&ids=2").unwrap(),
        Form {
            multi: vec!["hello".into(), "world".into()],
            ids: vec![1, 2],
        }
    );
    // a single value (serde_html_form#19, #11)
    assert_eq!(
        from_str::<Form>("multi=hello&ids=1").unwrap(),
        Form {
            multi: vec!["hello".into()],
            ids: vec![1],
        }
    );
    // no value (serde_html_form#2)
    assert_eq!(
        from_str::<Form>("").unwrap(),
        Form {
            multi: vec![],
            ids: vec![]
        }
    );

    #[derive(Debug, Deserialize, PartialEq)]
    struct Optional {
        value: Option<Vec<i32>>,
    }

    assert_eq!(from_str::<Optional>("").unwrap(), Optional { value: None });
    assert_eq!(
        from_str::<Optional>("value=1").unwrap(),
        Optional {
            value: Some(vec![1])
        }
    );
    assert_eq!(
        from_str::<Optional>("value=1&value=2").unwrap(),
        Optional {
            value: Some(vec![1, 2])
        }
    );

    // brackets and indexes (serde_qs#35, #16), also encoded like browsers
    // send them (serde_qs#21, #44, #55)
    for input in [
        "multi[]=hello&multi[]=world",
        "multi%5B%5D=hello&multi%5B%5D=world",
        "multi[1]=world&multi[0]=hello",
        "multi%5b0%5d=hello&multi%5b1%5d=world",
    ] {
        assert_eq!(
            from_str::<Form>(input).unwrap().multi,
            ["hello", "world"],
            "{}",
            input
        );
    }
    // brackets are always a sequence
    let map: BTreeMap<String, serde_like::Value> = from_str("a[]=1").unwrap();
    assert_eq!(map["a"], serde_like::Value::Seq(vec!["1".into()]));

    // a multimap
    let map: HashMap<String, Vec<String>> = from_str("a=1&b=2&a=3").unwrap();
    assert_eq!(map["a"], ["1", "3"]);
    assert_eq!(map["b"], ["2"]);
}

/// A tiny dynamic value to look at the shape of the input.
mod serde_like {
    use deser::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(untagged)]
    pub enum Value {
        Str(String),
        Seq(Vec<Value>),
        Map(std::collections::BTreeMap<String, Value>),
    }

    impl From<&str> for Value {
        fn from(value: &str) -> Value {
            Value::Str(value.into())
        }
    }
}

#[test]
fn test_flatten_sequences() {
    // serde_html_form#25, #6
    #[derive(Debug, Deserialize, PartialEq)]
    struct Seq {
        #[deser(default)]
        seq: Vec<String>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct WithFlatten {
        #[deser(flatten)]
        inner: Seq,
    }

    for (input, expected) in [
        ("seq=1", vec!["1"]),
        ("seq=1&seq=2", vec!["1", "2"]),
        ("", vec![]),
    ] {
        assert_eq!(
            from_str::<WithFlatten>(input).unwrap().inner.seq,
            expected,
            "{}",
            input
        );
    }
}

#[test]
fn test_nested() {
    // serde_urlencoded#15, #38, serde_html_form#31
    #[derive(Debug, Deserialize, PartialEq)]
    struct Book {
        title: String,
        pages: Option<u32>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Shelf {
        name: String,
        books: Vec<Book>,
        meta: BTreeMap<String, u32>,
    }

    let expected = Shelf {
        name: "fiction".into(),
        books: vec![
            Book {
                title: "Dune".into(),
                pages: Some(412),
            },
            Book {
                title: "Emma".into(),
                pages: None,
            },
        ],
        meta: BTreeMap::from([("rows".into(), 2), ("cols".into(), 3)]),
    };
    assert_eq!(
        from_str::<Shelf>(
            "name=fiction&books[0][title]=Dune&books[1][title]=Emma&books[0][pages]=412\
             &books[1][pages]=&meta[rows]=2&meta[cols]=3"
        )
        .unwrap(),
        expected
    );

    const DOTS: DeserializerConfig = DeserializerConfig::new().nesting(Nesting::Dots);
    assert_eq!(
        DOTS.from_str::<Shelf>(
            "name=fiction&books.0.title=Dune&books.0.pages=412&books.1.title=Emma\
             &meta.rows=2&meta.cols=3"
        )
        .unwrap(),
        expected
    );

    // maps with integer keys (serde_qs#167)
    #[derive(Debug, Deserialize)]
    struct Test {
        example: HashMap<i32, i32>,
    }
    let test: Test = from_str("example[123]=321&unrelated[123]=321").unwrap();
    assert_eq!(test.example[&123], 321);
    let test: Test = from_str("example[1]=2&example[5]=6").unwrap();
    assert_eq!(test.example[&5], 6);
}

#[test]
fn test_enums() {
    // serde_qs#6, #150
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(rename_all = "lowercase")]
    enum Order {
        Asc,
        Desc,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    enum Filter {
        Name(String),
        Range { min: u32, max: u32 },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "type", content = "value")]
    enum Adjacent {
        Limit(u32),
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Query {
        order: Order,
        filter: Filter,
        adjacent: Adjacent,
        orders: Vec<Order>,
    }

    assert_eq!(
        from_str::<Query>(
            "order=desc&filter[Range][min]=1&filter[Range][max]=5\
             &adjacent[type]=Limit&adjacent[value]=10&orders=asc&orders=desc"
        )
        .unwrap(),
        Query {
            order: Order::Desc,
            filter: Filter::Range { min: 1, max: 5 },
            adjacent: Adjacent::Limit(10),
            orders: vec![Order::Asc, Order::Desc],
        }
    );
}

#[test]
fn test_empty_values() {
    // serde_urlencoded#36, serde_html_form#13
    #[derive(Debug, Deserialize, PartialEq)]
    struct Form {
        age: Option<i32>,
        name: Option<String>,
        score: Option<f64>,
        names: Option<Vec<String>>,
    }

    assert_eq!(
        from_str::<Form>("age=&name=&score=&names=").unwrap(),
        Form {
            age: None,
            name: Some("".into()),
            score: None,
            names: Some(vec!["".into()]),
        }
    );
    // `a` is the same as `a=` (serde_html_form#21)
    assert_eq!(
        from_str::<Form>("age&name").unwrap(),
        Form {
            age: None,
            name: Some("".into()),
            score: None,
            names: None,
        }
    );
    // required values are still required
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Required {
        age: i32,
    }
    let err = from_str::<Required>("age=").unwrap_err();
    assert_eq!(err.message(), "invalid value \"\", expected i32");
}

#[test]
fn test_checkboxes() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Form {
        #[deser(default)]
        subscribe: bool,
        #[deser(default)]
        toppings: Vec<String>,
    }

    // a checked checkbox sends `on`, an unchecked one nothing
    assert_eq!(
        from_str::<Form>("subscribe=on&toppings=bacon&toppings=onion").unwrap(),
        Form {
            subscribe: true,
            toppings: vec!["bacon".into(), "onion".into()],
        }
    );
    assert_eq!(
        from_str::<Form>("").unwrap(),
        Form {
            subscribe: false,
            toppings: vec![],
        }
    );
    // a hidden input before the checkbox, the last value wins (serde_qs#68)
    assert!(
        from_str::<Form>("subscribe=false&subscribe=true")
            .unwrap()
            .subscribe
    );
    assert!(!from_str::<Form>("subscribe=0").unwrap().subscribe);
}

#[test]
fn test_duplicate_keys() {
    // serde_qs#161
    #[derive(Debug, Deserialize, PartialEq)]
    struct Body {
        single: String,
        multi: Vec<String>,
    }

    let input = "single=x&single=y&multi=a&multi=b";
    assert_eq!(from_str::<Body>(input).unwrap().single, "y");

    const FIRST: DeserializerConfig =
        DeserializerConfig::new().duplicate_keys(DuplicateKeys::First);
    assert_eq!(FIRST.from_str::<Body>(input).unwrap().single, "x");

    const STRICT: DeserializerConfig =
        DeserializerConfig::new().duplicate_keys(DuplicateKeys::Error);
    let err = Deserializer::from_str_with_config(input, &STRICT)
        .deserialize_with::<Body, _>(|driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    assert_eq!(err.message(), "duplicate key");
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "single[1]");
    // sequences still get all values
    assert_eq!(
        STRICT
            .from_str::<Body>("single=x&multi=a&multi=b")
            .unwrap()
            .multi,
        ["a", "b"]
    );
}

#[test]
fn test_bytes() {
    // serde_qs#163: bytes that are not UTF-8 are passed on as bytes
    #[derive(Debug, Deserialize)]
    struct Params {
        greeting: Vec<u8>,
    }

    let params: Params = from_str("greeting=hello_%ff_world").unwrap();
    assert_eq!(params.greeting, b"hello_\xff_world");
    // other strings are base64
    let params: Params = from_str("greeting=aGk%3D").unwrap();
    assert_eq!(params.greeting, b"hi");

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Text {
        greeting: String,
    }
    let err = from_str::<Text>("greeting=%ff").unwrap_err();
    assert_eq!(err.message(), "unexpected bytes, expected string");

    // the input needs to be UTF-8 (percent-encoded it's ASCII)
    let err = from_slice::<Text>(b"greeting=\xff").unwrap_err();
    assert_eq!(err.message(), "input is not valid UTF-8");
    assert_eq!(err.offset(), Some(9));
}

#[test]
fn test_errors() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Item {
        id: u32,
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Query {
        items: Vec<Item>,
    }

    let err = Deserializer::from_str("items[0][id]=1&items[1][id]=x")
        .deserialize_with::<Query, _>(|driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    assert_eq!(err.message(), "invalid value \"x\", expected u32");
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "items[1].id");
    assert_eq!((err.offset(), err.column()), (Some(28), Some(29)));

    let err = Deserializer::from_str("items[0][name]=1")
        .deserialize_with::<Query, _>(|driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::MissingField);
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "items[0]");
    assert_eq!(err.offset(), Some(0));

    type Map = BTreeMap<String, serde_like::Value>;

    let err = from_str::<Map>("a=1&a[b]=2").unwrap_err();
    assert_eq!(err.message(), "key has a value and nested keys");
    assert_eq!(err.offset(), Some(0));

    let err = from_str::<Map>("x=1&a[]=1&a[0]=2").unwrap_err();
    assert_eq!(
        err.message(),
        "`[]` cannot be combined with other nested keys"
    );
    assert_eq!(err.offset(), Some(4));

    // limits
    let err = from_str::<Map>(&format!("a{}=1", "[b]".repeat(17))).unwrap_err();
    assert_eq!(err.message(), "key is nested too deeply");
    assert!(from_str::<Map>(&format!("a{}=1", "[b]".repeat(16))).is_ok());
    let config = DeserializerConfig::new().max_depth(2);
    assert!(config.from_str::<Map>("a[b][c][d]=1").is_err());

    let config = DeserializerConfig::new().max_params(2);
    let err = config.from_str::<Map>("a=1&b=2&&c=3").unwrap_err();
    assert_eq!(err.message(), "too many parameters");
    assert_eq!(err.offset(), Some(9));

    // huge indexes are map keys, nothing is allocated for them
    let map: Map = from_str("a[99999999999]=1").unwrap();
    assert_eq!(
        map["a"],
        serde_like::Value::Map(BTreeMap::from([("99999999999".into(), "1".into())]))
    );
}

#[test]
fn test_malformed_keys() {
    let map: BTreeMap<String, String> = from_str("a[b=1&[c]=2&d]=3&e[f]g=4").unwrap();
    assert_eq!(
        map.keys().collect::<Vec<_>>(),
        ["[c]", "a[b", "d]", "e[f]g"]
    );
}

#[test]
fn test_values() {
    use deser_value::Value;

    // lexical atoms and repeated keys are retained in values
    let value: Value = from_str("page=2&tags=a&tags=b&filter[age]=30").unwrap();
    assert!(value["page"].is_lexical());
    assert!(value["tags"].as_seq().unwrap().is_repeated());
    assert_eq!(value["filter"]["age"], "30");

    #[derive(Debug, Deserialize, PartialEq)]
    struct Filter {
        age: u8,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Query {
        page: u32,
        tags: String,
        filter: Filter,
    }

    assert_eq!(
        deser_value::from_value::<Query>(&value).unwrap(),
        Query {
            page: 2,
            tags: "b".into(),
            filter: Filter { age: 30 },
        }
    );
}

#[test]
#[cfg(feature = "io")]
fn test_locations() {
    use deser::io::Reader;

    // from streams, like the body of a request
    let mut reader = Reader::new(&b"a=1&b=x"[..], DeserializerConfig::new());
    let err = reader.read::<BTreeMap<String, u32>>().unwrap_err();
    assert_eq!(err.message(), "invalid value \"x\", expected u32");
    assert_eq!((err.line(), err.column()), (Some(1), Some(7)));
}
