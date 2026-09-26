use std::collections::BTreeMap;

use deser::adapters::bytes::{BytesFallback, BytesFormat, Hex, IntSeq};
use deser::ext::Datetime;
use deser::hints::{Compact, Expanded, Layout};
use deser::ser::{Layer, Next};
use deser::{Deserialize, Serialize};
use deser_yaml::style::{DoubleQuoted, Folded, Literal, Plain, ScalarStyle, SingleQuoted};
use deser_yaml::{
    DeserializerConfig, FlowPolicy, Indent, MultilineStyle, NullStyle, QuoteStyle,
    SerializerConfig, Tagged, Version, from_str, to_string,
};

use crate::common::Value;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Service {
    image: String,
    ports: Vec<u16>,
    command: Option<String>,
    labels: BTreeMap<String, String>,
    origin: Point,
    points: Vec<Point>,
    volumes: Vec<String>,
}

fn service() -> Service {
    Service {
        image: "nginx".into(),
        ports: vec![80, 443],
        command: None,
        labels: BTreeMap::from([("env".into(), "prod".into()), ("tier".into(), "web".into())]),
        origin: Point { x: 1, y: 2 },
        points: vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }],
        volumes: vec![],
    }
}

#[test]
fn test_block_layout() {
    let yaml = to_string(&service()).unwrap();
    assert_eq!(
        yaml,
        "\
image: nginx
ports:
  - 80
  - 443
command: null
labels:
  env: prod
  tier: web
origin:
  x: 1
  'y': 2
points:
  - x: 1
    'y': 2
  - x: 3
    'y': 4
volumes: []
"
    );
    assert_eq!(from_str::<Service>(&yaml).unwrap(), service());
}

#[test]
fn test_indentless_sequences() {
    const CONFIG: SerializerConfig = SerializerConfig::new()
        .indent_sequences(false)
        .indent(Indent::Spaces(4))
        .compat(Version::V1_2);
    let yaml = CONFIG.to_string(&service()).unwrap();
    assert_eq!(
        yaml,
        "\
image: nginx
ports:
- 80
- 443
command: null
labels:
    env: prod
    tier: web
origin:
    x: 1
    y: 2
points:
- x: 1
  y: 2
- x: 3
  y: 4
volumes: []
"
    );
    assert_eq!(from_str::<Service>(&yaml).unwrap(), service());
}

#[test]
fn test_single_line() {
    const LINE: SerializerConfig = SerializerConfig::new()
        .indent(Indent::None)
        .compat(Version::V1_2);
    let yaml = LINE.to_string(&service()).unwrap();
    assert_eq!(
        yaml,
        "{image: nginx, ports: [80, 443], command: null, labels: {env: prod, tier: web}, \
         origin: {x: 1, y: 2}, points: [{x: 1, y: 2}, {x: 3, y: 4}], volumes: []}\n"
    );
    assert_eq!(from_str::<Service>(&yaml).unwrap(), service());

    // scalar documents and strings with line breaks stay on the line
    assert_eq!(LINE.to_string(&"a\nb").unwrap(), "\"a\\nb\"\n");
    assert_eq!(LINE.to_string(&vec!["a\nb"]).unwrap(), "[\"a\\nb\"]\n");
    assert_eq!(LINE.to_string(&"x").unwrap(), "x\n");
    assert_eq!(LINE.to_string(&Vec::<u32>::new()).unwrap(), "[]\n");

    // the flow policy and expanded hints have no effect, bytes are not
    // wrapped
    #[derive(Serialize)]
    struct Hinted {
        #[deser(as = Expanded)]
        items: Vec<u32>,
        data: Vec<u8>,
    }
    let value = Hinted {
        items: vec![1, 2],
        data: vec![0; 100],
    };
    let yaml = LINE
        .flow(FlowPolicy::LeafIfFits(8))
        .to_string(&value)
        .unwrap();
    assert_eq!(yaml.lines().count(), 1, "{}", yaml);
    assert!(yaml.starts_with("{items: [1, 2], data: !!binary AAAA"));

    // tags and keys that are collections
    let value = Value::Map(vec![
        (
            Value::Seq(vec![1i64.into()]),
            Value::Tagged("!point".into(), Box::new(Value::Map(vec![]))),
        ),
        ("k".into(), Value::Null),
    ]);
    let yaml = LINE.null_style(NullStyle::Empty).to_string(&value).unwrap();
    assert_eq!(yaml, "{? [1]: !point {}, k: null}\n");
    assert_eq!(from_str::<Value>(&yaml).unwrap(), value);
}

