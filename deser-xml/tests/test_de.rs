use std::collections::BTreeMap;

use deser::Deserialize;
use deser::de::DuplicateKeys;
use deser_path::{Path, PathLayer};
use deser_value::Value;
use deser_xml::{Deserializer, DeserializerConfig, from_str};

fn to_json(xml: &str) -> String {
    deser_json::to_string(&from_str::<Value>(xml).unwrap()).unwrap()
}

#[test]
fn test_data_model() {
    assert_eq!(to_json("<a>1</a>"), r#""1""#);
    assert_eq!(to_json("<a/>"), r#""""#);
    assert_eq!(to_json("<a></a>"), r#""""#);
    assert_eq!(to_json(r#"<a href="x"/>"#), r#"{"@href":"x"}"#);
    assert_eq!(
        to_json(r#"<a href="x">y</a>"#),
        r#"{"@href":"x","$text":"y"}"#
    );
    assert_eq!(
        to_json("<a> <b>1</b>\n  <c>2</c> </a>"),
        r#"{"b":"1","c":"2"}"#
    );
    // repeated elements are grouped by values
    assert_eq!(
        to_json("<a><b>1</b><c/><b>2</b></a>"),
        r#"{"b":["1","2"],"c":""}"#
    );
    // mixed content
    assert_eq!(
        to_json("<p>x <b>y</b> z</p>"),
        r#"{"$text":["x "," z"],"b":"y"}"#
    );
    // the prolog, comments and processing instructions are not data
    assert_eq!(
        to_json("<?xml version=\"1.0\"?>\n<!-- c --><!DOCTYPE a><a><?pi x?>1<!-- c -->2</a>\n"),
        r#""12""#
    );
}

#[test]
fn test_text() {
    // references, CDATA and line breaks
    assert_eq!(
        from_str::<String>("<a>a &amp; b &lt;&#x41;&#66;&gt; <![CDATA[<c>]]></a>").unwrap(),
        "a & b <AB> <c>"
    );
    assert_eq!(from_str::<String>("<a>x\r\ny</a>").unwrap(), "x\ny");
    // attribute values are normalized, references are kept
    let value: BTreeMap<String, String> = from_str("<a x=\"1 &amp;\n2&#10;\"/>").unwrap();
    assert_eq!(value["@x"], "1 & 2\n");
    let err = from_str::<String>("<a>&custom;</a>").unwrap_err();
    assert_eq!(err.message(), "unknown entity `&custom;`");

    // text without references is borrowed
    #[derive(Debug, Deserialize)]
    struct Borrowed<'a> {
        name: &'a str,
        #[deser(rename = "@id")]
        id: &'a str,
    }
    let input = String::from(r#"<a id="x1"><name>jane</name></a>"#);
    let value: Borrowed = from_str(&input).unwrap();
    assert_eq!((value.name, value.id), ("jane", "x1"));
}

#[test]
fn test_structs() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Address {
        city: String,
        #[deser(rename = "@country")]
        country: String,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Person {
        #[deser(rename = "@id")]
        id: u32,
        name: String,
        age: Option<u32>,
        email: Option<String>,
        active: bool,
        address: Address,
        tag: Vec<String>,
        phone: Vec<String>,
    }

    let person: Person = from_str(
        r#"
        <person id="7">
          <name>Jane</name>
          <tag>a</tag>
          <age/>
          <active>true</active>
          <address country="AT"><city>Vienna</city></address>
          <tag>b</tag>
        </person>
    "#,
    )
    .unwrap();
    assert_eq!(
        person,
        Person {
            id: 7,
            name: "Jane".into(),
            age: None,
            email: None,
            active: true,
            address: Address {
                city: "Vienna".into(),
                country: "AT".into(),
            },
            tag: vec!["a".into(), "b".into()],
            phone: vec![],
        }
    );

    // a single element is a collection of one
    #[derive(Debug, Deserialize, PartialEq)]
    struct Order {
        item: Vec<Item>,
    }
    #[derive(Debug, Deserialize, PartialEq)]
    struct Item {
        #[deser(rename = "@sku")]
        sku: String,
        qty: u32,
    }
    assert_eq!(
        from_str::<Order>(r#"<order><item sku="a"><qty>2</qty></item></order>"#).unwrap(),
        Order {
            item: vec![Item {
                sku: "a".into(),
                qty: 2
            }]
        }
    );

    // a repeated element for a single value
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Single {
        name: String,
    }
    let err = from_str::<Single>("<a><name>x</name><name>y</name></a>").unwrap_err();
    assert_eq!(err.message(), "duplicate field `name`");
    const LAST: DeserializerConfig = DeserializerConfig::new().duplicate_keys(DuplicateKeys::Last);
    assert_eq!(
        LAST.from_str::<Single>("<a><name>x</name><name>y</name></a>")
            .unwrap()
            .name,
        "y"
    );

    // maps of child elements
    let map: BTreeMap<String, u32> = from_str("<a><x>1</x><y>2</y></a>").unwrap();
    assert_eq!(map, BTreeMap::from([("x".into(), 1), ("y".into(), 2)]));
    let map: BTreeMap<String, Vec<u32>> = from_str("<a><x>1</x><y>2</y><x>3</x></a>").unwrap();
    assert_eq!(map["x"], [1, 3]);
}

#[test]
fn test_enums() {
    #[derive(Debug, Deserialize, PartialEq)]
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

    #[derive(Debug, Deserialize, PartialEq)]
    struct Drawing {
        shape: Vec<Shape>,
    }

    assert_eq!(
        from_str::<Drawing>(
            r#"<drawing>
                 <shape><circle r="1.5"/></shape>
                 <shape><square><side>2</side></square></shape>
                 <shape>empty</shape>
               </drawing>"#
        )
        .unwrap(),
        Drawing {
            shape: vec![
                Shape::Circle { r: 1.5 },
                Shape::Square { side: 2.0 },
                Shape::Empty
            ]
        }
    );

    // an attribute as tag of internally tagged enums
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "@type", rename_all = "lowercase")]
    enum Payment {
        Card { number: String },
        Transfer { iban: String },
    }
    assert_eq!(
        from_str::<Payment>(r#"<payment type="transfer"><iban>AT12</iban></payment>"#).unwrap(),
        Payment::Transfer {
            iban: "AT12".into()
        }
    );
    // the tag can come after the fields (they are buffered)
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "kind", rename_all = "lowercase")]
    enum Event {
        Click { x: u32, tag: Vec<String> },
    }
    assert_eq!(
        from_str::<Event>("<e><x>1</x><tag>a</tag><kind>click</kind><tag>b</tag></e>").unwrap(),
        Event::Click {
            x: 1,
            tag: vec!["a".into(), "b".into()]
        }
    );

    // untagged
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(untagged)]
    enum Value {
        Number(u32),
        Text(String),
    }
    assert_eq!(from_str::<Value>("<v>42</v>").unwrap(), Value::Number(42));
    assert_eq!(
        from_str::<Value>("<v>x</v>").unwrap(),
        Value::Text("x".into())
    );
}

#[test]
fn test_flatten() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Common {
        #[deser(rename = "@id")]
        id: String,
        #[deser(rename = "@class")]
        class: Option<String>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Div {
        #[deser(flatten)]
        common: Common,
        p: Vec<String>,
        #[deser(flatten)]
        rest: BTreeMap<String, String>,
    }

    assert_eq!(
        from_str::<Div>(r#"<div id="x" data-a="1"><p>a</p><span>s</span><p>b</p></div>"#).unwrap(),
        Div {
            common: Common {
                id: "x".into(),
                class: None,
            },
            p: vec!["a".into(), "b".into()],
            rest: BTreeMap::from([("@data-a".into(), "1".into()), ("span".into(), "s".into())]),
        }
    );
}

#[test]
fn test_namespaces() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Envelope {
        #[deser(rename = "soap:Body")]
        body: Body,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Body {
        #[deser(rename = "m:price")]
        price: u32,
    }

    // names as written
    let doc = r#"<soap:Envelope xmlns:soap="http://www.w3.org/2003/05/soap-envelope"
                               xmlns:m="urn:shop">
                   <soap:Body><m:price>10</m:price></soap:Body>
                 </soap:Envelope>"#;
    assert_eq!(
        from_str::<Envelope>(doc).unwrap(),
        Envelope {
            body: Body { price: 10 }
        }
    );

    // with other prefixes in the document, the configured ones are used
    const CONFIG: DeserializerConfig = DeserializerConfig::new().namespaces(&[
        ("soap", "http://www.w3.org/2003/05/soap-envelope"),
        ("m", "urn:shop"),
    ]);
    let doc = r#"<env:Envelope xmlns:env="http://www.w3.org/2003/05/soap-envelope">
                   <env:Body><price xmlns="urn:shop">10</price></env:Body>
                 </env:Envelope>"#;
    assert_eq!(
        CONFIG.from_str::<Envelope>(doc).unwrap(),
        Envelope {
            body: Body { price: 10 }
        }
    );
    // without the configuration the names are the ones of the document
    assert!(from_str::<Envelope>(doc).is_err());

    // attributes without prefix have no namespace
    const XLINK: DeserializerConfig =
        DeserializerConfig::new().namespaces(&[("xlink", "http://www.w3.org/1999/xlink")]);
    let value: BTreeMap<String, String> = XLINK
        .from_str(r#"<a xmlns:l="http://www.w3.org/1999/xlink" l:href="x" href="y"/>"#)
        .unwrap();
    assert_eq!(value["@xlink:href"], "x");
    assert_eq!(value["@href"], "y");
}

#[test]
fn test_config() {
    const CONFIG: DeserializerConfig = DeserializerConfig::new()
        .attribute_prefix("")
        .text_key("#text");
    let value: BTreeMap<String, String> = CONFIG.from_str(r#"<a href="x">y</a>"#).unwrap();
    assert_eq!(value["href"], "x");
    assert_eq!(value["#text"], "y");
}

#[test]
fn test_errors() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Item {
        qty: u32,
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Order {
        item: Vec<Item>,
    }

    let input = "<order>\n  <item><qty>1</qty></item>\n  <item><qty>x</qty></item>\n</order>";
    let err = Deserializer::from_str(input)
        .deserialize_with::<Order, _>(|driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    assert_eq!(err.message(), "invalid value \"x\", expected u32");
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "item.qty");
    assert_eq!((err.line(), err.column()), (Some(3), Some(14)));

    // malformed documents
    for (input, message) in [
        ("", "no root element"),
        ("<a>", "unexpected end of input"),
        ("<a></b>", "invalid XML"),
        ("<a/><b/>", "more than one root element"),
        ("<a/>x", "text outside of the root element"),
        (r#"<a x="1" x="2"/>"#, "invalid attribute"),
    ] {
        let err = from_str::<Value>(input).unwrap_err();
        assert!(err.message().starts_with(message), "{input}: {err}");
    }

    const SHALLOW: DeserializerConfig = DeserializerConfig::new().max_depth(2);
    assert!(SHALLOW.from_str::<Value>("<a><b><c/></b></a>").is_err());
}

#[test]
fn test_values() {
    let value: Value = from_str(r#"<a x="1"><b>2</b><b>3</b></a>"#).unwrap();
    let map = value.as_map().unwrap();
    assert!(map.is_multimap());
    assert_eq!(map.order(), deser::Order::Significant);
    assert!(value["b"].as_seq().unwrap().is_repeated());

    // converting the value gives the same result as the document
    #[derive(Debug, Deserialize, PartialEq)]
    struct A {
        #[deser(rename = "@x")]
        x: u32,
        b: Vec<u32>,
        c: Vec<u32>,
    }
    assert_eq!(
        deser_value::from_value::<A>(&value).unwrap(),
        A {
            x: 1,
            b: vec![2, 3],
            c: vec![]
        }
    );
}

#[test]
fn test_elements_and_text() {
    // an element with attributes for a type that expects text is its text
    #[derive(Debug, Deserialize, PartialEq)]
    struct Measure {
        count: u32,
        length: Vec<f64>,
        label: Option<String>,
        note: Option<u32>,
    }
    assert_eq!(
        from_str::<Measure>(
            r#"<m>
                 <count unit="x">3</count>
                 <length unit="m">1.5</length><length>2</length>
                 <label lang="en"/>
                 <note xml:lang="en"/>
               </m>"#
        )
        .unwrap(),
        Measure {
            count: 3,
            length: vec![1.5, 2.0],
            label: Some("".into()),
            note: None,
        }
    );
    // elements with child elements are not text
    let err = from_str::<Measure>("<m><count><a/><b/></count></m>").unwrap_err();
    assert!(err.message().starts_with("unexpected map"), "{err}");
    let err = from_str::<Measure>("<m><count>1<b/>2</count></m>").unwrap_err();
    assert!(err.message().contains("more than one content"), "{err}");

    // an element that is only text for a struct is its text
    #[derive(Debug, Deserialize, PartialEq)]
    struct Price {
        #[deser(rename = "@currency")]
        currency: Option<String>,
        #[deser(rename = "$text")]
        amount: f64,
    }
    #[derive(Debug, Deserialize, PartialEq)]
    struct Product {
        price: Vec<Price>,
    }
    assert_eq!(
        from_str::<Product>(r#"<p><price currency="EUR">3</price><price>4.5</price></p>"#).unwrap(),
        Product {
            price: vec![
                Price {
                    currency: Some("EUR".into()),
                    amount: 3.0
                },
                Price {
                    currency: None,
                    amount: 4.5
                }
            ]
        }
    );

    // an empty element is a struct without fields
    #[derive(Debug, Deserialize, PartialEq)]
    struct Options {
        #[deser(default)]
        verbose: bool,
        #[deser(rename = "@level")]
        level: Option<u32>,
    }
    #[derive(Debug, Deserialize, PartialEq)]
    struct Config {
        options: Options,
    }
    assert_eq!(
        from_str::<Config>("<config><options/></config>").unwrap(),
        Config {
            options: Options {
                verbose: false,
                level: None
            }
        }
    );

    // the same through buffering (the tag of an internally tagged enum
    // comes last)
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "@kind")]
    enum Item {
        Book { price: Price, pages: u32 },
    }
    assert_eq!(
        from_str::<Item>(r#"<item kind="Book"><price>3</price><pages n="x">10</pages></item>"#)
            .unwrap(),
        Item::Book {
            price: Price {
                currency: None,
                amount: 3.0
            },
            pages: 10
        }
    );
}
