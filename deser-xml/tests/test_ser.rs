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
         <shape><circle r=\"1.5\"/></shape><shape><square><side>2.0</side></square></shape>\
         <shape>empty</shape><note lang=\"en\">hi \"there\"</note></person>"
    );
    assert_eq!(from_str::<Person>(&out).unwrap(), person());
}

#[test]
fn test_root() {
    // atoms at the root with a name
    assert_eq!(
        SerializerConfig::builder()
            .root("n")
            .build()
            .to_string(&42)
            .unwrap(),
        "<n>42</n>"
    );
    assert_eq!(
        SerializerConfig::builder()
            .root("n")
            .build()
            .to_string(&None::<u32>)
            .unwrap(),
        "<n/>"
    );
    // maps need a name
    let map = BTreeMap::from([("a", 1)]);
    assert!(to_string(&map).is_err());
    assert_eq!(
        SerializerConfig::builder()
            .root("m")
            .build()
            .to_string(&map)
            .unwrap(),
        "<m><a>1</a></m>"
    );
    // empty structs
    #[derive(Serialize)]
    struct Empty {}
    assert_eq!(to_string(&Empty {}).unwrap(), "<Empty/>");
    // sequences have no root
    assert!(
        SerializerConfig::builder()
            .root("s")
            .build()
            .to_string(&[1, 2])
            .is_err()
    );

    assert_eq!(
        SerializerConfig::builder()
            .root("n")
            .declaration(true)
            .build()
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
fn test_resolved_names() {
    deser_xml::namespace!(
        atom = "http://www.w3.org/2005/Atom",
        dc = "http://purl.org/dc/elements/1.1/",
        xlink = "http://www.w3.org/1999/xlink",
    );

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[deser(rename = atom!("feed"))]
    struct Feed {
        #[deser(rename = atom!("title"))]
        title: String,
        #[deser(rename = atom!("link"))]
        link: Vec<Link>,
        #[deser(rename = dc!("creator"))]
        creator: Vec<String>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Link {
        #[deser(rename = xlink!(@ "href"))]
        href: String,
        #[deser(rename = atom!(@ "rel"))]
        rel: String,
        #[deser(rename = "@type")]
        kind: Option<String>,
        #[deser(rename = xlink!("title"))]
        title: Option<String>,
    }

    let feed = Feed {
        title: "x".into(),
        link: vec![
            Link {
                href: "/a".into(),
                rel: "self".into(),
                kind: Some("text/html".into()),
                title: Some("A".into()),
            },
            Link {
                href: "/b".into(),
                rel: "next".into(),
                kind: None,
                title: None,
            },
        ],
        creator: vec!["a".into(), "b".into()],
    };

    // without configuration the prefixes are generated, every namespace
    // has one prefix that is declared on the root
    let xml = to_string(&feed).unwrap();
    assert_eq!(
        xml,
        "<ns0:feed xmlns:ns0=\"http://www.w3.org/2005/Atom\" \
         xmlns:ns1=\"http://www.w3.org/1999/xlink\" xmlns:ns2=\"http://purl.org/dc/elements/1.1/\">\
         <ns0:title>x</ns0:title>\
         <ns0:link ns1:href=\"/a\" ns0:rel=\"self\" type=\"text/html\"><ns1:title>A</ns1:title></ns0:link>\
         <ns0:link ns1:href=\"/b\" ns0:rel=\"next\"/>\
         <ns2:creator>a</ns2:creator><ns2:creator>b</ns2:creator></ns0:feed>"
    );
    const RESOLVE: deser_xml::DeserializerConfig = deser_xml::DeserializerConfig::builder()
        .resolve_namespaces(true)
        .build();
    assert_eq!(RESOLVE.from_str::<Feed>(&xml).unwrap(), feed);

    // configured prefixes are used, generated ones skip them.  The default
    // namespace is not used for attributes, they get another prefix.
    const CONFIG: SerializerConfig =
        SerializerConfig::new().namespaces(&[("", atom!()), ("dc", dc!()), ("ns0", "urn:taken")]);
    let xml = CONFIG.to_string(&feed).unwrap();
    assert_eq!(
        xml,
        "<feed xmlns=\"http://www.w3.org/2005/Atom\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
         xmlns:ns0=\"urn:taken\" xmlns:ns1=\"http://www.w3.org/1999/xlink\" \
         xmlns:ns2=\"http://www.w3.org/2005/Atom\"><title>x</title>\
         <link ns1:href=\"/a\" ns2:rel=\"self\" type=\"text/html\"><ns1:title>A</ns1:title></link>\
         <link ns1:href=\"/b\" ns2:rel=\"next\"/>\
         <dc:creator>a</dc:creator><dc:creator>b</dc:creator></feed>"
    );
    assert_eq!(RESOLVE.from_str::<Feed>(&xml).unwrap(), feed);

    // prefixes named after the macros, a prefix for the attributes in the
    // default namespace, and the XML declaration before the root
    const PREFIXED: SerializerConfig = SerializerConfig::new()
        .namespaces(deser_xml::prefixes![atom as "", atom as "a", xlink])
        .into_builder()
        .declaration(true)
        .build();
    let xml = PREFIXED.to_string(&feed).unwrap();
    assert_eq!(
        xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <feed xmlns=\"http://www.w3.org/2005/Atom\" xmlns:a=\"http://www.w3.org/2005/Atom\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:ns0=\"http://purl.org/dc/elements/1.1/\">\
         <title>x</title>\
         <link xlink:href=\"/a\" a:rel=\"self\" type=\"text/html\"><xlink:title>A</xlink:title></link>\
         <link xlink:href=\"/b\" a:rel=\"next\"/>\
         <ns0:creator>a</ns0:creator><ns0:creator>b</ns0:creator></feed>"
    );
    assert_eq!(RESOLVE.from_str::<Feed>(&xml).unwrap(), feed);

    // a root that is a single value
    #[derive(Serialize)]
    #[deser(rename = atom!("id"))]
    struct Id(&'static str);
    assert_eq!(
        to_string(&Id("x")).unwrap(),
        r#"<ns0:id xmlns:ns0="http://www.w3.org/2005/Atom">x</ns0:id>"#
    );

    // the xml prefix is never declared
    let value = BTreeMap::from([("@{http://www.w3.org/XML/1998/namespace}lang", "en")]);
    assert_eq!(
        SerializerConfig::builder()
            .root("a")
            .build()
            .to_string(&value)
            .unwrap(),
        r#"<a xml:lang="en"/>"#
    );

    // names that are not names
    for name in ["{}a", "{urn:a", "{urn:a}", "{urn:a}x:y", "@{urn:a}1"] {
        let value = BTreeMap::from([(name, "1")]);
        let err = SerializerConfig::builder()
            .root("a")
            .build()
            .to_string(&value)
            .unwrap_err();
        assert!(err.message().ends_with("is not a name in XML"), "{name}");
    }
}

#[test]
fn test_attribute_order() {
    // attributes after content go into the start tag
    #[derive(Serialize)]
    struct Late {
        a: u32,
        #[deser(rename = "@b")]
        b: u32,
        c: Inner,
        #[deser(rename = "@d")]
        d: Option<u32>,
        #[deser(rename = "@e")]
        e: &'static str,
    }
    #[derive(Serialize)]
    struct Inner {
        #[deser(rename = "$text")]
        text: &'static str,
        #[deser(rename = "@x")]
        x: &'static str,
    }
    let late = Late {
        a: 1,
        b: 2,
        c: Inner { text: "t", x: "<" },
        d: None,
        e: "\"",
    };
    assert_eq!(
        to_string(&late).unwrap(),
        r#"<Late b="2" e="&quot;"><a>1</a><c x="&lt;">t</c></Late>"#
    );

    // maps: the text key sorts before the attribute prefix
    let map = BTreeMap::from([("$text", "x"), ("@a", "1"), ("@b", "2")]);
    assert_eq!(
        SerializerConfig::builder()
            .root("m")
            .build()
            .to_string(&map)
            .unwrap(),
        r#"<m a="1" b="2">x</m>"#
    );

    // the prefixes of late attributes are declared on the root
    let map = BTreeMap::from([("$text", "x"), ("@{urn:a}a", "1")]);
    assert_eq!(
        SerializerConfig::builder()
            .root("m")
            .build()
            .to_string(&map)
            .unwrap(),
        r#"<m xmlns:ns0="urn:a" ns0:a="1">x</m>"#
    );

    // values read from documents (they keep the name of the root)
    let mut value: deser_value::Value =
        from_str(r#"<doc><a>1</a><b x="y"><c/></b></doc>"#).unwrap();
    value.as_map_mut().unwrap().insert("@late", "1");
    assert_eq!(
        to_string(&value).unwrap(),
        r#"<doc late="1"><a>1</a><b x="y"><c/></b></doc>"#
    );
}

#[test]
fn test_errors() {
    // attributes that are not values
    #[derive(Serialize)]
    struct Nested {
        #[deser(rename = "@a")]
        a: Vec<u32>,
    }
    assert!(to_string(&Nested { a: vec![1] }).is_err());

    // names that are not names
    let map = BTreeMap::from([("not a name", 1)]);
    assert!(
        SerializerConfig::builder()
            .root("m")
            .build()
            .to_string(&map)
            .is_err()
    );

    // characters that cannot be written
    assert!(
        SerializerConfig::builder()
            .root("m")
            .build()
            .to_string(&"\u{1}")
            .is_err()
    );

    // sequences in sequences
    let map = BTreeMap::from([("a", vec![vec![1]])]);
    assert!(
        SerializerConfig::builder()
            .root("m")
            .build()
            .to_string(&map)
            .is_err()
    );
}

#[test]
fn test_value_round_trip() {
    // documents read into values write back the same way (element order is
    // grouped by name, text is kept)
    let input = r#"<doc id="1"><a>1</a><a>2</a><b x="y">text</b><c/></doc>"#;
    let value: deser_value::Value = from_str(input).unwrap();
    assert_eq!(to_string(&value).unwrap(), input);
}

#[test]
fn test_serializer() {
    #[derive(Serialize)]
    #[deser(rename = "point")]
    struct Point {
        #[deser(rename = "@x")]
        x: i32,
        y: i32,
    }

    // the root element is named after the struct
    let mut serializer = deser_xml::Serializer::new();
    serializer.serialize(&Point { x: 1, y: 2 }).unwrap();
    assert_eq!(serializer.as_str(), r#"<point x="1"><y>2</y></point>"#);
    // a document has a single root element
    let err = serializer.serialize(&Point { x: 3, y: 4 }).unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidState: an XML document holds a single root element"
    );
    assert_eq!(serializer.finish(), r#"<point x="1"><y>2</y></point>"#);

    // values without a name need the configured one, nothing is written if
    // the value fails
    let mut serializer = deser_xml::Serializer::new();
    let err = serializer
        .serialize(&BTreeMap::from([("a", 1)]))
        .unwrap_err();
    assert!(err.message().starts_with("the name of the root element"));
    assert_eq!(serializer.as_str(), "");
    let config = SerializerConfig::builder()
        .root("r")
        .declaration(true)
        .build();
    let mut serializer = deser_xml::Serializer::with_config(config);
    serializer.serialize(&BTreeMap::from([("a", 1)])).unwrap();
    assert_eq!(
        serializer.finish(),
        r#"<?xml version="1.0" encoding="UTF-8"?><r><a>1</a></r>"#
    );
}

#[test]
fn test_serializer_trait() {
    // the serializer can be used where the format is not known
    fn write(ser: &mut dyn deser::ser::Serializer, value: deser::ser::SerializeRef<'_>) {
        ser.serialize_ref(value).unwrap();
    }
    let mut serializer =
        deser_xml::Serializer::with_config(SerializerConfig::builder().root("r").build());
    write(
        &mut serializer,
        deser::ser::SerializeRef::new(&vec![("a", 1)].into_iter().collect::<BTreeMap<_, _>>()),
    );
    assert_eq!(serializer.finish(), "<r><a>1</a></r>");
}