#[test]
fn test_nested_collections() {
    let value = vec![vec![vec![1, 2], vec![]], vec![vec![3]]];
    assert_eq!(
        to_string(&value).unwrap(),
        "- - - 1\n    - 2\n  - []\n- - - 3\n"
    );
    let value = vec![BTreeMap::from([("a", vec![1])]), BTreeMap::new()];
    assert_eq!(to_string(&value).unwrap(), "- a:\n    - 1\n- {}\n");
    assert_eq!(to_string(&Vec::<u32>::new()).unwrap(), "[]\n");
    assert_eq!(to_string(&BTreeMap::<u32, u32>::new()).unwrap(), "{}\n");
}

#[test]
fn test_keys() {
    let long = "x".repeat(1100);
    let value = Value::Map(vec![
        (Value::Int(1), "int".into()),
        (Value::Bool(true), "bool".into()),
        (Value::Null, "null".into()),
        ("a: b".into(), "quoted".into()),
        ("line\nbreak".into(), "multi".into()),
        (Value::Seq(vec![1i64.into(), 2i64.into()]), "seq".into()),
        (Value::Map(vec![]), "empty".into()),
        (long.as_str().into(), "long".into()),
    ]);
    let yaml = to_string(&value).unwrap();
    assert_eq!(
        yaml,
        format!(
            "1: int\ntrue: bool\nnull: 'null'\n'a: b': quoted\n\"line\\nbreak\": multi\n\
             ? - 1\n  - 2\n: seq\n? {{}}\n: empty\n? {}\n: long\n",
            long
        )
    );
    assert_eq!(from_str::<Value>(&yaml).unwrap(), value);
}

#[test]
fn test_quoting() {
    let strings = [
        "plain", "yes", "true", "1.5", "0777", "a: b", "", " x", "it's", "tab\t",
    ];
    assert_eq!(
        to_string(&strings).unwrap(),
        "- plain\n- 'yes'\n- 'true'\n- '1.5'\n- '0777'\n- 'a: b'\n- ''\n- ' x'\n- it's\n- \"tab\\t\"\n"
    );
    const V1_2: SerializerConfig = SerializerConfig::new().compat(Version::V1_2);
    assert_eq!(
        V1_2.to_string(&["yes", "1_000", "0777"]).unwrap(),
        "- yes\n- 1_000\n- '0777'\n"
    );
    const DOUBLE: SerializerConfig = SerializerConfig::new().quote_style(QuoteStyle::Double);
    assert_eq!(
        DOUBLE.to_string(&["plain", "yes"]).unwrap(),
        "- plain\n- \"yes\"\n"
    );
    const ALL: SerializerConfig = SerializerConfig::new().quote_all(true);
    assert_eq!(
        ALL.to_string(&["plain", "a\nb"]).unwrap(),
        "- 'plain'\n- \"a\\nb\"\n"
    );
    assert_eq!(to_string(&'x').unwrap(), "x\n");
    assert_eq!(to_string(&"\u{7}\u{2028}").unwrap(), "\"\\a\\L\"\n");
}

