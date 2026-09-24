#![cfg(feature = "locations")]
use deser::Deserialize;
use deser_location::Spanned;

#[derive(Deserialize, Debug)]
struct Doc {
    name: Spanned<String>,
    list: Spanned<Vec<Spanned<u32>>>,
    nested: Spanned<Nested>,
    unicode: Spanned<String>,
}

#[derive(Deserialize, Debug)]
struct Nested {
    x: Spanned<bool>,
}

const INPUT: &str = r#"{
  "name": "demo",
  "list": [1, 23],
  "nested": {"x": true},
  "unicode": "äöü\n"
}"#;

#[test]
fn test_spans() {
    let doc: Doc = deser_json::Deserializer::new(INPUT)
        .track_locations(true)
        .deserialize()
        .unwrap();
    let span = |s: Option<deser_location::Span>| format!("{:?}", s.unwrap());
    // spans include the quotes of strings
    assert_eq!(span(doc.name.span), "2:11-2:17");
    assert_eq!(span(doc.list.span), "3:11-3:18");
    assert_eq!(span(doc.list.value[0].span), "3:12-3:13");
    assert_eq!(span(doc.list.value[1].span), "3:15-3:17");
    assert_eq!(span(doc.nested.span), "4:13-4:24");
    assert_eq!(span(doc.nested.value.x.span), "4:19-4:23");
    // columns count characters, the escape is two characters
    assert_eq!(span(doc.unicode.span), "5:14-5:21");
    assert_eq!(doc.unicode.value, "äöü\n");
    assert_eq!(doc.unicode.span.unwrap().start.offset, 77);
}

#[test]
fn test_no_tracking() {
    let doc: Doc = deser_json::from_str(INPUT).unwrap();
    assert!(doc.name.span.is_none());
    assert!(doc.nested.span.is_none());
    assert_eq!(doc.list.value[1].value, 23);
}

#[derive(Deserialize, Debug)]
#[deser(tag = "type")]
enum Backend {
    Http {
        url: Spanned<String>,
        headers: Spanned<Vec<Spanned<String>>>,
    },
}

#[test]
fn test_spans_through_buffering() {
    // the tag comes last, so all fields are buffered and replayed
    let input =
        "{\n  \"url\": \"http://x\",\n  \"headers\": [\"a\", \"b\"],\n  \"type\": \"Http\"\n}";
    let backend: Backend = deser_json::Deserializer::new(input)
        .track_locations(true)
        .deserialize()
        .unwrap();
    let Backend::Http { url, headers } = backend;
    let span = |s: Option<deser_location::Span>| format!("{:?}", s.unwrap());
    assert_eq!(url.value, "http://x");
    assert_eq!(span(url.span), "2:10-2:20");
    assert_eq!(span(headers.span), "3:14-3:24");
    assert_eq!(span(headers.value[1].span), "3:20-3:23");
}
