use deser::Deserialize;
use deser_location::{Span, Spanned};
use deser_toml::{Datetime, DeserializerConfig};

#[derive(Deserialize, Debug)]
struct Doc {
    name: Spanned<String>,
    list: Spanned<Vec<Spanned<u32>>>,
    when: Spanned<Datetime>,
    inline: Spanned<Nested>,
    dotted: Spanned<Nested>,
    table: Spanned<Nested>,
    items: Spanned<Vec<Spanned<Nested>>>,
}

#[derive(Deserialize, Debug)]
struct Nested {
    x: Spanned<bool>,
}

const INPUT: &str = "\
name = \"äöü\"
list = [1, 23]   # comment
when = 1979-05-27 07:32Z
inline = { x = true }
dotted.x = false

[table]
x = true

[[items]]
x = true
";

#[test]
fn test_spans() {
    let doc: Doc = DeserializerConfig::new()
        .track_locations(true)
        .from_str(INPUT)
        .unwrap();
    let span = |s: Option<Span>| format!("{:?}", s.unwrap());
    // spans include the quotes of strings, columns count characters
    assert_eq!(span(doc.name.span), "1:8-1:13");
    assert_eq!(span(doc.list.span), "2:8-2:15");
    assert_eq!(span(doc.list.value[0].span), "2:9-2:10");
    assert_eq!(span(doc.list.value[1].span), "2:12-2:14");
    assert_eq!(span(doc.when.span), "3:8-3:25");
    assert_eq!(span(doc.inline.span), "4:10-4:22");
    assert_eq!(span(doc.inline.value.x.span), "4:16-4:20");
    // tables created by dotted keys report the key
    assert_eq!(span(doc.dotted.span), "5:1-5:7");
    assert_eq!(span(doc.dotted.value.x.span), "5:12-5:17");
    // tables report their header
    assert_eq!(span(doc.table.span), "7:1-7:8");
    assert_eq!(span(doc.table.value.x.span), "8:5-8:9");
    assert_eq!(span(doc.items.span), "10:1-10:10");
    assert_eq!(span(doc.items.value[0].span), "10:1-10:10");
}

#[test]
fn test_root_span() {
    let doc: Spanned<Nested> = DeserializerConfig::new()
        .track_locations(true)
        .from_str("x = true\n")
        .unwrap();
    assert_eq!(format!("{:?}", doc.span.unwrap()), "1:1-2:1");
}
