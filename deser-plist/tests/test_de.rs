use std::collections::BTreeMap;

use deser::Deserialize;
use deser::de::Limits;
use deser::ext::Timestamp;
use deser_location::{Span, Spanned};
use deser_plist::{Deserializer, DeserializerConfig, Format, Uid};

use crate::common::{Value, parse, parse_err};

fn xml(body: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">{body}</plist>"
    )
    .into_bytes()
}

/// Builds a binary property list from the encoded objects.  References
/// are one byte.
fn bplist(objects: &[&[u8]], top: u64) -> Vec<u8> {
    let mut out = b"bplist00".to_vec();
    let mut offsets = Vec::new();
    for object in objects {
        offsets.push(out.len() as u8);
        out.extend_from_slice(object);
    }
    let table = out.len() as u64;
    out.extend_from_slice(&offsets);
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 1]);
    out.extend_from_slice(&(objects.len() as u64).to_be_bytes());
    out.extend_from_slice(&top.to_be_bytes());
    out.extend_from_slice(&table.to_be_bytes());
    out
}

#[derive(Debug, PartialEq, Deserialize)]
#[deser(rename_all = "PascalCase")]
struct Info {
    bundle_name: String,
    bundle_version: u32,
    requires_iphone_os: bool,
    icons: Vec<String>,
    #[deser(default)]
    minimum_version: Option<String>,
}