#[test]
fn test_block_scalars() {
    assert_eq!(to_string(&"a\nb\n").unwrap(), "|\n  a\n  b\n");
    assert_eq!(to_string(&"a\nb").unwrap(), "|-\n  a\n  b\n");
    assert_eq!(to_string(&"a\n\n").unwrap(), "|+\n  a\n\n");
    assert_eq!(to_string(&"\n").unwrap(), "|+\n\n");
    // leading spaces need an indentation indicator
    assert_eq!(to_string(&"  a\nb").unwrap(), "|2-\n    a\n  b\n");
    assert_eq!(
        to_string(&BTreeMap::from([("k", "  a\nb\n")])).unwrap(),
        "k: |2\n    a\n  b\n"
    );
    assert_eq!(to_string(&vec!["  a\nb"]).unwrap(), "- |2-\n    a\n  b\n");
    // carriage returns cannot be in block scalars
    assert_eq!(to_string(&"a\r\nb").unwrap(), "\"a\\r\\nb\"\n");
    const QUOTED: SerializerConfig = SerializerConfig::new().multiline(MultilineStyle::Quoted);
    assert_eq!(QUOTED.to_string(&"a\nb").unwrap(), "\"a\\nb\"\n");

    // everything reads back
    for value in [
        "a\nb",
        "a\n\n\n",
        "\n\n",
        " \n",
        "  a\n b\n",
        "a\n  \n",
        "#a\n- b\n",
        "a\n---\n",
    ] {
        for config in [
            SerializerConfig::new(),
            SerializerConfig::new().indent(Indent::Spaces(4)),
            SerializerConfig::new()
                .indent_sequences(false)
                .indent(Indent::Spaces(1)),
            SerializerConfig::new().indent(Indent::None),
        ] {
            for doc in [
                Value::from(value),
                Value::Seq(vec![value.into(), "x".into()]),
                Value::Map(vec![("k".into(), value.into()), ("z".into(), "x".into())]),
                Value::Seq(vec![Value::Map(vec![("k".into(), value.into())])]),
            ] {
                let yaml = config.to_string(&doc).unwrap();
                assert_eq!(
                    from_str::<Value>(&yaml).unwrap(),
                    doc,
                    "{:?}\n{}",
                    value,
                    yaml
                );
            }
        }
    }
}

#[test]
fn test_null_styles() {
    let value = (None::<u32>, BTreeMap::from([("a", None::<u32>)]));
    assert_eq!(to_string(&value).unwrap(), "- null\n- a: null\n");
    const TILDE: SerializerConfig = SerializerConfig::new().null_style(NullStyle::Tilde);
    assert_eq!(TILDE.to_string(&value).unwrap(), "- ~\n- a: ~\n");
    const EMPTY: SerializerConfig = SerializerConfig::new().null_style(NullStyle::Empty);
    assert_eq!(EMPTY.to_string(&value).unwrap(), "-\n- a:\n");
    assert_eq!(EMPTY.to_string(&()).unwrap(), "null\n");
    let parsed: (Option<u32>, BTreeMap<String, Option<u32>>) =
        from_str(&EMPTY.to_string(&value).unwrap()).unwrap();
    assert_eq!(parsed, (None, BTreeMap::from([("a".into(), None)])));
}

#[test]
fn test_numbers() {
    let value = (
        1u64,
        -1i64,
        1.5f64,
        1e20,
        f64::INFINITY,
        u128::MAX,
        i128::MIN,
    );
    let yaml = to_string(&value).unwrap();
    assert_eq!(
        yaml,
        format!(
            "- 1\n- -1\n- 1.5\n- 1.0e+20\n- .inf\n- {}\n- {}\n",
            u128::MAX,
            i128::MIN
        )
    );
    assert_eq!(
        from_str::<(u64, i64, f64, f64, f64, u128, i128)>(&yaml).unwrap(),
        value
    );
    // floats read back exactly with both versions
    let mut state = 0x2545f4914f6cdd1du64;
    for _ in 0..1000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let value = f64::from_bits(state);
        let yaml = to_string(&value).unwrap();
        for version in [Version::V1_1, Version::V1_2] {
            let parsed: f64 = DeserializerConfig::new()
                .version(version)
                .from_str(&yaml)
                .unwrap();
            assert!(parsed == value || value.is_nan(), "{} {:?}", yaml, version);
        }
    }
    // f32 is written as the f64 it widens to
    assert_eq!(
        from_str::<f32>(&to_string(&0.1f32).unwrap()).unwrap(),
        0.1f32
    );
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Blob {
    data: Vec<u8>,
    #[deser(as = BytesFallback<Hex>)]
    digest: Vec<u8>,
    #[deser(as = BytesFallback<IntSeq>)]
    legacy: Vec<u8>,
}

