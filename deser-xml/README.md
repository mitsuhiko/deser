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
* Parsing is done by [quick-xml](https://crates.io/crates/quick-xml), the
  events are passed on while the document is parsed and text is borrowed
  from the input where possible.

This is an early version.
