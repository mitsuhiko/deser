use std::collections::BTreeMap;
use std::time::{Duration, UNIX_EPOCH};

use deser::Serialize;
use deser::ext::{Datetime, Timestamp, Uuid};
use deser_plist::{Format, Serializer, SerializerConfig, Uid};

use crate::common::{Value, parse, write, write_str};

const XML_HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n";

fn xml_doc(body: &str) -> String {
    format!("{XML_HEADER}{body}</plist>\n")
}

#[derive(Serialize)]
#[deser(rename_all = "PascalCase")]
struct Info {
    name: String,
    version: u32,
    enabled: bool,
    scale: f64,
    tags: Vec<&'static str>,
    missing: Option<u32>,
    nested: BTreeMap<&'static str, i64>,
}

fn info() -> Info {
    Info {
        name: "a <b> & c".into(),
        version: 42,
        enabled: true,
        scale: 1.5,
        tags: vec!["x", "y"],
        missing: None,
        nested: BTreeMap::from([("neg", -1)]),
    }
}

#[test]
fn test_xml() {
    assert_eq!(
        deser_plist::to_string(&info()).unwrap(),
        xml_doc(
            "<dict>
\t<key>Name</key>
\t<string>a &lt;b&gt; &amp; c</string>
\t<key>Version</key>
\t<integer>42</integer>
\t<key>Enabled</key>
\t<true/>
\t<key>Scale</key>
\t<real>1.5</real>
\t<key>Tags</key>
\t<array>
\t\t<string>x</string>
\t\t<string>y</string>
\t</array>
\t<key>Nested</key>
\t<dict>
\t\t<key>neg</key>
\t\t<integer>-1</integer>
\t</dict>
</dict>
"
        )
    );
}

#[test]
fn test_ascii() {
    assert_eq!(
        write_str(&info(), Format::Ascii),
        "{
\tName = \"a <b> & c\";
\tVersion = 42;
\tEnabled = YES;
\tScale = 1.5;
\tTags = (
\t\tx,
\t\ty,
\t);
\tNested = {
\t\tneg = -1;
\t};
}
"
    );
    assert_eq!(write_str(&"plain", Format::Ascii), "plain\n");
    assert_eq!(
        write_str(&"a \"q\" \\ \n\t\r\u{e4}\u{1F600}\x01", Format::Ascii),
        "\"a \\\"q\\\" \\\\ \\n\\t\\r\\U00e4\\Ud83d\\Ude00\\U0001\"\n"
    );
    assert_eq!(write_str(&"", Format::Ascii), "\"\"\n");
    assert_eq!(
        write_str(&vec![1e20, f64::NAN], Format::Ascii),
        "(\n\t\"1e+20\",\n\tnan,\n)\n"
    );
    assert_eq!(
        write_str(&Value::Bytes(vec![1, 2, 3, 4, 5]), Format::Ascii),
        "<01020304 05>\n"
    );
    assert_eq!(write_str(&Vec::<u32>::new(), Format::Ascii), "()\n");
    // comments start with `//` and `/*`
    assert_eq!(
        write_str(&vec!["//x", "/*x", "a//b", "/x"], Format::Ascii),
        "(\n\t\"//x\",\n\t\"/*x\",\n\ta//b,\n\t/x,\n)\n"
    );
    let value: Vec<String> = deser_plist::from_slice(b"(\"//x\", \"/*x\", a//b, /x)").unwrap();
    assert_eq!(value, ["//x", "/*x", "a//b", "/x"]);
}

#[test]
fn test_binary() {
    // `plutil -convert binary1` writes exactly the same
    let value = map! {
        "a" => 1u64,
        "b" => "x",
        "c" => array![1u64, "x", true],
    };
    let expected = "\
        62706c6973743030 d3010203040506 516151625163 1001 5178 a3040507 09 \
        080f111315171 91d 000000000000 01 01 0000000000000008 0000000000000000 \
        000000000000001e"
        .replace(' ', "");
    let bytes = write(&value, Format::Binary);
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    assert_eq!(hex, expected);
    assert_eq!(parse(&bytes), value);

    // top level scalars work too
    let mut expected = b"bplist00\x09\x08".to_vec();
    expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 1]);
    expected.extend_from_slice(&1u64.to_be_bytes());
    expected.extend_from_slice(&0u64.to_be_bytes());
    expected.extend_from_slice(&9u64.to_be_bytes());
    assert_eq!(write(&true, Format::Binary), expected);
}