#[test]
fn test_bytes() {
    let blob = Blob {
        data: vec![1, 255],
        digest: vec![2, 3],
        legacy: vec![4],
    };
    // native bytes by default, also for fallbacks
    let yaml = to_string(&blob).unwrap();
    assert_eq!(
        yaml,
        "data: !!binary Af8=\ndigest: !!binary AgM=\nlegacy: !!binary BA==\n"
    );
    assert_eq!(from_str::<Blob>(&yaml).unwrap(), blob);
    assert_eq!(to_string(&Vec::<u8>::new()).unwrap(), "!!binary \"\"\n");
    assert_eq!(from_str::<Vec<u8>>("!!binary \"\"").unwrap(), b"");
    // long values are wrapped
    let long = (0..=255u8).collect::<Vec<_>>();
    let yaml = to_string(&long).unwrap();
    assert!(yaml.starts_with("!!binary |\n  AAECAwQF"), "{}", yaml);
    assert!(yaml.lines().all(|line| line.len() <= 78), "{}", yaml);
    assert_eq!(from_str::<Vec<u8>>(&yaml).unwrap(), long);

    // without native bytes the fallbacks and the configured format apply
    const HEX: SerializerConfig = SerializerConfig::new()
        .binary(false)
        .bytes(BytesFormat::encoded::<Hex>());
    let yaml = HEX.to_string(&blob).unwrap();
    assert_eq!(yaml, "data: 01ff\ndigest: '0203'\nlegacy:\n  - 4\n");
    const HEX_DE: DeserializerConfig =
        DeserializerConfig::new().bytes(BytesFormat::encoded::<Hex>());
    assert_eq!(HEX_DE.from_str::<Blob>(&yaml).unwrap(), blob);
    const SEQ: SerializerConfig = SerializerConfig::new()
        .binary(false)
        .bytes(BytesFormat::SEQ);
    assert_eq!(SEQ.to_string(&vec![1u8, 2]).unwrap(), "- 1\n- 2\n");
}

#[test]
fn test_tags() {
    let value = vec![
        Tagged::new("!color", Value::from("red")),
        Tagged::new("!point", Value::Map(vec![("x".into(), 1i64.into())])),
        Tagged::new("!empty", Value::Seq(vec![])),
        Tagged::new(
            "tag:yaml.org,2002:set",
            Value::Map(vec![("a".into(), Value::Null)]),
        ),
        Tagged::new("tag:example.com,2000:thing", Value::from("x")),
        Tagged::untagged(Value::from("plain")),
    ];
    let yaml = to_string(&value).unwrap();
    assert_eq!(
        yaml,
        "\
- !color red
- !point
  x: 1
- !empty []
- !!set
  a: null
- !<tag:example.com,2000:thing> x
- plain
"
    );
    let parsed: Vec<Tagged<Value>> = from_str(&yaml).unwrap();
    assert_eq!(parsed, value);

    // tags (and flow collections) survive a recording
    let input = "a: !foo {x: 1}\nb: !bar [1, !baz 2]\n";
    let recording: deser::de::Recording = from_str(input).unwrap();
    assert_eq!(
        to_string(&recording).unwrap(),
        // the custom tag makes the scalar a string, it's quoted
        "a: !foo {x: 1}\nb: !bar [1, !baz '2']\n"
    );
}

#[test]
fn test_datetimes() {
    let date: Datetime = "2001-12-14".parse().unwrap();
    let datetime: Datetime = "2001-12-14T21:59:43.1-05:00".parse().unwrap();
    let local: Datetime = "2001-12-14T21:59:43".parse().unwrap();
    let value = (date, datetime, local);
    let yaml = to_string(&value).unwrap();
    assert_eq!(
        yaml,
        "- 2001-12-14\n- 2001-12-14T21:59:43.1-05:00\n- '2001-12-14T21:59:43'\n"
    );
    assert_eq!(
        from_str::<(Datetime, Datetime, Datetime)>(&yaml).unwrap(),
        value
    );
    const TAGGED: SerializerConfig = SerializerConfig::new().timestamp_tag(true);
    assert_eq!(TAGGED.to_string(&date).unwrap(), "!!timestamp 2001-12-14\n");
    assert_eq!(
        from_str::<Datetime>("!!timestamp 2001-12-14").unwrap(),
        date
    );
}

