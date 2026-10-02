use deser::{Deserialize, Serialize};
use deser_xml::{DeserializerConfig, Mixed, from_str, to_string};

#[derive(Debug, Deserialize, Serialize, PartialEq)]
enum Inline {
    #[deser(rename = "$text")]
    Text(String),
    #[deser(rename = "b")]
    Bold(String),
    #[deser(rename = "a")]
    Link(Link),
    #[deser(rename = "em")]
    Emphasis(Mixed<Inline>),
    #[deser(rename = "br")]
    Break,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct Link {
    #[deser(rename = "@href")]
    href: String,
    #[deser(rename = "$text")]
    text: String,
}

fn text(text: &str) -> Inline {
    Inline::Text(text.into())
}

fn bold(text: &str) -> Inline {
    Inline::Bold(text.into())
}

#[test]
fn test_in_order() {
    let p: Mixed<Inline> = from_str("<p>x <b>y</b> z <b>w</b></p>").unwrap();
    assert_eq!(p.0, [text("x "), bold("y"), text(" z "), bold("w")]);

    // attributes are skipped
    let p: Mixed<Inline> = from_str(r#"<p class="c">x <b>y</b></p>"#).unwrap();
    assert_eq!(p.0, [text("x "), bold("y")]);

    // text only and empty elements
    let p: Mixed<Inline> = from_str("<p>only text</p>").unwrap();
    assert_eq!(p.0, [text("only text")]);
    let p: Mixed<Inline> = from_str("<p/>").unwrap();
    assert_eq!(p.0, []);
    let p: Mixed<Inline> = from_str(r#"<p class="c"/>"#).unwrap();
    assert_eq!(p.0, []);

    // elements with attributes, nested mixed content and unit variants
    let p: Mixed<Inline> =
        from_str(r#"<p>see <a href="/x">here</a><br/>for <em>very <b>much</b></em> more</p>"#)
            .unwrap();
    assert_eq!(
        p.0,
        [
            text("see "),
            Inline::Link(Link {
                href: "/x".into(),
                text: "here".into()
            }),
            Inline::Break,
            text("for "),
            Inline::Emphasis(Mixed::from(vec![text("very "), bold("much")])),
            text(" more"),
        ]
    );

    // unknown elements are errors
    let err = from_str::<Mixed<Inline>>("<p>x <i>y</i></p>").unwrap_err();
    assert!(err.message().contains("unknown variant"), "{err}");
}

#[test]
fn test_whitespace() {
    // whitespace between elements is text in mixed content
    let p: Mixed<Inline> = from_str("<p><b>x</b> <b>y</b>\n</p>").unwrap();
    assert_eq!(p.0, [bold("x"), text(" "), bold("y"), text("\n")]);
    let p: Mixed<Inline> = from_str("<p> <b>x</b></p>").unwrap();
    assert_eq!(p.0, [text(" "), bold("x")]);

    // but not in the elements in it
    #[derive(Debug, Deserialize, PartialEq)]
    enum Block {
        #[deser(rename = "list")]
        List { item: Vec<String> },
        #[deser(rename = "$text")]
        Text(String),
    }
    let doc: Mixed<Block> =
        from_str("<doc>a\n  <list>\n    <item>1</item>\n    <item>2</item>\n  </list>\n</doc>")
            .unwrap();
    assert_eq!(
        doc.0,
        [
            Block::Text("a\n  ".into()),
            Block::List {
                item: vec!["1".into(), "2".into()]
            },
            Block::Text("\n".into()),
        ]
    );

    // and not after it
    #[derive(Debug, Deserialize, PartialEq)]
    struct Doc {
        p: Mixed<Inline>,
        after: Vec<String>,
    }
    let doc: Doc =
        from_str("<doc>\n  <p><b>x</b> <b>y</b></p>\n  <after>1</after>\n</doc>").unwrap();
    assert_eq!(doc.p.0, [bold("x"), text(" "), bold("y")]);
    assert_eq!(doc.after, ["1"]);
}

#[test]
fn test_flattened() {
    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    #[deser(rename = "section")]
    struct Section {
        #[deser(rename = "@id")]
        id: String,
        title: String,
        #[deser(flatten)]
        content: Mixed<Inline>,
    }

    // the fields take their keys, also between the content
    let section: Section = from_str(
        r#"<section id="s1" class="x">intro <b>y</b> <title>T</title> <b>z</b> end</section>"#,
    )
    .unwrap();
    assert_eq!(section.id, "s1");
    assert_eq!(section.title, "T");
    // the texts around the title are not joined
    assert_eq!(
        section.content.0,
        [
            text("intro "),
            bold("y"),
            text(" "),
            text(" "),
            bold("z"),
            text(" end")
        ]
    );

    // whitespace is kept from the first value on
    let section: Section =
        from_str(r#"<section id="s1"><title>T</title> <b>y</b> <b>z</b></section>"#).unwrap();
    assert_eq!(section.content.0, [bold("y"), text(" "), bold("z")]);

    // without content
    let section: Section = from_str(r#"<section id="s1"><title>T</title></section>"#).unwrap();
    assert_eq!(section.content.0, []);

    // round trip
    let section = Section {
        id: "s1".into(),
        title: "T".into(),
        content: Mixed::from(vec![text("a & "), bold("b"), Inline::Break, text("c")]),
    };
    let xml = to_string(&section).unwrap();
    assert_eq!(
        xml,
        r#"<section id="s1"><title>T</title>a &amp; <b>b</b><br/>c</section>"#
    );
    assert_eq!(from_str::<Section>(&xml).unwrap(), section);
}

#[test]
fn test_serialize() {
    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    #[deser(rename = "p")]
    struct Paragraph(Mixed<Inline>);

    let p = Paragraph(Mixed::from(vec![
        text("see "),
        Inline::Link(Link {
            href: "/x".into(),
            text: "here".into(),
        }),
        text(" and "),
        Inline::Emphasis(Mixed::from(vec![bold("this")])),
    ]));
    let xml = to_string(&p).unwrap();
    assert_eq!(
        xml,
        r#"<p>see <a href="/x">here</a> and <em><b>this</b></em></p>"#
    );
    assert_eq!(from_str::<Paragraph>(&xml).unwrap(), p);
}

#[test]
fn test_config() {
    let config = DeserializerConfig::builder()
        .attribute_prefix("-")
        .text_key("#text")
        .build();

    #[derive(Debug, Deserialize, PartialEq)]
    enum Inline {
        #[deser(rename = "#text")]
        Text(String),
        #[deser(rename = "b")]
        Bold(String),
    }
    let p: Mixed<Inline> = config.from_str(r#"<p x="1">x <b>y</b></p>"#).unwrap();
    assert_eq!(p.0, [Inline::Text("x ".into()), Inline::Bold("y".into())]);
}

#[test]
fn test_borrowed() {
    #[derive(Debug, Deserialize, PartialEq)]
    enum Inline<'a> {
        #[deser(rename = "$text")]
        Text(&'a str),
        #[deser(rename = "b")]
        Bold(&'a str),
    }
    let input = String::from("<p>x <b>y</b></p>");
    let p: Mixed<Inline> = from_str(&input).unwrap();
    assert_eq!(p.0, [Inline::Text("x "), Inline::Bold("y")]);
    // elements that are only text
    let input = String::from("<p>z</p>");
    let p: Mixed<Inline> = from_str(&input).unwrap();
    assert_eq!(p.0, [Inline::Text("z")]);
}

#[test]
fn test_serialize_errors() {
    #[derive(Serialize)]
    #[deser(rename = "p")]
    struct Strings(Mixed<String>);
    let err = to_string(&Strings(Mixed::from(vec!["x".into()]))).unwrap_err();
    assert_eq!(
        err.message(),
        "the values of mixed content must be structs or externally tagged enums"
    );
}

#[test]
fn test_skip_whitespace() {
    use deser::adapters::TrimWhitespace;
    use deser_xml::SkipWhitespace;

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    enum Block {
        #[deser(rename = "$text")]
        Text(#[deser(as = TrimWhitespace)] String),
        #[deser(rename = "p")]
        Paragraph(Mixed<Inline>),
    }

    let doc: Mixed<Block, SkipWhitespace> =
        from_str("<doc>\n  <p>x <b>y</b> <b>z</b></p>\n  some text\n  <p>w</p>\n</doc>").unwrap();
    assert_eq!(
        doc.0,
        [
            // the paragraphs keep their whitespace
            Block::Paragraph(Mixed::from(vec![
                text("x "),
                bold("y"),
                text(" "),
                bold("z")
            ])),
            Block::Text("some text".into()),
            Block::Paragraph(Mixed::from(vec![text("w")])),
        ]
    );

    // elements that are only whitespace are empty
    let doc: Mixed<Block, SkipWhitespace> = from_str("<doc> \n </doc>").unwrap();
    assert_eq!(doc.0, []);
    let doc: Mixed<Block> = from_str("<doc> \n </doc>").unwrap();
    assert_eq!(doc.0, [Block::Text("".into())]);

    // and in reverse: paragraphs without whitespace in a document with it
    #[derive(Debug, Deserialize, PartialEq)]
    enum Loose {
        #[deser(rename = "$text")]
        Text(String),
        #[deser(rename = "p")]
        Paragraph(Mixed<Inline, SkipWhitespace>),
    }
    let doc: Mixed<Loose> = from_str("<doc><p><b>y</b> <b>z</b></p> </doc>").unwrap();
    assert_eq!(
        doc.0,
        [
            Loose::Paragraph(Mixed::from(vec![bold("y"), bold("z")])),
            Loose::Text(" ".into())
        ]
    );
}

#[test]
fn test_text_field() {
    use deser::adapters::SkipBlank;

    // whitespace between elements is text in elements with mixed content,
    // also for a field of the text
    #[derive(Debug, Deserialize, PartialEq)]
    struct Element {
        #[deser(rename = "$text")]
        text: Vec<String>,
        #[deser(flatten)]
        content: Mixed<Inline>,
    }
    let element: Element = from_str("<e>a <b>x</b> <b>y</b> c</e>").unwrap();
    assert_eq!(element.text, ["a ", " ", " c"]);
    assert_eq!(element.content.0, [bold("x"), bold("y")]);

    // which `SkipBlank` leaves out
    #[derive(Debug, Deserialize, PartialEq)]
    struct NonBlank {
        #[deser(rename = "$text", as = Vec<SkipBlank>)]
        text: Vec<String>,
        #[deser(flatten)]
        content: Mixed<Inline>,
    }
    let element: NonBlank = from_str("<e>a <b>x</b> <b>y</b> c</e>").unwrap();
    assert_eq!(element.text, ["a ", " c"]);
    assert_eq!(element.content.0, [bold("x"), bold("y")]);
}
