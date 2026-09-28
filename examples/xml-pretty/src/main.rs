//! Writing indented XML with namespaces with `deser-xml`.
//!
//! An SVG drawing mixes three vocabularies: SVG itself, XLink for
//! references (only in attributes) and Dublin Core for the metadata.  It's
//! written with chosen and generated prefixes, compact and pretty printed.
//! Whitespace can be text in XML, so indentation is only added where it is
//! not: the label with mixed content stays on a single line, the shapes
//! (whose whitespace is skipped) are indented.  Every version is read back
//! into the same value.
use deser::hints::Compact;
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

/// Dublin Core has no prefix in the configuration, it gets a generated
/// one.
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
    /// The stops are written next to each other on a single line.
    #[deser(rename = svg!("stop"), as = Compact)]
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
    const READ: DeserializerConfig = DeserializerConfig::new().resolve_namespaces(true);
    let drawing = drawing();

    // SVG is the default namespace and XLink has its usual prefix, both
    // are declared on the root.  Dublin Core is not configured and gets
    // a generated prefix, which is declared on the root as well.
    const COMPACT: SerializerConfig =
        SerializerConfig::new().namespaces(deser_xml::prefixes![svg as "", xlink]);
    let xml = COMPACT.to_string(&drawing).unwrap();
    println!("compact:\n{xml}\n");
    assert!(xml.starts_with(&format!(
        r#"<svg xmlns="{}" xmlns:xlink="{}" xmlns:ns0="{}" width="200""#,
        svg!(),
        xlink!(),
        dc!()
    )));
    assert!(xml.contains(r##"<use xlink:href="#sun" x="10"/>"##));
    assert!(!xml.contains('\n'));
    assert_eq!(READ.from_str::<Drawing>(&xml).unwrap(), drawing);

    // the same pretty printed, with the declaration on a line of its own
    const PRETTY: SerializerConfig = COMPACT.declaration(true).pretty(Indent::Spaces(2));
    let xml = PRETTY.to_string(&drawing).unwrap();
    println!("pretty:\n{xml}\n");
    assert!(xml.contains("\n  <metadata>\n    <ns0:creator>Jane</ns0:creator>\n"));
    // the stops are compact, the label is mixed content
    assert!(xml.contains(
        r#"<stop offset="0" stop-color="orange"/><stop offset="1" stop-color="purple"/>"#
    ));
    assert!(
        xml.contains(
            r#"  <text x="10" y="20">Hello <tspan font-weight="bold">world</tspan></text>"#
        )
    );
    assert_eq!(READ.from_str::<Drawing>(&xml).unwrap(), drawing);

    // indented with tabs, all prefixes generated in the order the
    // namespaces are first used
    const GENERATED: SerializerConfig = SerializerConfig::new().indent(Indent::Tab);
    let xml = GENERATED.to_string(&drawing).unwrap();
    println!("generated prefixes:\n{xml}");
    assert!(xml.starts_with(&format!(
        r#"<ns0:svg xmlns:ns0="{}" xmlns:ns1="{}" xmlns:ns2="{}""#,
        svg!(),
        dc!(),
        xlink!()
    )));
    assert!(xml.contains("\n\t<ns0:title>Sunset</ns0:title>\n"));
    assert_eq!(READ.from_str::<Drawing>(&xml).unwrap(), drawing);
}
