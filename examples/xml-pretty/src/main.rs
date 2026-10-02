//! Writing indented XML with namespaces with `deser-xml`.
//!
//! An SVG drawing mixes three vocabularies: SVG itself, XLink for
//! references (only in attributes) and Dublin Core for the metadata.  It's
//! written compact and pretty printed.  Whitespace can be text in XML, so
//! indentation is only added where it is not: the label with mixed content
//! stays on a single line, the shapes (whose whitespace is skipped) are
//! indented.  Both versions are read back into the same value.
use deser::{Deserialize, Serialize};
use deser_xml::{DeserializerConfig, Indent, Mixed, SerializerConfig, SkipWhitespace};

// `svg!("rect")` is `"{http://www.w3.org/2000/svg}rect"`, `xlink!(@ "href")`
// the attribute in the XLink namespace and `svg!()` the URI
deser_xml::namespace!(
    svg = "http://www.w3.org/2000/svg",
    xlink = "http://www.w3.org/1999/xlink",
    dc = "http://purl.org/dc/elements/1.1/",
);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename = svg!("svg"))]
struct Drawing {
    #[deser(rename = "@width")]
    width: u32,
    #[deser(rename = "@height")]
    height: u32,
    #[deser(rename = svg!("title"))]
    title: String,
    #[deser(rename = svg!("metadata"))]
    metadata: Metadata,
    #[deser(rename = svg!("defs"))]
    defs: Defs,
    /// The shapes in the order they are painted.  The whitespace between
    /// them is not content, so they are indented.
    #[deser(flatten)]
    shapes: Mixed<Shape, SkipWhitespace>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Metadata {
    #[deser(rename = dc!("creator"))]
    creators: Vec<String>,
    #[deser(rename = dc!("date"))]
    date: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Defs {
    #[deser(rename = svg!("linearGradient"))]
    gradients: Vec<Gradient>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Gradient {
    #[deser(rename = "@id")]
    id: String,
    #[deser(rename = svg!("stop"))]
    stops: Vec<Stop>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Stop {
    #[deser(rename = "@offset")]
    offset: f32,
    #[deser(rename = "@stop-color")]
    color: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Shape {
    #[deser(rename = svg!("rect"))]
    Rect {
        #[deser(rename = "@width")]
        width: u32,
        #[deser(rename = "@height")]
        height: u32,
        #[deser(rename = "@fill")]
        fill: String,
    },
    #[deser(rename = svg!("circle"))]
    Circle {
        #[deser(rename = "@cx")]
        cx: u32,
        #[deser(rename = "@cy")]
        cy: u32,
        #[deser(rename = "@r")]
        r: u32,
    },
    /// A reference to another element, in the XLink namespace.
    #[deser(rename = svg!("use"))]
    Use {
        #[deser(rename = xlink!(@ "href"))]
        href: String,
        #[deser(rename = "@x")]
        x: u32,
    },
    #[deser(rename = svg!("text"))]
    Text(Label),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Label {
    #[deser(rename = "@x")]
    x: u32,
    #[deser(rename = "@y")]
    y: u32,
    /// Mixed content keeps whitespace as text, it's never indented.
    #[deser(flatten)]
    content: Mixed<Span>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Span {
    #[deser(rename = "$text")]
    Text(String),
    #[deser(rename = svg!("tspan"))]
    Bold(Bold),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Bold {
    #[deser(rename = "@font-weight")]
    weight: String,
    #[deser(rename = "$text")]
    text: String,
}

fn drawing() -> Drawing {
    Drawing {
        width: 200,
        height: 100,
        title: "Sunset".into(),
        metadata: Metadata {
            creators: vec!["Jane".into(), "John".into()],
            date: "2024-05-01".into(),
        },
        defs: Defs {
            gradients: vec![Gradient {
                id: "sky".into(),
                stops: vec![
                    Stop {
                        offset: 0.0,
                        color: "orange".into(),
                    },
                    Stop {
                        offset: 1.0,
                        color: "purple".into(),
                    },
                ],
            }],
        },
        shapes: Mixed::from(vec![
            Shape::Rect {
                width: 200,
                height: 100,
                fill: "url(#sky)".into(),
            },
            Shape::Circle {
                cx: 100,
                cy: 80,
                r: 30,
            },
            Shape::Use {
                href: "#sun".into(),
                x: 10,
            },
            Shape::Text(Label {
                x: 10,
                y: 20,
                content: Mixed::from(vec![
                    Span::Text("Hello ".into()),
                    Span::Bold(Bold {
                        weight: "bold".into(),
                        text: "world".into(),
                    }),
                ]),
            }),
        ]),
    }
}

fn main() {
    const READ: DeserializerConfig = DeserializerConfig::builder()
        .resolve_namespaces(true)
        .build();
    let drawing = drawing();

    // SVG is the default namespace, XLink and Dublin Core have their
    // usual prefixes.  All are declared on the root.
    const COMPACT: SerializerConfig =
        SerializerConfig::new().namespaces(deser_xml::prefixes![svg as "", xlink, dc]);
    let xml = COMPACT.to_string(&drawing).unwrap();
    println!("compact:\n{xml}\n");
    assert!(xml.starts_with(&format!(
        r#"<svg xmlns="{}" xmlns:xlink="{}" xmlns:dc="{}" width="200""#,
        svg!(),
        xlink!(),
        dc!()
    )));
    assert!(xml.contains(r##"<use xlink:href="#sun" x="10"/>"##));
    assert!(!xml.contains('\n'));
    assert_eq!(READ.from_str::<Drawing>(&xml).unwrap(), drawing);

    // the same pretty printed, with the declaration on a line of its own
    let mut pretty = COMPACT.clone();
    pretty.set_declaration(true);
    pretty.set_pretty(Indent::Spaces(2));
    let xml = pretty.to_string(&drawing).unwrap();
    println!("pretty:\n{xml}");
    assert!(xml.contains("\n  <metadata>\n    <dc:creator>Jane</dc:creator>\n"));
    // the label is mixed content, its whitespace is text
    assert!(
        xml.contains(
            r#"  <text x="10" y="20">Hello <tspan font-weight="bold">world</tspan></text>"#
        )
    );
    assert_eq!(READ.from_str::<Drawing>(&xml).unwrap(), drawing);
}