#[test]
fn test_documents() {
    let mut serializer = deser_yaml::Serializer::new();
    serializer.serialize(&"a\n\n").unwrap();
    serializer.serialize(&vec![1]).unwrap();
    serializer.serialize(&()).unwrap();
    let yaml = serializer.finish();
    assert_eq!(yaml, "|+\n  a\n\n---\n- 1\n---\nnull\n");
    let docs = deser_yaml::Deserializer::from_str(&yaml)
        .iter::<Value>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        docs,
        [
            Value::from("a\n\n"),
            Value::Seq(vec![1i64.into()]),
            Value::Null
        ]
    );

    const START: SerializerConfig = SerializerConfig::new().document_start(true);
    assert_eq!(START.to_string(&1).unwrap(), "---\n1\n");
    const DIRECTIVE: SerializerConfig = SerializerConfig::new().version_directive(true);
    assert_eq!(DIRECTIVE.to_string(&1).unwrap(), "%YAML 1.2\n---\n1\n");

    // directives of later documents follow the end of the previous one
    let mut serializer = deser_yaml::Serializer::with_config(&DIRECTIVE);
    serializer.serialize(&"a").unwrap();
    serializer.serialize(&"b").unwrap();
    let yaml = serializer.finish();
    assert_eq!(yaml, "%YAML 1.2\n---\na\n...\n%YAML 1.2\n---\nb\n");
    let docs = deser_yaml::Deserializer::from_str(&yaml)
        .iter::<String>()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(docs, ["a", "b"]);
}

/// Strings built from pieces that have a meaning in YAML must read back as
/// the same string as value, key and sequence item with YAML 1.1 and 1.2
/// readers.
#[test]
fn test_quoting_fuzz() {
    let pieces = [
        "",
        "a",
        " ",
        "-",
        "?",
        ":",
        ",",
        "[",
        "]",
        "{",
        "}",
        "#",
        "&",
        "*",
        "!",
        "|",
        ">",
        "'",
        "\"",
        "%",
        "@",
        "`",
        "\t",
        "\n",
        "\r",
        "\\",
        ": ",
        " #",
        "---",
        "...",
        "<<",
        "=",
        "~",
        "null",
        "Null",
        "true",
        "yes",
        "No",
        "on",
        "OFF",
        "y",
        "n",
        "0",
        "1",
        "-1",
        "+1",
        "0x1F",
        "0o17",
        "0b10",
        "017",
        "1_0",
        "1:30",
        "1.5",
        ".5",
        "1e3",
        "1.0e+3",
        ".inf",
        "-.Inf",
        ".NaN",
        "2001-12-14",
        "2001-12-14 21:59:43",
        "\u{85}",
        "\u{a0}",
        "\u{2028}",
        "\u{feff}",
        "\u{1}",
        "\u{7f}",
        "é",
        "😀",
    ];
    let mut strings = Vec::new();
    for a in pieces {
        for b in ["", "a", " ", ":", "#", "\n", "1"] {
            strings.push(format!("{}{}", a, b));
            strings.push(format!("{}{}", b, a));
        }
    }
    let configs = [
        SerializerConfig::new(),
        SerializerConfig::new().compat(Version::V1_2),
        SerializerConfig::new().quote_style(QuoteStyle::Double),
        SerializerConfig::new().multiline(MultilineStyle::Quoted),
        SerializerConfig::new()
            .flow(FlowPolicy::LeafIfFits(200))
            .fold_width(Some(2)),
    ];
    let styles = [
        None,
        Some(ScalarStyle::Plain),
        Some(ScalarStyle::SingleQuoted),
        Some(ScalarStyle::DoubleQuoted),
        Some(ScalarStyle::Literal),
        Some(ScalarStyle::Folded),
    ];
    for (config, style) in configs
        .iter()
        .flat_map(|config| styles.iter().map(move |style| (config, *style)))
    {
        for s in &strings {
            let doc = Value::Seq(vec![
                Value::from(s.as_str()),
                Value::Map(vec![(Value::from(s.as_str()), Value::from(s.as_str()))]),
            ]);
            let yaml = config
                .to_string_with(&doc, |driver| {
                    if let Some(style) = style {
                        driver.push_layer(ForceStyle(style));
                    }
                })
                .unwrap();
            let versions: &[Version] = if config.to_string(&"yes").unwrap() == "yes\n" {
                &[Version::V1_2]
            } else {
                &[Version::V1_1, Version::V1_2]
            };
            for &version in versions {
                let parsed = DeserializerConfig::new()
                    .version(version)
                    .merge_keys(false)
                    .from_str::<Value>(&yaml);
                assert_eq!(
                    parsed.as_ref().ok(),
                    Some(&doc),
                    "{:?} {:?}\n{}",
                    s,
                    version,
                    yaml
                );
            }
        }
    }
}

