use std::collections::BTreeMap;

use deser::hints::{Compact, Expanded};
use deser::{Deserialize, Serialize};
use deser_xml::{Indent, Mixed, SerializerConfig, SkipWhitespace, from_str};

const PRETTY: SerializerConfig = SerializerConfig::new().pretty(Indent::Spaces(2));

/// Serializes pretty and checks that the value reads back the same.
fn pretty<T>(value: &T) -> String
where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let xml = PRETTY.to_string(value).unwrap();
    assert_eq!(&from_str::<T>(&xml).unwrap(), value, "{xml}");
    xml
}

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
struct Note {
    #[deser(rename = "@lang")]
    lang: String,
    #[deser(rename = "$text")]
    text: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Default)]
struct Nothing {}

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
    nothing: Nothing,
}

#[test]
fn test_indent() {
    let person = Person {
        id: 7,
        name: " Jane <J> ".into(),
        age: None,
        tag: vec!["a".into(), "".into()],
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
            text: " hi\n".into(),
        },
        nothing: Nothing {},
    };
    // text is never touched, empty elements stay empty
    assert_eq!(
        pretty(&person),
        r#"<person id="7">
  <name> Jane &lt;J&gt; </name>
  <tag>a</tag>
  <tag/>
  <address country="AT">
    <city>Vienna</city>
  </address>
  <shape>
    <circle r="1.5"/>
  </shape>
  <shape>
    <square>
      <side>2.0</side>
    </square>
  </shape>
  <shape>empty</shape>
  <note lang="en"> hi
</note>
  <nothing/>
</person>"#
    );

    let map = BTreeMap::from([("a", BTreeMap::from([("b", 1)]))]);
    const CONFIG: SerializerConfig = SerializerConfig::new().root("m");
    assert_eq!(
        CONFIG.indent(Indent::Tab).to_string(&map).unwrap(),
        "<m>\n\t<a>\n\t\t<b>1</b>\n\t</a>\n</m>"
    );
    assert_eq!(
        CONFIG.indent(Indent::Spaces(0)).to_string(&map).unwrap(),
        "<m>\n<a>\n<b>1</b>\n</a>\n</m>"
    );
    assert_eq!(
        CONFIG
            .indent(Indent::Spaces(4))
            .indent(Indent::None)
            .to_string(&map)
            .unwrap(),
        "<m><a><b>1</b></a></m>"
    );
    // single values at the root
    assert_eq!(PRETTY.root("n").to_string(&42).unwrap(), "<n>42</n>");
}

#[test]
fn test_declaration_and_namespaces() {
    deser_xml::namespace!(
        atom = "http://www.w3.org/2005/Atom",
        dc = "http://purl.org/dc/elements/1.1/",
    );

    #[derive(Serialize)]
    #[deser(rename = atom!("feed"))]
    struct Feed {
        #[deser(rename = atom!("title"))]
        title: String,
        #[deser(rename = dc!("creator"))]
        creator: Vec<String>,
    }

    let feed = Feed {
        title: "x".into(),
        creator: vec!["y".into(), "z".into()],
    };
    assert_eq!(
        PRETTY
            .declaration(true)
            .namespaces(deser_xml::prefixes![atom as ""])
            .to_string(&feed)
            .unwrap(),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:ns0="http://purl.org/dc/elements/1.1/">
  <title>x</title>
  <ns0:creator>y</ns0:creator>
  <ns0:creator>z</ns0:creator>
</feed>"#
    );
}

#[test]
fn test_text_fields() {
    // text before the child elements, left out: indented
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[deser(rename = "e")]
    struct TextFirst {
        #[deser(rename = "$text", skip_serializing_if = Option::is_none)]
        text: Option<String>,
        b: Vec<u32>,
    }
    assert_eq!(
        pretty(&TextFirst {
            text: None,
            b: vec![1, 2]
        }),
        "<e>\n  <b>1</b>\n  <b>2</b>\n</e>"
    );
    // written: on a single line
    assert_eq!(
        pretty(&TextFirst {
            text: Some("x".into()),
            b: vec![1, 2]
        }),
        "<e>x<b>1</b><b>2</b></e>"
    );

    // text after the child elements: on a single line
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[deser(rename = "e")]
    struct TextLast {
        b: Vec<Address>,
        #[deser(rename = "$text", skip_serializing_if = Option::is_none)]
        text: Option<String>,
    }
    let address = || Address {
        country: "AT".into(),
        city: "Vienna".into(),
    };
    assert_eq!(
        pretty(&TextLast {
            b: vec![address()],
            text: Some("x".into())
        }),
        r#"<e><b country="AT"><city>Vienna</city></b>x</e>"#
    );
    assert_eq!(
        pretty(&TextLast {
            b: vec![address()],
            text: None
        }),
        r#"<e><b country="AT"><city>Vienna</city></b></e>"#
    );

    // unless it's expanded: indented until the text comes
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[deser(rename = "doc")]
    struct Doc {
        #[deser(as = Expanded)]
        e: TextLast,
    }
    assert_eq!(
        pretty(&Doc {
            e: TextLast {
                b: vec![address(), address()],
                text: None,
            }
        }),
        r#"<doc>
  <e>
    <b country="AT">
      <city>Vienna</city>
    </b>
    <b country="AT">
      <city>Vienna</city>
    </b>
  </e>
</doc>"#
    );
    assert_eq!(
        pretty(&Doc {
            e: TextLast {
                b: vec![address(), address()],
                text: Some("x".into()),
            }
        }),
        r#"<doc>
  <e>
    <b country="AT">
      <city>Vienna</city>
    </b>
    <b country="AT">
      <city>Vienna</city>
    </b>x</e>
</doc>"#
    );

    // empty text is no text (it's not there when read back, also in
    // compact output)
    let empty = TextFirst {
        text: Some("".into()),
        b: vec![1],
    };
    assert_eq!(PRETTY.to_string(&empty).unwrap(), "<e>\n  <b>1</b>\n</e>");
}

