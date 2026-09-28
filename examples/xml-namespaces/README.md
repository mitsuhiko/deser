# xml-namespaces

```
cargo run -p xml-namespaces
```

## Why

Names in XML documents that mix vocabularies are in namespaces, and a
namespace is identified by its URI, not by its prefix: `<a:title>` and
`<title>` are the same element if `a` and the default namespace are bound
to the same URI.  Matching the prefixes as written breaks as soon as a
document picks other prefixes.  With `resolve_namespaces` `deser-xml`
passes names on as `{uri}local` (the notation of James Clark), which is
what the types match, and the serializer turns them back into prefixes.

## What it shows

- `deser_xml::namespace!` defining `atom!`, `dc!`, `media!` and `xhtml!`
  for the names of fields and variants (`atom!("title")` is
  `{http://www.w3.org/2005/Atom}title`, `atom!()` is the URI).
- Two documents with different prefixes (a default namespace, prefixes
  on the root and on inner elements) read into the same value with
  `DeserializerConfig::resolve_namespaces`.
- Attributes without prefix are in no namespace (`@href`), the `xml`
  prefix is kept (`@xml:lang`).
- XHTML content read in order with `Mixed`, its variants named after
  names in the XHTML namespace.
- An element of a vocabulary the types do not know (Media RSS) kept in a
  flattened map of dynamic values under its resolved name.
- Without resolving, names are compared as written and the second
  document does not have an Atom title.
- Writing the feed with the prefixes of `deser_xml::prefixes!`, which are
  named after the namespace macros unless given with `as`
  (`prefixes![atom as "", dc, media, xhtml as "h"]`), and with generated
  prefixes (`ns0`, ...), and reading both back.  Every namespace has one
  prefix in the document, all are declared on the root element.

## What you should see

The feed, the key of the thumbnail, then

```
as written: MissingField: missing field
  `{http://www.w3.org/2005/Atom}title` at line 14 column 1
```

followed by the feed as XML with the chosen prefixes and with generated
ones.

## How to read it

Start with the `namespace!` invocation and the `Feed` type, compare the
two documents `USUAL` and `UNUSUAL`, then follow `main`.

Related: `renames` (names given by constants and macros), `enums`.
