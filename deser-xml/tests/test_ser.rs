use std::collections::BTreeMap;

use deser::{Deserialize, Serialize};
use deser_xml::{SerializerConfig, from_str, to_string};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(rename_all = "lowercase")]
enum Shape {
    Circle {
        #[deser(rename = "@r")]
        r: f64,
    },
    Square {
        side: f64,
    },
    Empty,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Address {
    #[deser(rename = "@country")]
    country: String,
    city: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[deser(rename = "person")]
struct Person {
    #[deser(rename = "@id")]
    id: u32,
    name: String,
    age: Option<u32>,
    tag: Vec<String>,
    address: Address,
    shape: Vec<Shape>,
    note: Note,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Note {
    #[deser(rename = "@lang")]
    lang: String,
    #[deser(rename = "$text")]
    text: String,
}

fn person() -> Person {
    Person {
        id: 7,
        name: "Jane <J> & Co".into(),
        age: None,
        tag: vec!["a".into(), "b".into()],
        address: Address {
            country: "AT".into(),
            city: "Vienna".into(),
        },
        shape: vec![
            Shape::Circle { r: 1.5 },
            Shape::Square { side: 2.0 },
            Shape::Empty,
        ],
        note: Note {
            lang: "en".into(),
            text: "hi \"there\"".into(),
        },
    }
}

#[test]
fn test_round_trip() {
    let out = to_string(&person()).unwrap();
    assert_eq!(
        out,
        "<person id=\"7\"><name>Jane &lt;J&gt; &amp; Co</name><tag>a</tag><tag>b</tag>\
         <address country=\"AT\"><city>Vienna</city></address>\
         <shape><circle r=\"1.5\"/></shape><shape><square><side>2</side></square></shape>\
         <shape>empty</shape><note lang=\"en\">hi \"there\"</note></person>"
    );
    assert_eq!(from_str::<Person>(&out).unwrap(), person());
}

#[test]
fn test_root() {
    // atoms at the root with a name
    assert_eq!(
        SerializerConfig::new().root("n").to_string(&42).unwrap(),
        "<n>42</n>"
    );
    assert_eq!(
        SerializerConfig::new()
            .root("n")
            .to_string(&None::<u32>)
            .unwrap(),
        "<n/>"
    );
    // maps need a name
    let map = BTreeMap::from([("a", 1)]);
    assert!(to_string(&map).is_err());
    assert_eq!(
        SerializerConfig::new().root("m").to_string(&map).unwrap(),
        "<m><a>1</a></m>"
    );
    // empty structs
    #[derive(Serialize)]
    struct Empty {}
    assert_eq!(to_string(&Empty {}).unwrap(), "<Empty/>");
    // sequences have no root
    assert!(
        SerializerConfig::new()
            .root("s")
            .to_string(&[1, 2])
            .is_err()
    );

    assert_eq!(
        SerializerConfig::new()
            .root("n")
            .declaration(true)
            .to_string(&1)
            .unwrap(),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><n>1</n>"
    );
}

#[test]
fn test_values() {
    #[derive(Serialize)]
    struct Values {
        #[deser(rename = "@attr")]
        attr: String,
        float: f64,
        inf: f64,
        nan: f64,
        flag: bool,
        bytes: Vec<u8>,
        nulls: Vec<Option<u32>>,
        empty: String,
    }
    assert_eq!(
        to_string(&Values {
            attr: "a\"b\n<c>".into(),
            float: 1.5,
            inf: f64::NEG_INFINITY,
            nan: f64::NAN,
            flag: true,
            bytes: b"hi".to_vec(),
            nulls: vec![Some(1), None],
            empty: String::new(),
        })
        .unwrap(),
        "<Values attr=\"a&quot;b&#10;&lt;c&gt;\"><float>1.5</float><inf>-INF</inf>\
         <nan>NaN</nan><flag>true</flag><bytes>aGk=</bytes><nulls>1</nulls><nulls/>\
         <empty/></Values>"
    );
}

#[test]
fn test_namespaces() {
    #[derive(Serialize)]
    #[deser(rename = "feed")]
    struct Feed {
        title: String,
        #[deser(rename = "dc:creator")]
        creator: String,
    }
    const CONFIG: SerializerConfig = SerializerConfig::new().namespaces(&[
        ("", "http://www.w3.org/2005/Atom"),
        ("dc", "http://purl.org/dc/elements/1.1/"),
    ]);
    assert_eq!(
        CONFIG
            .to_string(&Feed {
                title: "x".into(),
                creator: "y".into()
            })
            .unwrap(),
        "<feed xmlns=\"http://www.w3.org/2005/Atom\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
         <title>x</title><dc:creator>y</dc:creator></feed>"
    );
}

#[test]
fn test_errors() {
    // attributes after content
    #[derive(Serialize)]
    struct Late {
        a: u32,
        #[deser(rename = "@b")]
        b: u32,
    }
    let err = to_string(&Late { a: 1, b: 2 }).unwrap_err();
    assert_eq!(
        err.message(),
        "attribute `b` comes after the content of the element"
    );

    // attributes that are not values
    #[derive(Serialize)]
    struct Nested {
        #[deser(rename = "@a")]
        a: Vec<u32>,
    }
    assert!(to_string(&Nested { a: vec![1] }).is_err());

    // names that are not names
    let map = BTreeMap::from([("not a name", 1)]);
    assert!(SerializerConfig::new().root("m").to_string(&map).is_err());

    // characters that cannot be written
    assert!(
        SerializerConfig::new()
            .root("m")
            .to_string(&"\u{1}")
            .is_err()
    );

    // sequences in sequences
    let map = BTreeMap::from([("a", vec![vec![1]])]);
    assert!(SerializerConfig::new().root("m").to_string(&map).is_err());
}

#[test]
fn test_value_round_trip() {
    // documents read into values write back the same way (element order is
    // grouped by name, text is kept)
    let input = r#"<doc id="1"><a>1</a><a>2</a><b x="y">text</b><c/></doc>"#;
    let value: deser_value::Value = from_str(input).unwrap();
    assert_eq!(
        SerializerConfig::new()
            .root("doc")
            .to_string(&value)
            .unwrap(),
        input
    );
}
