use deser::Deserialize;
use deser_location::{Span, Spanned};
use deser_yaml::Deserializer;

#[derive(Deserialize, Debug)]
struct Doc {
    name: Spanned<String>,
    list: Spanned<Vec<Spanned<u32>>>,
    flow: Spanned<Vec<u32>>,
    nested: Spanned<Nested>,
    tagged: Spanned<String>,
    alias: Spanned<Nested>,
}

#[derive(Deserialize, Debug)]
struct Nested {
    x: Spanned<bool>,
}

const INPUT: &str = "\
name: \"demo\"
list:
  - 1
  - 23
flow: [1, 2]   # comment
nested: &n
  x: true

# comment
tagged: !!str äöü
alias: *n
";

#[test]
fn test_spans() {
    let doc: Doc = Deserializer::new(INPUT)
        .track_locations(true)
        .deserialize()
        .unwrap();
    let span = |s: Option<Span>| format!("{:?}", s.unwrap());
    // spans include the quotes of strings
    assert_eq!(span(doc.name.span), "1:7-1:13");
    // block collections end with their last value
    assert_eq!(span(doc.list.span), "3:3-4:7");
    assert_eq!(span(doc.list.value[0].span), "3:5-3:6");
    assert_eq!(span(doc.list.value[1].span), "4:5-4:7");
    assert_eq!(span(doc.flow.span), "5:7-5:13");
    // the properties are part of the node, columns count characters
    assert_eq!(span(doc.nested.span), "6:9-7:10");
    assert_eq!(span(doc.nested.value.x.span), "7:6-7:10");
    assert_eq!(span(doc.tagged.span), "10:9-10:18");
    // aliases report the location of the anchored node
    assert_eq!(span(doc.alias.span), "6:9-7:10");
    assert_eq!(span(doc.alias.value.x.span), "7:6-7:10");
}