#[test]
fn test_binary_encodings() {
    // integers and lengths of all sizes and strings that need UTF-16
    let long = "x".repeat(300);
    let value = array![
        0u64,
        255u64,
        256u64,
        65536u64,
        u32::MAX as u64 + 1,
        u64::MAX,
        -1i64,
        i64::MIN,
        Value::I128(i128::MIN),
        Value::I128(i128::MAX),
        long.as_str(),
        "\u{e4}\u{1F600}",
        Value::Bytes(vec![7; 20]),
        Value::Array((0..300u64).map(Value::U64).collect()),
    ];
    assert_eq!(parse(&write(&value, Format::Binary)), value);

    // f32 is written as a 4 byte real
    let bytes = write(&1.5f32, Format::Binary);
    assert_eq!(&bytes[8..13], b"\x22\x3f\xc0\x00\x00");
    assert_eq!(deser_plist::from_slice::<f32>(&bytes).unwrap(), 1.5);

    // equal values are only stored once
    let value = array!["same", "same", "same", map! { "same" => "same" }];
    let bytes = write(&value, Format::Binary);
    assert_eq!(bytes.windows(4).filter(|w| w == b"same").count(), 1);
    assert_eq!(parse(&bytes), value);
}

#[test]
fn test_nulls() {
    // entries with null values are skipped
    let value = BTreeMap::from([("a", Some(1)), ("b", None)]);
    for format in [Format::Xml, Format::Binary, Format::Ascii] {
        let bytes = write(&value, format);
        assert_eq!(parse(&bytes).get("a"), &parse(&write(&1u32, format)));
        assert_eq!(
            deser_plist::from_slice::<BTreeMap<String, Option<u32>>>(&bytes)
                .unwrap()
                .len(),
            1
        );
    }
    let config = SerializerConfig::new();
    let err = config.to_vec(&vec![Some(1), None]).unwrap_err();
    assert!(
        err.to_string()
            .contains("cannot hold null values in arrays"),
        "{err}"
    );
    let err = config.to_vec(&None::<u32>).unwrap_err();
    assert!(err.to_string().contains("cannot hold null values"), "{err}");
    let err = config.to_vec(&()).unwrap_err();
    assert!(err.to_string().contains("cannot hold null values"), "{err}");
}

#[test]
fn test_keys() {
    let value = BTreeMap::from([(1, "a"), (2, "b")]);
    let xml = deser_plist::to_string(&value).unwrap();
    assert!(xml.contains("<key>1</key>"), "{xml}");
    let back: BTreeMap<u32, String> = deser_plist::from_slice(xml.as_bytes()).unwrap();
    assert_eq!(back[&2], "b");

    let value = BTreeMap::from([(vec![1], "a")]);
    let err = deser_plist::to_vec(&value).unwrap_err();
    assert!(
        err.to_string()
            .contains("keys of property lists must be strings"),
        "{err}"
    );
}

#[test]
fn test_integers() {
    assert_eq!(
        deser_plist::to_string(&vec![u64::MAX]).unwrap(),
        xml_doc("<array>\n\t<integer>18446744073709551615</integer>\n</array>\n")
    );
    let err = deser_plist::to_vec(&(u64::MAX as i128 + 1)).unwrap_err();
    assert!(err.to_string().contains("out of range for XML"), "{err}");
    // binary property lists hold 128 bits
    let value = u64::MAX as i128 + 1;
    let bytes = write(&value, Format::Binary);
    assert_eq!(deser_plist::from_slice::<i128>(&bytes).unwrap(), value);
    let err = SerializerConfig::builder()
        .format(Format::Binary)
        .build()
        .to_vec(&u128::MAX)
        .unwrap_err();
    assert!(err.to_string().contains("out of range"), "{err}");
}

