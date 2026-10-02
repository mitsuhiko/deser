use std::collections::BTreeMap;

use deser::Serialize;
use deser::de::Recording;
use deser_value::Value;
use deser_xml::{DeserializerConfig, Root, SerializerConfig, from_str, to_string};

const RESOLVE: DeserializerConfig = DeserializerConfig::builder()
    .resolve_namespaces(true)
    .build();

/// Reads a document into a recording and writes it again.
fn round_trip(config: &DeserializerConfig, input: &str) -> String {
    let recording: Recording = config.from_str(input).unwrap();
    to_string(&recording).unwrap()
}

#[test]
fn test_root_of_values() {
    // values that keep event data keep the root element
    let input = r#"<feed id="1"><title>x</title><entry>a</entry><entry>b</entry></feed>"#;
    let recording: Recording = from_str(input).unwrap();
    assert_eq!(to_string(&recording).unwrap(), input);
    let value: Value = from_str(input).unwrap();
    assert_eq!(to_string(&value).unwrap(), input);

    // also if the root is text or empty
    for input in ["<name>x</name>", "<empty/>", r#"<name lang="en">x</name>"#] {
        assert_eq!(round_trip(&DeserializerConfig::new(), input), input);
    }

    // the configured name is for values without one
    assert_eq!(
        SerializerConfig::builder()
            .root("r")
            .build()
            .to_string(&recording)
            .unwrap(),
        input
    );

    // other formats ignore it
    let recording: Recording = from_str("<r><a>1</a></r>").unwrap();
    assert_eq!(deser_json::to_string(&recording).unwrap(), r#"{"a":"1"}"#);
}

#[test]
fn test_namespaces_of_values() {
    // declarations on the root are kept, with the names as written
    for input in [
        r#"<feed xmlns="urn:atom"><title>x</title></feed>"#,
        r#"<a:feed xmlns:a="urn:a" a:x="1"><a:t>x</a:t></a:feed>"#,
        r#"<doc xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:schemaLocation="urn:x x.xsd"><a>1</a></doc>"#,
        r#"<a:feed xmlns="urn:atom" xmlns:a="urn:a"><title>x</title><a:t>y</a:t></a:feed>"#,
    ] {
        assert_eq!(round_trip(&DeserializerConfig::new(), input), input);
        // and resolved names are written with the prefixes of the document
        assert_eq!(round_trip(&RESOLVE, input), input);
    }

    // names get the configured prefixes, and so do the declarations
    const PREFIXED: DeserializerConfig = DeserializerConfig::new().namespaces(&[("x", "urn:a")]);
    assert_eq!(
        round_trip(
            &PREFIXED,
            r#"<a:feed xmlns:a="urn:a"><a:t>x</a:t></a:feed>"#
        ),
        r#"<x:feed xmlns:x="urn:a"><x:t>x</x:t></x:feed>"#
    );
}

#[test]
fn test_nested_namespaces() {
    // declarations on other elements are kept where they are, for maps,
    // text, repeated and empty elements
    for input in [
        r#"<r><a:t xmlns:a="urn:a">x</a:t></r>"#,
        r#"<r><a:t xmlns:a="urn:a" a:x="1"><a:u>y</a:u></a:t></r>"#,
        r#"<r><t xmlns="urn:a">1</t><t xmlns="urn:b">2</t></r>"#,
        r#"<r><a:t xmlns:a="urn:a"/><a:t xmlns:a="urn:a"/></r>"#,
        r#"<r xmlns:a="urn:a"><a:t><b xmlns:a="urn:b"><a:u>x</a:u></b></a:t></r>"#,
    ] {
        assert_eq!(round_trip(&DeserializerConfig::new(), input), input);
        assert_eq!(round_trip(&RESOLVE, input), input);
        let value: Value = from_str(input).unwrap();
        assert_eq!(to_string(&value).unwrap(), input);
    }

    // undeclaring the default namespace keeps names out of it
    let input = r#"<feed xmlns="urn:atom"><title>x</title><x xmlns="">1</x></feed>"#;
    assert_eq!(round_trip(&DeserializerConfig::new(), input), input);
    assert_eq!(round_trip(&RESOLVE, input), input);

    // a prefix that is bound again is not used for the outer namespace
    let input = r#"<a:r xmlns:a="urn:a"><b xmlns:a="urn:b"><x>1</x></b></a:r>"#;
    let recording: Recording = RESOLVE.from_str(input).unwrap();
    let mut value: Value = RESOLVE.from_str(input).unwrap();
    let b = value.as_map_mut().unwrap().get_mut("b").unwrap();
    b.as_map_mut().unwrap().insert("{urn:a}y", "2");
    assert_eq!(to_string(&recording).unwrap(), input);
    assert_eq!(
        to_string(&value).unwrap(),
        r#"<a:r xmlns:a="urn:a" xmlns:ns0="urn:a"><b xmlns:a="urn:b"><x>1</x><ns0:y>2</ns0:y></b></a:r>"#
    );
}

#[test]
fn test_deserialize_root() {
    let doc: Root<BTreeMap<String, String>> = from_str(
        r#"<feed xmlns="urn:atom" xmlns:dc="urn:dc"><dc:creator>Jane</dc:creator></feed>"#,
    )
    .unwrap();
    assert_eq!(doc.name.as_deref(), Some("feed"));
    assert_eq!(
        doc.namespaces,
        [
            (String::new(), "urn:atom".to_string()),
            ("dc".to_string(), "urn:dc".to_string())
        ]
    );
    assert_eq!(doc.value["dc:creator"], "Jane");

    // resolved names
    let doc: Root<String> = RESOLVE.from_str(r#"<a:n xmlns:a="urn:a">x</a:n>"#).unwrap();
    assert_eq!(doc.name.as_deref(), Some("{urn:a}n"));
    assert_eq!(doc.value, "x");

    // other formats have no root element
    let doc: Root<u32> = deser_json::from_str("42").unwrap();
    assert_eq!(
        doc,
        Root {
            name: None,
            namespaces: vec![],
            value: 42
        }
    );
}

#[test]
fn test_serialize_root() {
    #[derive(Serialize)]
    #[deser(rename = "point")]
    struct Point {
        #[deser(rename = "@x")]
        x: i32,
    }

    // the root names values, also over the name of their type and the
    // configured name
    assert_eq!(to_string(&Root::new("n", 42)).unwrap(), "<n>42</n>");
    assert_eq!(
        to_string(&Root::new("p", Point { x: 1 })).unwrap(),
        r#"<p x="1"/>"#
    );
    let config = SerializerConfig::builder().root("r").build();
    assert_eq!(
        config.to_string(&Point { x: 1 }).unwrap(),
        r#"<point x="1"/>"#
    );
    assert_eq!(config.to_string(&Root::new("n", 1)).unwrap(), "<n>1</n>");
    // without a name the root only declares namespaces
    let root = Root {
        name: None,
        namespaces: vec![("a".into(), "urn:a".into())],
        value: Point { x: 1 },
    };
    assert_eq!(
        to_string(&root).unwrap(),
        r#"<point xmlns:a="urn:a" x="1"/>"#
    );

    // namespaces of the root come before the configured ones, which are
    // left out if their prefix is taken
    let config = SerializerConfig::new().namespaces(&[("a", "urn:other"), ("b", "urn:b")]);
    let root =
        Root::new("{urn:a}doc", BTreeMap::from([("{urn:other}x", 1)])).with_namespace("a", "urn:a");
    assert_eq!(
        config.to_string(&root).unwrap(),
        r#"<a:doc xmlns:a="urn:a" xmlns:b="urn:b" xmlns:ns0="urn:other"><ns0:x>1</ns0:x></a:doc>"#
    );

    // prefixes have to be prefixes
    for prefix in ["a:b", "1"] {
        let root = Root::new("r", 1).with_namespace(prefix, "urn:a");
        assert!(to_string(&root).is_err(), "{prefix}");
    }

    // only documents have a root
    let value = BTreeMap::from([("a", Root::new("b", 1))]);
    assert_eq!(
        SerializerConfig::builder()
            .root("r")
            .build()
            .to_string(&value)
            .unwrap(),
        "<r><a>1</a></r>"
    );
}

#[test]
fn test_root_of_captured_values() {
    // the root is taken from the value, so it's not kept twice
    let input = r#"<a:feed xmlns:a="urn:a"><a:t>x</a:t></a:feed>"#;
    let doc: Root<Recording> = from_str(input).unwrap();
    assert_eq!(doc.name.as_deref(), Some("a:feed"));
    assert_eq!(to_string(&doc).unwrap(), input);
    assert_eq!(
        to_string(&doc.value).unwrap_err().message(),
        "the name of the root element is unknown (see deser_xml::Root)"
    );

    // and it can be changed
    let mut doc: Root<Value> = from_str(input).unwrap();
    doc.name = Some("a:entry".into());
    assert_eq!(
        to_string(&doc).unwrap(),
        r#"<a:entry xmlns:a="urn:a"><a:t>x</a:t></a:entry>"#
    );

    // a new root for a recorded value
    let recording: Recording = deser_json::from_str(r#"{"@a": 1, "b": [2, 3]}"#).unwrap();
    assert_eq!(
        to_string(&Root::new("r", recording)).unwrap(),
        r#"<r a="1"><b>2</b><b>3</b></r>"#
    );
}
