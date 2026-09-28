# deser-xml

XML support for [deser](https://github.com/mitsuhiko/deser).

```rust
use deser::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct Link {
    #[deser(rename = "@href")]
    href: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[deser(rename = "feed")]
struct Feed {
    title: String,
    link: Vec<Link>,
}

let feed: Feed = deser_xml::from_str(r#"
    <feed>
      <title>Example</title>
      <link href="/a"/>
      <link href="/b"/>
    </feed>
"#).unwrap();
assert_eq!(feed.link.len(), 2);
let xml = deser_xml::to_string(&feed).unwrap();
```

* Attributes are entries with an `@` prefix, text is the `$text` entry.
* Elements are multimaps: `Vec<T>` fields collect all child elements with
  their name, also if other elements are between them.
* Elements with attributes are their text for types that expect text,
  elements that are only text are maps for structs.
* Names are kept as written, namespaces can be given fixed prefixes or
  be resolved into `{uri}local` names (`qname!`, `namespace!`) which the
  serializer writes with configured (`prefixes!`) or generated prefixes.
* `Root<T>` captures the name of the root element and the namespaces
  declared on it, and writes them.  Values that keep event data (like
  `Recording` and `deser_value::Value`) keep them too, so documents that
  are read into them or transcoded keep their root element.
* `deser_xml::Serializer` implements deser's `Serializer` trait for code
  that does not know the format upfront (like `deser-transcode`).  Values
  without a name (like maps) need a `Root` or `SerializerConfig::root`.
* `SerializerConfig::pretty` indents child elements where the whitespace
  is not text, mixed content stays on a single line.
* Parsing is done by [quick-xml](https://crates.io/crates/quick-xml), the
  events are passed on while the document is parsed and text is borrowed
  from the input where possible.
* With the `io` feature (enabled by default) documents are read with
  `from_reader` and written with `to_writer` (or `deser::io`).  The output
  is written while the value is serialized, elements are held back only
  until no more attributes can come for them.

This is an early version.