#[test]
fn test_reals() {
    assert_eq!(
        deser_plist::to_string(&vec![
            1.0,
            0.1f64,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN
        ])
        .unwrap(),
        xml_doc(
            "<array>
\t<real>1.0</real>
\t<real>0.1</real>
\t<real>+infinity</real>
\t<real>-infinity</real>
\t<real>nan</real>
</array>
"
        )
    );
    let back: Vec<f64> = deser_plist::from_slice(
        deser_plist::to_string(&vec![0.1f64, f64::INFINITY])
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(back, [0.1, f64::INFINITY]);
}

#[test]
fn test_data() {
    let data = Value::Bytes((0..=255).collect());
    let xml = deser_plist::to_string(&data).unwrap();
    // lines of 76 characters at the top level, like Core Foundation
    let lines: Vec<_> = xml.lines().skip(4).take(5).collect();
    assert_eq!(
        lines.iter().map(|l| l.len()).collect::<Vec<_>>(),
        [76, 76, 76, 76, 40]
    );
    assert_eq!(parse(xml.as_bytes()), data);
    // nested data is indented and the lines get shorter
    let xml = deser_plist::to_string(&array![array![Value::Bytes(vec![0; 60])]]).unwrap();
    let expected = format!(
        "\t\t<data>\n\t\t{}\n\t\t{}\n\t\t</data>\n",
        "A".repeat(60),
        "A".repeat(20)
    );
    assert!(xml.contains(&expected), "{xml}");
    assert_eq!(
        deser_plist::to_string(&Value::Bytes(vec![])).unwrap(),
        xml_doc("<data>\n</data>\n")
    );
    // `Vec<u8>` is data
    let bytes = SerializerConfig::builder()
        .format(Format::Binary)
        .build()
        .to_vec(&vec![1u8, 2])
        .unwrap();
    assert_eq!(parse(&bytes), Value::Bytes(vec![1, 2]));
}

#[test]
fn test_dates() {
    let time = UNIX_EPOCH + Duration::new(1_718_824_965, 250_000_000);
    // XML dates have no fraction
    assert_eq!(
        deser_plist::to_string(&time).unwrap(),
        xml_doc("<date>2024-06-19T19:22:45Z</date>\n")
    );
    // binary dates keep it
    let bytes = write(&time, Format::Binary);
    assert_eq!(
        deser_plist::from_slice::<std::time::SystemTime>(&bytes).unwrap(),
        time
    );
    // OpenStep writes strings
    assert_eq!(write_str(&time, Format::Ascii), "2024-06-19T19:22:45.25Z\n");

    // offset date-times are dates, other date-times strings
    let datetime: Datetime = "2024-06-19T21:22:45+02:00".parse().unwrap();
    assert_eq!(
        deser_plist::to_string(&datetime).unwrap(),
        xml_doc("<date>2024-06-19T19:22:45Z</date>\n")
    );
    let date: Datetime = "2024-06-19".parse().unwrap();
    assert_eq!(
        deser_plist::to_string(&date).unwrap(),
        xml_doc("<string>2024-06-19</string>\n")
    );
    // dates before 2001
    let old = Timestamp {
        seconds: -1,
        nanosecond: 500_000_000,
    };
    assert_eq!(parse(&write(&old, Format::Binary)), Value::Date(old));
    // binary property lists hold dates as seconds in a float, XML property
    // lists also hold dates beyond that
    let far = Timestamp {
        seconds: i64::MAX,
        nanosecond: 0,
    };
    let err = SerializerConfig::builder()
        .format(Format::Binary)
        .build()
        .to_vec(&far)
        .unwrap_err();
    assert_eq!(err.message(), "date out of range for binary property lists");
    assert_eq!(parse(&write(&far, Format::Xml)), Value::Date(far));
}

#[test]
fn test_uids() {
    let value = array![Uid::new(1), Uid::new(300), Uid::new(u64::MAX)];
    for format in [Format::Xml, Format::Binary] {
        assert_eq!(parse(&write(&value, format)), value, "{format:?}");
    }
    assert_eq!(
        deser_plist::to_string(&Uid::new(1)).unwrap(),
        xml_doc("<dict>\n\t<key>CF$UID</key>\n\t<integer>1</integer>\n</dict>\n")
    );
    assert_eq!(write_str(&Uid::new(1), Format::Ascii), "{CF$UID = 1;}\n");
}

#[test]
fn test_other_extensions() {
    // other well-known types are written as their fallback
    let uuid: Uuid = "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap();
    assert_eq!(
        deser_plist::to_string(&uuid).unwrap(),
        xml_doc("<string>67e55044-10b1-426f-9247-bb680e5fe0c8</string>\n")
    );
}

#[test]
fn test_serializer() {
    let mut serializer =
        Serializer::with_config(SerializerConfig::builder().format(Format::Ascii).build());
    serializer.serialize(&1u32).unwrap();
    assert!(serializer.serialize(&2u32).is_err());
    assert_eq!(serializer.finish(), b"1\n");

    let err = SerializerConfig::builder()
        .format(Format::Binary)
        .build()
        .to_string(&1u32)
        .unwrap_err();
    assert!(
        err.to_string().contains("cannot be written to strings"),
        "{err}"
    );
}

#[test]
fn test_deep_nesting() {
    // the indentation of the text formats grows quadratically with the
    // depth, they are tested with less
    for (format, depth) in [
        (Format::Binary, 100_000),
        (Format::Xml, 5000),
        (Format::Ascii, 5000),
    ] {
        let input = format!("{}{}", "(".repeat(depth), ")".repeat(depth));
        let value: deser::de::Recording = deser_plist::from_slice(input.as_bytes()).unwrap();
        let bytes = write(&value, format);
        let back: deser::de::Recording = deser_plist::from_slice(&bytes).unwrap();
        assert_eq!(back.events().count(), depth * 2);
    }
}