/// Requests a style for all strings.
struct ForceStyle(ScalarStyle);

impl Layer for ForceStyle {
    fn event(&mut self, event: deser::Event<'_>, next: &mut Next<'_>) -> Result<(), deser::Error> {
        self.0.set(next.state_mut());
        next.emit(event)
    }
}

#[test]
fn test_layout_hints() {
    #[derive(Serialize)]
    struct Config {
        #[deser(as = Compact)]
        point: Point,
        #[deser(as = Compact)]
        ports: Vec<u16>,
        #[deser(as = Compact)]
        matrix: Vec<Vec<u16>>,
        #[deser(as = Expanded)]
        tags: Vec<String>,
        #[deser(as = Compact)]
        empty: Vec<u16>,
        other: Vec<u16>,
    }

    let config = Config {
        point: Point { x: 1, y: 2 },
        ports: vec![80, 443],
        matrix: vec![vec![1, 2], vec![]],
        tags: vec!["a".into(), "b, c".into()],
        empty: vec![],
        other: vec![3],
    };
    const V1_2: SerializerConfig = SerializerConfig::new().compat(Version::V1_2);
    let yaml = V1_2.to_string(&config).unwrap();
    assert_eq!(
        yaml,
        "\
point: {x: 1, y: 2}
ports: [80, 443]
matrix: [[1, 2], []]
tags:
  - a
  - b, c
empty: []
other:
  - 3
"
    );

    // the leaf policy writes collections of scalars in flow style if they fit
    const LEAF: SerializerConfig = SerializerConfig::new()
        .compat(Version::V1_2)
        .flow(FlowPolicy::LeafIfFits(20));
    let yaml = LEAF.to_string(&config).unwrap();
    assert_eq!(
        yaml,
        "\
point: {x: 1, y: 2}
ports: [80, 443]
matrix: [[1, 2], []]
tags:
  - a
  - b, c
empty: []
other: [3]
"
    );
    let value = BTreeMap::from([
        ("short", vec!["a", "b"]),
        ("long", vec!["aaaaaaaaaa", "bbbbbbbbbb"]),
        ("quoted", vec!["a,b", "c"]),
    ]);
    assert_eq!(
        LEAF.to_string(&value).unwrap(),
        "long:\n  - aaaaaaaaaa\n  - bbbbbbbbbb\nquoted: ['a,b', c]\nshort: [a, b]\n"
    );

    // keys, tags, nulls and multi-line strings in flow collections
    let value = Value::Tagged(
        "!t".into(),
        Box::new(Value::Map(vec![
            (Value::Seq(vec![1i64.into()]), Value::Null),
            ("k".into(), "a\nb".into()),
            (
                Value::Null,
                Value::Tagged("!u".into(), Box::new("x".into())),
            ),
        ])),
    );
    const EMPTY: SerializerConfig = SerializerConfig::new()
        .null_style(NullStyle::Empty)
        .flow(FlowPolicy::LeafIfFits(80));
    let yaml = to_string_compact(&EMPTY, &value);
    assert_eq!(yaml, "!t {? [1]: null, k: \"a\\nb\", null: !u x}\n");
    assert_eq!(from_str::<Value>(&yaml).unwrap(), value);

    // layers can set the layout by path, here for the root
    struct CompactRoot;
    impl Layer for CompactRoot {
        fn event(
            &mut self,
            event: deser::Event<'_>,
            next: &mut Next<'_>,
        ) -> Result<(), deser::Error> {
            // the start event of the root is at depth 1
            if next.state().depth() == 1
                && matches!(event, deser::Event::MapStart(_) | deser::Event::SeqStart(_))
            {
                Layout::Compact.set(next.state_mut());
            }
            next.emit(event)
        }
    }
    let yaml = SerializerConfig::new()
        .to_string_with(&service(), |driver| driver.push_layer(CompactRoot))
        .unwrap();
    assert!(
        yaml.starts_with("{image: nginx, ports: [80, 443], "),
        "{}",
        yaml
    );
    assert_eq!(from_str::<Service>(&yaml).unwrap(), service());
}

