# xml-pretty

```
cargo run -p xml-pretty
```

## Why

XML is often written for people to read and diff, so it's indented.  But
unlike in JSON, whitespace can be text in XML: indenting
`<text>Hello <tspan>world</tspan></text>` would change the text.
`deser-xml` only adds whitespace where it is not text, so the indented
document reads back into the same value.  Documents that mix
vocabularies also need their namespaces declared with prefixes, which the
serializer picks from the configuration or generates.

## What it shows

- An SVG drawing whose names are `{uri}local` names written with
  `deser_xml::namespace!` (`svg!("rect")`, `xlink!(@ "href")` for an
  attribute in a namespace).
- Writing with the prefixes of `deser_xml::prefixes![svg as "", xlink]`:
  SVG is the default namespace, XLink has its usual prefix and Dublin
  Core, which is not configured, gets a generated one (`ns0`).  All are
  declared on the root element.
- `SerializerConfig::pretty(Indent::Spaces(2))` with the XML declaration:
  child elements on lines of their own, text left as it is.
- Mixed content (`Mixed<Span>`, the label) stays on a single line as its
  whitespace is text.  The shapes are `Mixed<Shape, SkipWhitespace>`,
  whose whitespace is not content, so they are indented.
- `#[deser(as = Compact)]` keeping a sequence (the gradient stops) on a
  single line.
- `SerializerConfig::indent(Indent::Tab)` with all prefixes generated in
  the order the namespaces are first used.
- Every version read back into the same value with
  `DeserializerConfig::resolve_namespaces`.

## What you should see

The drawing three times: compact on a single line, indented by two spaces
with the declaration, and indented by tabs with generated prefixes:

```
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:ns0="http://purl.org/dc/elements/1.1/" width="200" height="100">
  <title>Sunset</title>
  <metadata>
    <ns0:creator>Jane</ns0:creator>
    <ns0:creator>John</ns0:creator>
    <ns0:date>2024-05-01</ns0:date>
  </metadata>
  <defs>
    <linearGradient id="sky">
      <stop offset="0" stop-color="orange"/><stop offset="1" stop-color="purple"/>
    </linearGradient>
  </defs>
  <rect width="200" height="100" fill="url(#sky)"/>
  <circle cx="100" cy="80" r="30"/>
  <use xlink:href="#sun" x="10"/>
  <text x="10" y="20">Hello <tspan font-weight="bold">world</tspan></text>
</svg>
```

## How to read it

Start with the `namespace!` invocation and the `Drawing` type, note the
two kinds of `Mixed` content and the `Compact` stops, then follow `main`.

Related: `xml-namespaces` (reading documents with other prefixes),
`formats` (layout hints in other formats).
