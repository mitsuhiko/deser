//! Reading and writing XML documents from and to streams.
use std::collections::BTreeMap;
use std::io::Read;

use deser::ser::SerializeRef;
use deser::{Deserialize, Serialize};
use deser_xml::{DeserializerConfig, Indent, SerializerConfig};

/// Counts the writes.
struct Pieces(Vec<u8>, usize);

impl std::io::Write for Pieces {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(buf);
        self.1 += 1;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn streamed<T: Serialize + ?Sized>(
    config: &SerializerConfig,
    value: &T,
    limit: usize,
) -> (String, usize) {
    let mut writer = config.writer(Pieces(Vec::new(), 0));
    writer.set_buffer_limit(limit);
    writer.write(value).unwrap();
    let Pieces(out, writes) = writer.into_inner();
    (String::from_utf8(out).unwrap(), writes)
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename = "feed")]
struct Feed {
    #[deser(rename = "@version")]
    version: u32,
    title: String,
    entry: Vec<Entry>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    #[deser(rename = "@id")]
    id: u32,
    title: String,
    #[deser(rename = "$text")]
    text: Option<String>,
    tag: Vec<String>,
}

fn feed(entries: u32) -> Feed {
    Feed {
        version: 2,
        title: "A & B".into(),
        entry: (0..entries)
            .map(|id| Entry {
                id,
                title: format!("entry <{id}>"),
                text: (id % 2 == 0).then(|| format!("text {id}")),
                tag: (0..id % 3).map(|x| format!("t{x}")).collect(),
            })
            .collect(),
    }
}

#[test]
fn test_writer_same_output() {
    // miri is slow, it checks a smaller document, fewer configurations and
    // limits (small limits pause at every value)
    let miri = cfg!(miri);
    let feed = feed(if miri { 8 } else { 50 });
    let map = BTreeMap::from([
        ("@a", "1".to_string()),
        ("b", "2".to_string()),
        ("$text", "x".to_string()),
    ]);
    let values: [SerializeRef<'_>; 3] = [
        SerializeRef::new(&feed),
        SerializeRef::new(&map),
        SerializeRef::new(&Some(42)),
    ];
    let configs = [
        SerializerConfig::builder().root("root").build(),
        SerializerConfig::builder()
            .root("root")
            .indent(Indent::Spaces(2))
            .build(),
        SerializerConfig::builder()
            .root("root")
            .declaration(true)
            .build(),
        SerializerConfig::builder()
            .root("root")
            .indent(Indent::Tab)
            .build(),
    ];
    let limits: &[usize] = if miri {
        &[1, 100, usize::MAX]
    } else {
        &[1, 10, 100, usize::MAX]
    };
    for config in &configs[..if miri { 2 } else { 4 }] {
        for value in values {
            let expected = config.to_string(&value).unwrap();
            for &limit in limits {
                assert_eq!(streamed(config, &value, limit).0, expected, "limit {limit}");
            }
        }
    }
}

#[test]
fn test_writer_pieces() {
    // miri is slow, it writes fewer pieces
    let miri = cfg!(miri);
    let feed = feed(if miri { 60 } else { 500 });
    let config = SerializerConfig::new();
    let (out, writes) = streamed(&config, &feed, 256);
    assert_eq!(out, config.to_string(&feed).unwrap());
    assert!(writes > 10, "{writes}");

    // the output can be read again
    let read: Feed = deser_xml::from_reader(out.as_bytes()).unwrap();
    assert_eq!(read, feed);

    // with to_writer
    let mut out = Vec::new();
    deser_xml::to_writer(&mut out, &feed).unwrap();
    assert_eq!(out, config.to_string(&feed).unwrap().as_bytes());

    // maps are held back until they are complete as attributes can come
    // until their end
    let map: BTreeMap<String, String> = (0..if miri { 60 } else { 500 })
        .map(|x| (format!("k{x}"), x.to_string()))
        .collect();
    let config = SerializerConfig::builder().root("map").build();
    let (out, writes) = streamed(&config, &map, 64);
    assert_eq!(out, config.to_string(&map).unwrap());
    assert!(writes <= 2, "{writes}");
}

#[test]
fn test_writer_namespaces() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[deser(rename = "{urn:root}root")]
    struct Root {
        #[deser(rename = "{urn:a}a")]
        a: Vec<Child>,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Child {
        #[deser(rename = "@{urn:b}b")]
        b: u32,
        #[deser(rename = "{urn:a}c")]
        c: u32,
    }

    let root = Root {
        a: (0..if cfg!(miri) { 5 } else { 20 })
            .map(|x| Child { b: x, c: x * 2 })
            .collect(),
    };
    let config = SerializerConfig::new();

    // namespaces that are found after the root start tag was written are
    // declared where they are used, the document is the same
    let (out, _) = streamed(&config, &root, 1);
    assert!(
        out.starts_with("<ns0:root xmlns:ns0=\"urn:root\" xmlns:ns1=\"urn:a\">"),
        "{out}"
    );
    const READ: DeserializerConfig = DeserializerConfig::builder()
        .resolve_namespaces(true)
        .build();
    let read: Root = READ.from_slice(out.as_bytes()).unwrap();
    assert_eq!(read, root);

    // written at once all are declared on the root element
    let (out, _) = streamed(&config, &root, usize::MAX);
    assert_eq!(out, config.to_string(&root).unwrap());

    // also if the document is below the limit
    let (out, _) = streamed(&config, &root, 100_000);
    assert_eq!(out, config.to_string(&root).unwrap());

    // like the configured ones
    let config =
        SerializerConfig::new().namespaces(&[("r", "urn:root"), ("a", "urn:a"), ("b", "urn:b")]);
    let (out, _) = streamed(&config, &root, 1);
    assert_eq!(out, config.to_string(&root).unwrap());
}

#[test]
fn test_writer_single_document() {
    let mut writer = SerializerConfig::new().writer(Vec::new());
    writer.write(&feed(1)).unwrap();
    let err = writer.write(&feed(1)).unwrap_err();
    assert!(err.message().contains("single root element"), "{err}");
}

#[test]
fn test_reader() {
    let feed = feed(3);
    let xml = deser_xml::to_string(&feed).unwrap();

    let mut reader = DeserializerConfig::new().reader(xml.as_bytes());
    let read: Feed = reader.read().unwrap().unwrap();
    assert_eq!(read, feed);
    assert!(reader.read::<Feed>().unwrap().is_none());

    let read: Feed = DeserializerConfig::new()
        .from_reader(xml.as_bytes())
        .unwrap();
    assert_eq!(read, feed);

    // errors are located in the stream
    let err = deser_xml::from_reader::<Feed, _>(&b"<feed>\n  <title>x</title>\n  <entry"[..])
        .unwrap_err();
    assert_eq!(err.line(), Some(3), "{err}");

    // what follows the root element is checked at the end
    assert!(deser_xml::from_reader::<Feed, _>(xml.as_bytes().chain(&b"<x/>"[..])).is_err());
}