/// Serializes with the root compact.
fn to_string_compact(config: &SerializerConfig, value: &Value) -> String {
    struct CompactRoot;
    impl Layer for CompactRoot {
        fn event(
            &mut self,
            event: deser::Event<'_>,
            next: &mut Next<'_>,
        ) -> Result<(), deser::Error> {
            // the start event of the root is at depth 1
            if next.state().depth() == 1
                && matches!(event, deser::Event::MapStart(_) | deser::Event::SeqStart(_))
            {
                Layout::Compact.set(next.state_mut());
            }
            next.emit(event)
        }
    }
    config
        .to_string_with(value, |driver| driver.push_layer(CompactRoot))
        .unwrap()
}

#[test]
fn test_scalar_styles() {
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Strings {
        #[deser(as = Plain)]
        plain: String,
        #[deser(as = Plain)]
        not_plain: String,
        #[deser(as = SingleQuoted)]
        single: String,
        #[deser(as = SingleQuoted)]
        not_single: String,
        #[deser(as = DoubleQuoted)]
        double: String,
        #[deser(as = Literal)]
        literal: String,
        #[deser(as = Literal)]
        not_literal: String,
        #[deser(as = Folded)]
        folded: String,
        #[deser(as = Folded)]
        not_folded: String,
        #[deser(as = Vec<DoubleQuoted>)]
        list: Vec<String>,
    }

    let value = Strings {
        plain: "a".into(),
        not_plain: "yes".into(),
        single: "a".into(),
        not_single: "a\tb".into(),
        double: "a".into(),
        literal: "a b".into(),
        not_literal: "a\rb".into(),
        folded: "one two three four five six seven eight nine ten eleven twelve".into(),
        not_folded: " leading\nspace".into(),
        list: vec!["a".into(), "b".into()],
    };
    const WIDTH: SerializerConfig = SerializerConfig::new().fold_width(Some(30));
    let yaml = WIDTH.to_string(&value).unwrap();
    assert_eq!(
        yaml,
        "\
plain: a
not_plain: 'yes'
single: 'a'
not_single: \"a\\tb\"
double: \"a\"
literal: |-
  a b
not_literal: \"a\\rb\"
folded: >-
  one two three four five six
  seven eight nine ten eleven
  twelve
not_folded: |2-
   leading
  space
list:
  - \"a\"
  - \"b\"
"
    );
    assert_eq!(from_str::<Strings>(&yaml).unwrap(), value);
}

#[test]
fn test_folding() {
    const FOLD: SerializerConfig = SerializerConfig::new().fold_width(Some(20));
    let long = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod";
    let yaml = FOLD.to_string(&BTreeMap::from([("text", long)])).unwrap();
    assert_eq!(
        yaml,
        "text: >-\n  Lorem ipsum dolor\n  sit amet,\n  consectetur\n  adipiscing elit, sed\n  do eiusmod\n"
    );
    assert_eq!(
        from_str::<BTreeMap<String, String>>(&yaml).unwrap()["text"],
        long
    );
    // strings without spaces to fold at are not folded
    let word = "x".repeat(50);
    assert_eq!(FOLD.to_string(&word).unwrap(), format!("{}\n", word));
    // multi-line strings with the hint keep their line breaks
    for value in [
        "a b c d e f g h i j k l m n o p q r s t u v w x y z\nsecond line\n\nthird\n",
        "a  b\n\n\nc",
        "trailing\n\n",
        "x",
    ] {
        let yaml = SerializerConfig::new()
            .fold_width(Some(10))
            .to_string_with(&value, |driver| {
                driver.push_layer(ForceStyle(ScalarStyle::Folded))
            })
            .unwrap();
        assert!(yaml.starts_with('>'), "{}", yaml);
        assert_eq!(from_str::<String>(&yaml).unwrap(), value, "{}", yaml);
    }
}