#[test]
fn test_maps() {
    // the keys are not known, text is found when it comes
    let config = PRETTY.root("m");
    let map = BTreeMap::from([("$text", "x"), ("@a", "1"), ("b", "2")]);
    assert_eq!(config.to_string(&map).unwrap(), r#"<m a="1">x<b>2</b></m>"#);
    let map = BTreeMap::from([("@a", "1"), ("b", "2"), ("c", "3")]);
    assert_eq!(
        config.to_string(&map).unwrap(),
        "<m a=\"1\">\n  <b>2</b>\n  <c>3</c>\n</m>"
    );
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
enum Inline {
    #[deser(rename = "$text")]
    Text(String),
    #[deser(rename = "b")]
    Bold(String),
    #[deser(rename = "em")]
    Emphasis(Mixed<Inline>),
}

#[test]
fn test_mixed() {
    // whitespace would be text of mixed content
    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    #[deser(rename = "doc")]
    struct Doc {
        title: String,
        p: Vec<Mixed<Inline>>,
    }
    let doc = Doc {
        title: "T".into(),
        p: vec![
            Mixed::from(vec![
                Inline::Bold("x".into()),
                Inline::Emphasis(Mixed::from(vec![Inline::Bold("y".into())])),
            ]),
            Mixed::from(vec![Inline::Text("a ".into()), Inline::Bold("b".into())]),
        ],
    };
    assert_eq!(
        pretty(&doc),
        "<doc>\n  <title>T</title>\n  <p><b>x</b><em><b>y</b></em></p>\n  <p>a <b>b</b></p>\n</doc>"
    );

    // also if it's flattened, from the first value on
    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    #[deser(rename = "section")]
    struct Section {
        #[deser(rename = "@id")]
        id: String,
        title: String,
        #[deser(flatten)]
        content: Mixed<Inline>,
    }
    let section = Section {
        id: "s1".into(),
        title: "T".into(),
        content: Mixed::from(vec![Inline::Bold("a".into()), Inline::Bold("b".into())]),
    };
    assert_eq!(
        pretty(&section),
        "<section id=\"s1\">\n  <title>T</title><b>a</b><b>b</b></section>"
    );
    let section = Section {
        id: "s1".into(),
        title: "T".into(),
        content: Mixed::new(),
    };
    assert_eq!(
        pretty(&section),
        "<section id=\"s1\">\n  <title>T</title>\n</section>"
    );

    // content that skips whitespace is indented until text comes
    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    enum Block {
        #[deser(rename = "$text")]
        Text(String),
        #[deser(rename = "p")]
        Paragraph(Mixed<Inline>),
    }
    let doc: Mixed<Block, SkipWhitespace> = Mixed::from(vec![
        Block::Paragraph(Mixed::from(vec![Inline::Bold("a".into())])),
        Block::Paragraph(Mixed::from(vec![Inline::Bold("b".into())])),
        Block::Text("text".into()),
        Block::Paragraph(Mixed::from(vec![Inline::Bold("c".into())])),
    ]);
    let xml = PRETTY.root("doc").to_string(&doc).unwrap();
    assert_eq!(
        xml,
        "<doc>\n  <p><b>a</b></p>\n  <p><b>b</b></p>text<p><b>c</b></p></doc>"
    );
    assert_eq!(from_str::<Mixed<Block, SkipWhitespace>>(&xml).unwrap(), doc);
}

#[test]
fn test_compact() {
    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    #[deser(rename = "doc")]
    struct Doc {
        #[deser(as = Compact)]
        address: Address,
        #[deser(as = Compact)]
        point: Vec<Point>,
        other: Vec<Point>,
    }

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct Point {
        x: i32,
        y: i32,
    }

    let doc = Doc {
        address: Address {
            country: "AT".into(),
            city: "Vienna".into(),
        },
        point: vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }],
        other: vec![Point { x: 5, y: 6 }],
    };
    assert_eq!(
        pretty(&doc),
        r#"<doc>
  <address country="AT"><city>Vienna</city></address>
  <point><x>1</x><y>2</y></point><point><x>3</x><y>4</y></point>
  <other>
    <x>5</x>
    <y>6</y>
  </other>
</doc>"#
    );
}
