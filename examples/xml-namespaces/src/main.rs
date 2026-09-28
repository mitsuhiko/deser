//! XML namespaces with `deser-xml`.
//!
//! An Atom feed that mixes four vocabularies: Atom itself, Dublin Core
//! for authors, Media RSS for thumbnails and XHTML for the content of
//! entries.  Two documents that say the same thing with different
//! prefixes are read into the same value because the names are resolved
//! into `{uri}local` names.  Elements the types do not know (the
//! thumbnail) are kept as dynamic values under their resolved names.
//! Then the feed is written back, once with chosen prefixes and once with
//! generated ones.
use std::collections::BTreeMap;

use deser::{Deserialize, Serialize};
use deser_value::Value;
use deser_xml::{DeserializerConfig, Mixed, SerializerConfig};

// `atom!("title")` is `"{http://www.w3.org/2005/Atom}title"` and
// `atom!(@ "rel")` the attribute in that namespace
deser_xml::namespace!(
    atom = "http://www.w3.org/2005/Atom",
    dc = "http://purl.org/dc/elements/1.1/",
    xhtml = "http://www.w3.org/1999/xhtml",
);

const MEDIA: &str = "http://search.yahoo.com/mrss/";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename = atom!("feed"))]
struct Feed {
    /// the `xml` prefix is the same in every document
    #[deser(rename = "@xml:lang")]
    lang: Option<String>,
    #[deser(rename = atom!("title"))]
    title: String,
    #[deser(rename = atom!("link"))]
    links: Vec<Link>,
    #[deser(rename = atom!("entry"))]
    entries: Vec<Entry>,
}

/// Attributes without prefix are in no namespace, also if the element is
/// in the default namespace, so these are plain `@href` and `@rel`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Link {
    #[deser(rename = "@href")]
    href: String,
    #[deser(rename = "@rel")]
    rel: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    #[deser(rename = atom!("title"))]
    title: String,
    #[deser(rename = dc!("creator"))]
    creators: Vec<String>,
    #[deser(rename = atom!("updated"))]
    updated: String,
    #[deser(rename = atom!("content"))]
    content: Content,
    /// the elements of other vocabularies, under their `{uri}local` names
    #[deser(flatten)]
    extensions: BTreeMap<String, Value>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Content {
    #[deser(rename = "@type")]
    kind: String,
    #[deser(rename = xhtml!("div"))]
    div: Mixed<Inline>,
}

/// XHTML is mixed content: text and elements in order.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Inline {
    #[deser(rename = "$text")]
    Text(String),
    #[deser(rename = xhtml!("b"))]
    Bold(String),
    #[deser(rename = xhtml!("a"))]
    Anchor(Anchor),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Anchor {
    #[deser(rename = "@href")]
    href: String,
    #[deser(rename = "$text")]
    text: String,
}

/// Atom is the default namespace, the others have their usual prefixes.
const USUAL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"
      xmlns:dc="http://purl.org/dc/elements/1.1/"
      xmlns:media="http://search.yahoo.com/mrss/"
      xml:lang="en">
  <title>Example Blog</title>
  <link href="https://example.com/"/>
  <link rel="self" href="https://example.com/feed.xml"/>
  <entry>
    <title>Namespaces</title>
    <dc:creator>Jane</dc:creator>
    <dc:creator>John</dc:creator>
    <updated>2024-05-01T12:00:00Z</updated>
    <media:thumbnail url="https://example.com/n.png" width="64"/>
    <content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml">Names are <b>hard</b>, see <a href="https://www.w3.org/TR/xml-names/">the spec</a>.</div></content>
  </entry>
</feed>
"#;

/// The same feed from a generator that picked other prefixes: Atom has
/// the prefix `a`, Dublin Core is the default namespace of the creators
/// and XHTML has the prefix `h`.
const UNUSUAL: &str = r#"<a:feed xmlns:a="http://www.w3.org/2005/Atom"
        xmlns:m="http://search.yahoo.com/mrss/" xml:lang="en">
  <a:title>Example Blog</a:title>
  <a:link href="https://example.com/"/>
  <a:link rel="self" href="https://example.com/feed.xml"/>
  <a:entry xmlns:h="http://www.w3.org/1999/xhtml">
    <a:title>Namespaces</a:title>
    <creator xmlns="http://purl.org/dc/elements/1.1/">Jane</creator>
    <creator xmlns="http://purl.org/dc/elements/1.1/">John</creator>
    <a:updated>2024-05-01T12:00:00Z</a:updated>
    <m:thumbnail width="64" url="https://example.com/n.png"/>
    <a:content type="xhtml"><h:div>Names are <h:b>hard</h:b>, see <h:a href="https://www.w3.org/TR/xml-names/">the spec</h:a>.</h:div></a:content>
  </a:entry>
</a:feed>
"#;

fn main() {
    const RESOLVE: DeserializerConfig = DeserializerConfig::new().resolve_namespaces(true);

    let feed: Feed = RESOLVE.from_str(USUAL).unwrap();
    println!("{:#?}", feed);
    let entry = &feed.entries[0];
    assert_eq!(feed.lang.as_deref(), Some("en"));
    assert_eq!(feed.links[1].rel.as_deref(), Some("self"));
    assert_eq!(entry.creators, ["Jane", "John"]);
    assert_eq!(
        entry.content.div.0[1],
        Inline::Bold("hard".into()),
        "the XHTML is read in order"
    );

    // the thumbnail is not a field, it's kept under its resolved name
    let thumbnail = format!("{{{MEDIA}}}thumbnail");
    println!("\nextensions: {:?}", entry.extensions.keys());
    assert_eq!(entry.extensions.keys().collect::<Vec<_>>(), [&thumbnail]);

    // the prefixes of the document do not matter
    let other: Feed = RESOLVE.from_str(UNUSUAL).unwrap();
    assert_eq!(other, feed);

    // without resolving, names are compared as written: the Atom title is
    // `a:title` in the second document
    let err = deser_xml::from_str::<Feed>(UNUSUAL).unwrap_err();
    println!("\nas written: {}", err);

    // writing with chosen prefixes, which are declared on the root
    const PREFIXES: SerializerConfig = SerializerConfig::new().namespaces(&[
        ("", "http://www.w3.org/2005/Atom"),
        ("dc", "http://purl.org/dc/elements/1.1/"),
        ("media", MEDIA),
        ("h", "http://www.w3.org/1999/xhtml"),
    ]);
    let xml = PREFIXES.to_string(&feed).unwrap();
    println!("\nwith prefixes:\n{}", xml);
    assert!(xml.starts_with(r#"<feed xmlns="http://www.w3.org/2005/Atom""#));
    assert!(xml.contains("<dc:creator>Jane</dc:creator>"));
    assert!(xml.contains("<media:thumbnail "));
    assert_eq!(RESOLVE.from_str::<Feed>(&xml).unwrap(), feed);

    // without them, prefixes are generated and declared where they are
    // first needed
    let xml = deser_xml::to_string(&feed).unwrap();
    println!("\ngenerated prefixes:\n{}", xml);
    assert!(xml.starts_with(r#"<ns0:feed xmlns:ns0="http://www.w3.org/2005/Atom""#));
    assert!(xml.contains(r#"<ns1:creator xmlns:ns1="http://purl.org/dc/elements/1.1/">"#));
    assert_eq!(RESOLVE.from_str::<Feed>(&xml).unwrap(), feed);
}