#[test]
fn test_struct() {
    let expected = Info {
        bundle_name: "Demo".into(),
        bundle_version: 42,
        requires_iphone_os: true,
        icons: vec!["a.png".into(), "b.png".into()],
        minimum_version: None,
    };
    let input = xml("<dict>
            <key>BundleName</key><string>Demo</string>
            <key>BundleVersion</key><integer>42</integer>
            <key>RequiresIphoneOs</key><true/>
            <key>Icons</key><array><string>a.png</string><string>b.png</string></array>
        </dict>");
    assert_eq!(deser_plist::from_slice::<Info>(&input).unwrap(), expected);

    // everything is a string in OpenStep, `YES` is a boolean
    let input = b"{
        BundleName = Demo;
        BundleVersion = 42;
        RequiresIphoneOs = YES;
        Icons = (a.png, \"b.png\");
    }";
    assert_eq!(deser_plist::from_slice::<Info>(input).unwrap(), expected);
}

#[test]
fn test_xml_values() {
    let input = xml("<array>
            <string>a &lt;b&gt; &amp; &quot;c&quot; &apos;d&apos; &#65;&#x42;&#x1F600;</string>
            <string>x<![CDATA[<y> & z]]>w</string>
            <string/>
            <string></string>
            <integer>-0x10</integer>
            <integer>+7</integer>
            <integer> 12 </integer>
            <real>1.5</real>
            <real>-inf</real>
            <real>1e3</real>
            <true></true>
            <false />
            <data>AQID
            BA==</data>
            <data/>
            <date>2001-01-01T00:00:00Z</date>
            <array/>
            <dict/>
        </array>");
    assert_eq!(
        parse(&input),
        array![
            "a <b> & \"c\" 'd' AB\u{1F600}",
            "x<y> & zw",
            "",
            "",
            -16i64,
            7u64,
            12u64,
            1.5,
            f64::NEG_INFINITY,
            1000.0,
            true,
            false,
            Value::Bytes(vec![1, 2, 3, 4]),
            Value::Bytes(vec![]),
            Value::Date(Timestamp {
                seconds: 978_307_200,
                nanosecond: 0
            }),
            array![],
            map! {},
        ]
    );
}

#[test]
fn test_xml_document() {
    // no declarations, comments and processing instructions everywhere and
    // a document type with an internal subset
    let input = b"<!-- a -->
        <!DOCTYPE plist [ <!ENTITY x \">\"> ]>
        <?pi x?>
        <plist version='1.0'>
            <!-- b -->
            <dict>
                <!-- c -->
                <key>a</key>
                <!-- d -->
                <integer>1</integer>
            </dict>
        </plist>
        <!-- e -->
    ";
    assert_eq!(parse(input), map! { "a" => 1u64 });
    // Core Foundation also accepts values without `<plist>`
    assert_eq!(parse(b"<array><true/></array>"), array![true]);
    // a byte order mark is skipped
    assert_eq!(
        parse(b"\xef\xbb\xbf<plist><true/></plist>"),
        Value::Bool(true)
    );
}

#[test]
fn test_xml_uid() {
    let input = xml("<array>
            <dict><key>CF$UID</key><integer>7</integer></dict>
            <dict>
                <!-- only exactly this shape is a UID -->
                <key>CF$UID</key><integer>7</integer>
                <key>other</key><integer>8</integer>
            </dict>
            <dict><key>CF$UID</key><string>7</string></dict>
        </array>");
    assert_eq!(
        parse(&input),
        array![
            Uid::new(7),
            map! { "CF$UID" => 7u64, "other" => 8u64 },
            map! { "CF$UID" => "7" },
        ]
    );
    // UIDs fall back to integers
    let value: Vec<u64> = deser_plist::from_slice(&xml(
        "<array><dict><key>CF$UID</key><integer>7</integer></dict></array>",
    ))
    .unwrap();
    assert_eq!(value, [7]);
}

#[test]
fn test_xml_errors() {
    for (input, message) in [
        ("<plist></plist>", "empty plist"),
        ("<plist/>", "empty plist"),
        (
            "<plist><true/><true/></plist>",
            "a plist can only hold a single value",
        ),
        (
            "<plist><dict><integer>1</integer></dict></plist>",
            "expected <key> in <dict>",
        ),
        (
            "<plist><dict><key>a</key></dict></plist>",
            "missing value for key in <dict>",
        ),
        ("<plist><key>a</key></plist>", "<key> outside of <dict>"),
        ("<plist><foo/></plist>", "unknown element"),
        (
            "<plist><array></dict></plist>",
            "closing tag does not match",
        ),
        (
            "<plist><string>a</strong></plist>",
            "closing tag does not match",
        ),
        ("<plist>text</plist>", "unexpected content, expected a tag"),
        ("<plist><string>&foo;</string></plist>", "invalid reference"),
        ("<plist><string>&#0;</string></plist>", "invalid reference"),
        (
            "<plist><string>a<!-- x -->b</string></plist>",
            "unexpected markup in text",
        ),
        ("<plist><integer>1.5</integer></plist>", "invalid integer"),
        (
            "<plist><integer>18446744073709551616</integer></plist>",
            "invalid integer",
        ),
        ("<plist><real>x</real></plist>", "invalid real"),
        ("<plist><date>2001-01-01</date></plist>", "invalid date"),
        ("<plist><data>A*</data></plist>", "invalid base64 data"),
        (
            "<plist><true>x</true></plist>",
            "unexpected content in boolean",
        ),
        ("<plist><array>", "unexpected end of input"),
        (
            "<plist><true/></plist><true/>",
            "unexpected content after the plist",
        ),
        ("<!-- x", "unterminated comment"),
    ] {
        let err = parse_err(input.as_bytes());
        assert!(err.contains(message), "{input}: {err}");
    }
    // text errors have lines and columns
    let err = deser_plist::from_slice::<Value>(b"<plist>\n<array>\n  <foo/>").unwrap_err();
    assert_eq!((err.line(), err.column()), (Some(3), Some(3)));
}

#[test]
fn test_ascii_values() {
    let input = br#"
        // a comment
        {
            /* another comment */
            plain = abc_$/:.-09;
            quoted = "a \"b\" \\ \n\t\U00e4\Ud83d\Ude00\101";
            single = 'it\'s';
            empty = "";
            data = <0fbd7a 12 34>;
            array = (1, 2, 3,);
            nested = { a = (); b = {}; };
            "quoted key" = x;
            short;
        }
    "#;
    assert_eq!(
        parse(input),
        map! {
            "plain" => "abc_$/:.-09",
            "quoted" => "a \"b\" \\ \n\t\u{e4}\u{1F600}A",
            "single" => "it's",
            "empty" => "",
            "data" => Value::Bytes(vec![0x0f, 0xbd, 0x7a, 0x12, 0x34]),
            "array" => array!["1", "2", "3"],
            "nested" => map! { "a" => array![], "b" => map! {} },
            "quoted key" => "x",
            "short" => "short",
        }
    );
    assert_eq!(parse(b"()"), array![]);
    assert_eq!(parse(b"\"top\""), Value::from("top"));
    assert_eq!(parse(b"<00ff>"), Value::Bytes(vec![0, 255]));
}

#[test]
fn test_ascii_errors() {
    let err = parse_err(b"'it''s'");
    assert!(err.contains("unexpected content after the plist"), "{err}");
    for (input, message) in [
        ("{ a = b }", "expected `;`"),
        ("{ a b; }", "expected `=`"),
        ("{ (a) = b; }", "expected a key or `}`"),
        ("(a b)", "expected `,` or `)`"),
        ("(a, ;)", "expected a value"),
        ("\"abc", "unterminated string"),
        ("(<0fx>)", "invalid character in data"),
        ("(<0fb>)", "odd number of hex digits in data"),
        ("\"\\U\"", "invalid unicode escape"),
        ("\"\\Ud800\"", "unpaired surrogate in escape"),
        (
            "\"\\351\"",
            "octal escapes of non-ASCII characters are not supported",
        ),
        ("/* x", "unterminated comment"),
        ("{", "unexpected end of input"),
        ("a = b; c", "unexpected end of input"),
        ("(a) (b)", "unexpected content after the plist"),
    ] {
        let err = parse_err(input.as_bytes());
        assert!(err.contains(message), "{input}: {err}");
    }
}

#[test]
fn test_strings_file() {
    // `.strings` files are dictionaries without braces
    let input = b"/* Title */\n\"title\" = \"Hello\";\n\"bye\" = \"Bye\";\n";
    assert_eq!(parse(input), map! { "title" => "Hello", "bye" => "Bye" });
    // an empty file is an empty dictionary
    assert_eq!(parse(b""), map! {});
    assert_eq!(parse(b"/* nothing */"), map! {});
    // they are often UTF-16
    let mut utf16 = vec![0xff, 0xfe];
    for unit in "\"k\" = \"\u{e4}\";".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    assert_eq!(Format::detect(&utf16), Format::Ascii);
    assert_eq!(parse(&utf16), map! { "k" => "\u{e4}" });
}

#[test]
fn test_ascii_lexical() {
    // all strings of OpenStep are lexical and parsed by the target type
    #[derive(Debug, PartialEq, Deserialize)]
    struct Values {
        int: i32,
        float: f64,
        yes: bool,
        no: bool,
        quoted: u8,
        text: String,
    }
    let value: Values = deser_plist::from_slice(
        b"{ int = -1; float = 0.5; yes = YES; no = NO; quoted = \"7\"; text = 42; }",
    )
    .unwrap();
    assert_eq!(
        value,
        Values {
            int: -1,
            float: 0.5,
            yes: true,
            no: false,
            quoted: 7,
            text: "42".into(),
        }
    );
    // keys are lexical in all formats
    let value: BTreeMap<u32, bool> =
        deser_plist::from_slice(&xml("<dict><key>1</key><true/><key>2</key><false/></dict>"))
            .unwrap();
    assert_eq!(value, BTreeMap::from([(1, true), (2, false)]));
}

#[test]
fn test_utf16_text() {
    let text = "<plist><string>\u{e4}\u{1F600}</string></plist>";
    let mut le = vec![0xff, 0xfe];
    let mut be = vec![0xfe, 0xff];
    for unit in text.encode_utf16() {
        le.extend_from_slice(&unit.to_le_bytes());
        be.extend_from_slice(&unit.to_be_bytes());
    }
    for input in [le, be] {
        assert_eq!(Format::detect(&input), Format::Xml);
        assert_eq!(parse(&input), Value::from("\u{e4}\u{1F600}"));
    }
    let err = parse_err(b"\xff\xfe\x00\xd8");
    assert!(err.contains("invalid UTF-16"), "{err}");
    let err = parse_err(b"<plist><string>\xff</string></plist>");
    assert!(err.contains("invalid UTF-8"), "{err}");
}

#[test]
fn test_borrowing() {
    #[derive(Deserialize)]
    struct Borrowed<'a> {
        a: &'a str,
        b: &'a str,
    }
    let value: Borrowed = deser_plist::from_slice(b"{ a = x; b = \"y z\"; }").unwrap();
    assert_eq!((value.a, value.b), ("x", "y z"));
    let input = xml("<dict><key>a</key><string>x</string><key>b</key><string>y z</string></dict>");
    let value: Borrowed = deser_plist::from_slice(&input).unwrap();
    assert_eq!((value.a, value.b), ("x", "y z"));
    let input = bplist(
        &[
            b"\xd2\x01\x02\x03\x04",
            b"\x51a",
            b"\x51b",
            b"\x51x",
            b"\x53y z",
        ],
        0,
    );
    let value: Borrowed = deser_plist::from_slice(&input).unwrap();
    assert_eq!((value.a, value.b), ("x", "y z"));
    // strings with escapes cannot be borrowed
    assert!(deser_plist::from_slice::<Borrowed>(b"{ a = \"\\n\"; b = x; }").is_err());
}

#[test]
fn test_binary_objects() {
    let input = bplist(
        &[
            b"\xaf\x10\x10\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10",
            b"\x00",                                                                 // null
            b"\x10\xff",                                                             // 255
            b"\x11\xff\xff",                                                         // 65535
            b"\x13\xff\xff\xff\xff\xff\xff\xff\xff",                                 // -1
            b"\x14\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00", // 2^64
            b"\x22\x3f\xc0\x00\x00",                                                 // 1.5f32
            b"\x33\xc0\x00\x00\x00\x00\x00\x00\x00", // 2 seconds before 2001
            b"\x42\x01\x02",                         // data
            b"\x61\x00\xe4",                         // "ä"
            b"\x80\x05",                             // uid 5
            b"\x83\x00\x00\x01\x00",                 // uid 256
            b"\xc2\x02\x03",                         // set
            b"\xd1\x09\x02",                         // {"ä": 255}
            b"\x09",                                 // true
            b"\x4f\x10\x02\x01\x02",                 // data with an extended length
            b"\x08",                                 // false
        ],
        0,
    );
    assert_eq!(
        parse(&input),
        array![
            Value::Null,
            255u64,
            65535u64,
            -1i64,
            Value::I128(1 << 64),
            1.5,
            Value::Date(Timestamp {
                seconds: 978_307_198,
                nanosecond: 0
            }),
            Value::Bytes(vec![1, 2]),
            "\u{e4}",
            Uid::new(5),
            Uid::new(256),
            array![255u64, 65535u64],
            map! { "\u{e4}" => 255u64 },
            true,
            Value::Bytes(vec![1, 2]),
            false,
        ]
    );
}

#[test]
fn test_binary_shared() {
    // objects can be referenced multiple times, also containers
    let input = bplist(&[b"\xa3\x01\x01\x02", b"\xa1\x02", b"\x51a"], 0);
    assert_eq!(parse(&input), array![array!["a"], array!["a"], "a"]);

    // but containers that contain themselves are an error
    let err = parse_err(&bplist(&[b"\xa1\x01", b"\xa1\x00"], 0));
    assert!(err.contains("object references itself"), "{err}");

    // and shared containers must not expand exponentially
    let mut objects: Vec<Vec<u8>> = (0..40u8).map(|idx| vec![0xa2, idx + 1, idx + 1]).collect();
    objects.push(b"\x09".to_vec());
    let objects: Vec<&[u8]> = objects.iter().map(|x| x.as_slice()).collect();
    let err = parse_err(&bplist(&objects, 0));
    assert!(
        err.contains("shared objects expand beyond the size of the input"),
        "{err}"
    );
}

#[test]
fn test_binary_errors() {
    for (input, message) in [
        (
            b"bplist01".to_vec(),
            "unsupported binary property list version",
        ),
        (b"bplist00".to_vec(), "unexpected end of input"),
        (bplist(&[b"\xa1\x05"], 0), "invalid object reference"),
        (
            bplist(&[b"\xa2\x00"], 0),
            "object extends beyond the object table",
        ),
        (
            bplist(&[b"\x5f\x10"], 0),
            "object extends beyond the object table",
        ),
        (bplist(&[b"\x4f\x51"], 0), "expected integer"),
        (bplist(&[b"\x15"], 0), "invalid size of integer"),
        (bplist(&[b"\x21\x00\x00"], 0), "invalid size of real"),
        (bplist(&[b"\x88\x00"], 0), "uid out of range"),
        (bplist(&[b"\x0f"], 0), "unknown object type"),
        (bplist(&[b"\xf0"], 0), "unknown object type"),
        (
            bplist(&[b"\xd1\x01\x01", b"\x09"], 0),
            "dictionary key is not a string",
        ),
        (bplist(&[b"\x52\xff\xfe"], 0), "invalid string"),
        (bplist(&[b"\x61\xd8\x00"], 0), "invalid string"),
        (
            bplist(&[b"\x33\x7f\xf0\x00\x00\x00\x00\x00\x00"], 0),
            "date out of range",
        ),
        (bplist(&[b"\x09"], 1), "invalid top object"),
    ] {
        let err = parse_err(&input);
        assert!(err.contains(message), "{input:?}: {err}");
    }

    // offsets have to point into the object table
    let mut input = bplist(&[b"\x09"], 0);
    let table = input.len() - 33;
    input[table] = 0x20;
    let err = parse_err(&input);
    assert!(err.contains("invalid object offset"), "{err}");
}

#[test]
fn test_dates() {
    let input = xml("<date>2024-06-19T19:22:45Z</date>");
    let value: std::time::SystemTime = deser_plist::from_slice(&input).unwrap();
    assert_eq!(
        value
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        1_718_824_965
    );
    // dates fall back to strings
    let value: String = deser_plist::from_slice(&input).unwrap();
    assert_eq!(value, "2024-06-19T19:22:45Z");
    // binary dates are rounded to microseconds
    let seconds = 740_000_000.1f64;
    let mut object = vec![0x33];
    object.extend_from_slice(&seconds.to_be_bytes());
    let value: Timestamp = deser_plist::from_slice(&bplist(&[&object], 0)).unwrap();
    assert_eq!(value.nanosecond, 100_000_000);
}

#[test]
fn test_deserializer() {
    for (input, format) in [
        (&b"<plist><integer>1</integer></plist>"[..], Format::Xml),
        (&b"1"[..], Format::Ascii),
    ] {
        let mut de = Deserializer::from_slice(input);
        assert_eq!(de.format(), format);
        assert_eq!(de.deserialize::<u32>().unwrap(), 1);
    }

    // the depth is limited with a layer
    let limited = |input: &[u8]| {
        Deserializer::from_slice(input).deserialize_with::<Value, _>(|driver| {
            driver.push_layer(Limits::builder().max_depth(2).build())
        })
    };
    assert!(limited(b"((()))").is_err());
    assert!(limited(b"(())").is_ok());
}

#[test]
fn test_deep_nesting() {
    let depth = 100_000;
    let input = format!("{}{}", "(".repeat(depth), ")".repeat(depth));
    let value: deser::de::Recording = deser_plist::from_slice(input.as_bytes()).unwrap();
    assert_eq!(value.events().count(), depth * 2);
    let input = xml(&format!(
        "{}{}",
        "<array>".repeat(depth),
        "</array>".repeat(depth)
    ));
    let value: deser::de::Recording = deser_plist::from_slice(&input).unwrap();
    assert_eq!(value.events().count(), depth * 2);
}

#[test]
fn test_locations() {
    #[derive(Deserialize)]
    struct Doc {
        a: Spanned<u32>,
        b: Spanned<Vec<Spanned<String>>>,
    }
    let span = |s: Option<Span>| format!("{:?}", s.unwrap());
    let config = DeserializerConfig::builder().track_locations(true).build();

    let doc: Doc = config
        .from_slice(b"{\n  a = 1;\n  b = (x, \"y\");\n}")
        .unwrap();
    assert_eq!(span(doc.a.span), "2:7-2:8");
    assert_eq!(span(doc.b.span), "3:7-3:15");
    assert_eq!(span(doc.b.value[1].span), "3:11-3:14");

    let doc: Doc = config
        .from_slice(b"<plist><dict>\n<key>a</key><integer>1</integer>\n<key>b</key><array><string>x</string></array>\n</dict></plist>")
        .unwrap();
    assert_eq!(span(doc.a.span), "2:13-2:33");
    assert_eq!(span(doc.b.span), "3:13-3:46");
    assert_eq!(span(doc.b.value[0].span), "3:20-3:38");
}
