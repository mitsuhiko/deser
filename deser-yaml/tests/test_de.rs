use std::collections::{BTreeMap, HashMap};

use deser::{Deserialize, ErrorKind};
use deser_yaml::{Deserializer, DeserializerConfig, Tagged, Version, from_slice, from_str};

mod common;

use common::Value;

#[derive(Deserialize, Debug, PartialEq)]
struct Config {
    name: String,
    port: u16,
    debug: bool,
    ratio: f64,
    tags: Vec<String>,
    owner: Option<String>,
    limits: HashMap<String, u32>,
}

#[test]
fn test_struct() {
    let config: Config = from_str(
        "
# a comment
name: web
port: 8080
debug: false
ratio: 0.5
tags:
  - a
  - 'b'
  - \"c\"
owner: ~
limits: {cpu: 2, memory: 512}
",
    )
    .unwrap();
    assert_eq!(
        config,
        Config {
            name: "web".into(),
            port: 8080,
            debug: false,
            ratio: 0.5,
            tags: vec!["a".into(), "b".into(), "c".into()],
            owner: None,
            limits: [("cpu".into(), 2), ("memory".into(), 512)].into(),
        }
    );
}

#[test]
fn test_scalars() {
    assert_eq!(from_str::<u64>("42").unwrap(), 42);
    assert_eq!(from_str::<i64>("-42").unwrap(), -42);
    assert_eq!(from_str::<u32>("0x2a").unwrap(), 42);
    assert_eq!(from_str::<u32>("0o52").unwrap(), 42);
    assert_eq!(from_str::<f64>("1e3").unwrap(), 1000.0);
    assert_eq!(from_str::<f64>("-.inf").unwrap(), f64::NEG_INFINITY);
    assert!(from_str::<f64>(".nan").unwrap().is_nan());
    assert!(from_str::<bool>("True").unwrap());
    assert_eq!(from_str::<Option<u32>>("null").unwrap(), None);
    assert_eq!(from_str::<String>("hello world").unwrap(), "hello world");
    assert_eq!(from_str::<String>("'42'").unwrap(), "42");
    assert_eq!(
        from_str::<String>("\"a\\tb\\u00e4\"").unwrap(),
        "a\tb\u{e4}"
    );
    assert_eq!(from_str::<String>("|\n  a\n  b\n").unwrap(), "a\nb\n");
    assert_eq!(from_str::<String>(">-\n  a\n  b\n").unwrap(), "a b");
    // a plain scalar that looks like a number is a number
    assert!(from_str::<String>("42").is_err());
}

#[test]
fn test_wide_integers() {
    assert_eq!(
        from_str::<u128>("340282366920938463463374607431768211455").unwrap(),
        u128::MAX
    );
    assert_eq!(
        from_str::<i128>("-170141183460469231731687303715884105728").unwrap(),
        i128::MIN
    );
}

#[test]
fn test_explicit_tags() {
    assert_eq!(from_str::<String>("!!str 42").unwrap(), "42");
    assert_eq!(from_str::<u32>("!!int '42'").unwrap(), 42);
    assert_eq!(from_str::<f64>("!!float 42").unwrap(), 42.0);
    assert_eq!(from_str::<String>("! 42").unwrap(), "42");
    assert_eq!(
        from_str::<Value>("!!binary aGVsbG8=").unwrap(),
        Value::Bytes(b"hello".to_vec())
    );
    let err = from_str::<Value>("!!int abc").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: invalid !!int value at line 1 column 1"
    );
    assert!(from_str::<Value>("!!map [1]").is_err());
    assert!(from_str::<Value>("!!str [1]").is_err());
    assert_eq!(from_str::<Vec<u32>>("!!seq [1]").unwrap(), [1]);
}

#[test]
fn test_timestamps() {
    use deser::ext::{Datetime, Timestamp};

    let value: Datetime = from_str("!!timestamp 2001-12-14 21:59:43.10 -5").unwrap();
    assert_eq!(value.to_string(), "2001-12-14T21:59:43.1-05:00");
    let value: Timestamp = from_str("!!timestamp 2001-12-15T02:59:43.1Z").unwrap();
    assert_eq!(value.to_string(), "2001-12-15T02:59:43.1Z");
    // timestamps without time zone are in UTC
    let value: Datetime = from_str("!!timestamp 2001-12-15 2:59:43").unwrap();
    assert_eq!(value.to_string(), "2001-12-15T02:59:43Z");
    let value: Datetime = from_str("!!timestamp 2002-12-14").unwrap();
    assert_eq!(value.to_string(), "2002-12-14");
    // other consumers get strings
    let value: String = from_str("!!timestamp 2001-12-14t21:59:43.10-05:00").unwrap();
    assert_eq!(value, "2001-12-14T21:59:43.1-05:00");
    // plain scalars are not timestamps
    let value: String = from_str("2001-12-14").unwrap();
    assert_eq!(value, "2001-12-14");
    assert!(from_str::<Value>("!!timestamp yesterday").is_err());
}

#[test]
fn test_custom_tags() {
    let value: Vec<Tagged<String>> = from_str("[!color red, blue, !!str green]").unwrap();
    assert_eq!(
        value,
        [
            Tagged::new("!color", "red".to_string()),
            Tagged::untagged("blue".to_string()),
            Tagged::untagged("green".to_string()),
        ]
    );

    // tags on collections, handles are resolved
    let value: Tagged<BTreeMap<String, u32>> =
        from_str("%TAG !e! tag:example.com,2000:\n--- !e!point {x: 1, y: 2}").unwrap();
    assert_eq!(value.tag.as_deref(), Some("tag:example.com,2000:point"));
    assert_eq!(value.value, [("x".into(), 1), ("y".into(), 2)].into());

    // unknown tags are transparent for types that do not care
    assert_eq!(from_str::<String>("!Ref name").unwrap(), "name");
    assert_eq!(from_str::<Vec<u32>>("!!set [1, 2]").unwrap(), [1, 2]);
    assert_eq!(
        from_str::<Value>("!!set {a, b}").unwrap(),
        Value::Tagged(
            "tag:yaml.org,2002:set".into(),
            Box::new(map! { "a" => (), "b" => () })
        )
    );
}

#[test]
fn test_complex_keys() {
    let value: Value = from_str("? [a, b]\n: c\n1: d\n{x: y}: e\n").unwrap();
    assert_eq!(
        value,
        Value::Map(vec![
            (seq!["a", "b"], "c".into()),
            (Value::Int(1), "d".into()),
            (map! { "x" => "y" }, "e".into()),
        ])
    );
    let value: BTreeMap<u32, String> = from_str("1: a\n2: b").unwrap();
    assert_eq!(value, [(1, "a".into()), (2, "b".into())].into());
}

#[test]
fn test_enums() {
    #[derive(Deserialize, Debug, PartialEq)]
    enum Shape {
        Empty,
        Circle(f64),
        Rect { w: u32, h: u32 },
    }
    let shapes: Vec<Shape> = from_str("- Empty\n- Circle: 1.5\n- Rect: {w: 1, h: 2}\n").unwrap();
    assert_eq!(
        shapes,
        [Shape::Empty, Shape::Circle(1.5), Shape::Rect { w: 1, h: 2 }]
    );
}

#[test]
fn test_aliases() {
    let value: Value = from_str(
        "
base: &base {a: 1, b: [x, y]}
copy: *base
scalar: &s hello
again: *s
",
    )
    .unwrap();
    let base = map! { "a" => 1, "b" => seq!["x", "y"] };
    assert_eq!(
        value,
        map! { "base" => base.clone(), "copy" => base, "scalar" => "hello", "again" => "hello" }
    );

    // aliases within anchored nodes
    let value: Value = from_str("- &a 1\n- &b [*a, *a]\n- *b\n").unwrap();
    assert_eq!(value, seq![1, seq![1, 1], seq![1, 1]]);

    // redefined anchors: an alias refers to the definition before it
    let value: Value = from_str("- &a 1\n- &b [*a]\n- &a 2\n- *b\n- *a\n").unwrap();
    assert_eq!(value, seq![1, seq![1], 2, seq![1], 2]);

    // aliases do not reach into other documents
    let mut de = Deserializer::from_str("--- &a 1\n--- *a\n");
    assert_eq!(de.deserialize::<u32>().unwrap(), 1);
    assert_eq!(
        de.deserialize::<u32>().unwrap_err().to_string(),
        "Unexpected: unknown anchor 'a' at line 2 column 5"
    );

    let err = from_str::<Value>("&a [*a]").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: recursive alias at line 1 column 5"
    );
}

#[test]
fn test_alias_limit() {
    // the "billion laughs" attack
    let mut input = String::from("a: &a [lol, lol, lol, lol, lol, lol, lol, lol, lol]\n");
    for (prev, name) in ["a", "b", "c", "d", "e", "f", "g", "h"]
        .iter()
        .zip(["b", "c", "d", "e", "f", "g", "h", "i"])
    {
        input.push_str(&format!(
            "{}: &{} [*{p}, *{p}, *{p}, *{p}, *{p}, *{p}, *{p}, *{p}, *{p}]\n",
            name,
            name,
            p = prev
        ));
    }
    let err = from_str::<Value>(&input).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: aliases expand to too many events"
    );

    let input = "a: &a [1, 2]\nb: [*a, *a]";
    let mut de =
        Deserializer::from_str_with_config(input, &DeserializerConfig::new().alias_limit(8));
    assert_eq!(
        de.deserialize::<Value>().unwrap(),
        map! { "a" => seq![1, 2], "b" => seq![seq![1, 2], seq![1, 2]] }
    );
    let mut de =
        Deserializer::from_str_with_config(input, &DeserializerConfig::new().alias_limit(7));
    assert!(de.deserialize::<Value>().is_err());
}

#[test]
fn test_max_depth() {
    let input = "[[[[1]]]]";
    assert!(
        DeserializerConfig::new()
            .max_depth(4)
            .from_str::<Value>(input)
            .is_ok()
    );
    let err = DeserializerConfig::new()
        .max_depth(3)
        .from_str::<Value>(input)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: recursion limit exceeded at line 1 column 4"
    );

    // nesting does not use the stack
    #[derive(Deserialize)]
    struct Node {
        name: String,
        child: Option<Box<Node>>,
    }
    let depth = 50_000;
    let input = "{name: x, child: ".repeat(depth) + "null" + &"}".repeat(depth);
    let mut node: Node = from_str(&input).unwrap();
    let mut count = 0;
    // avoid a recursive drop
    while let Some(child) = node.child.take() {
        assert_eq!(node.name, "x");
        node = *child;
        count += 1;
    }
    assert_eq!(count, depth - 1);

    // ignored values are not held in memory
    #[derive(Deserialize)]
    struct Simple {
        a: u32,
    }
    let input = format!(
        "a: 42
ignored: {}{}",
        "[".repeat(depth),
        "]".repeat(depth)
    );
    assert_eq!(from_str::<Simple>(&input).unwrap().a, 42);
}

#[test]
fn test_documents() {
    let mut de = Deserializer::from_str("--- 1\n---\n- 2\n...\n--- 3\n");
    assert!(!de.is_end());
    assert_eq!(de.deserialize::<Value>().unwrap(), Value::Int(1));
    assert_eq!(de.deserialize::<Value>().unwrap(), seq![2]);
    assert_eq!(de.deserialize::<Value>().unwrap(), Value::Int(3));
    assert!(de.is_end());
    de.end().unwrap();
    let err = de.deserialize::<Value>().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);

    let docs: Vec<u32> = Deserializer::from_str("1\n--- 2\n--- 3\n")
        .iter()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(docs, [1, 2, 3]);

    // an empty document is null
    let docs: Vec<Option<u32>> = Deserializer::from_str("---\n--- 1\n")
        .iter()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(docs, [None, Some(1)]);

    // from_str expects at most one document
    let err = from_str::<u32>("--- 1\n--- 2\n").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: expected a single document, found more"
    );
}

#[test]
fn test_empty_stream() {
    // a stream without documents is null
    assert_eq!(from_str::<Option<u32>>("").unwrap(), None);
    assert_eq!(from_str::<Option<u32>>("# just a comment\n").unwrap(), None);
    assert_eq!(from_str::<()>("").unwrap(), ());
    assert!(from_str::<u32>("").is_err());
    assert!(Deserializer::from_str("# nothing").is_end());
    assert_eq!(Deserializer::from_str("").iter::<Value>().count(), 0);
}

#[test]
fn test_error_recovery() {
    // a document that fails to deserialize is skipped
    let mut de = Deserializer::from_str("--- [1, 2]\n--- abc\n--- 3\n");
    assert!(de.deserialize::<u32>().is_err());
    assert!(de.deserialize::<u32>().is_err());
    assert_eq!(de.deserialize::<u32>().unwrap(), 3);
    assert!(de.is_end());

    // syntax errors end the stream
    let mut de = Deserializer::from_str("--- 1\n--- [\n--- 3\n");
    assert_eq!(de.deserialize::<u32>().unwrap(), 1);
    let err = de.deserialize::<Value>().unwrap_err();
    assert!(err.to_string().contains("syntax error"), "{}", err);
    assert!(de.deserialize::<Value>().is_err());

    // the iterator stops at the first error
    let rv: Vec<_> = Deserializer::from_str("--- 1\n--- x\n--- 3\n")
        .iter::<u32>()
        .collect();
    assert_eq!(rv.len(), 2);
    assert!(rv[1].is_err());
}

#[test]
fn test_syntax_errors() {
    let err = from_str::<Value>("a: b: c").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: syntax error: mapping values are not allowed in this context at line 1 column 5"
    );
    let err = from_str::<Value>("[1, 2").unwrap_err();
    assert!(err.to_string().contains("syntax error"), "{}", err);
    let mut de = Deserializer::from_str("- a\nb");
    assert!(!de.is_end());
    assert!(de.deserialize::<Value>().is_err());
}

#[test]
fn test_versions() {
    let input = "[yes, No, on, 0777, 0o777, 1_000, 1:30, 0b101, 3e3]";
    let value: Value = Deserializer::from_str(input).deserialize().unwrap();
    assert_eq!(
        value,
        seq![
            "yes", "No", "on", 777, 511, "1_000", "1:30", "0b101", 3000.0
        ]
    );
    let value: Value = DeserializerConfig::new()
        .version(Version::V1_1)
        .from_str(input)
        .unwrap();
    assert_eq!(
        value,
        seq![true, false, true, 511, "0o777", 1000, 90, 5, "3e3"]
    );

    // the directive overrides the configured version
    let mut de = Deserializer::from_str_with_config(
        "%YAML 1.1\n--- yes\n...\n%YAML 1.2\n--- yes\n--- yes\n",
        &DeserializerConfig::new().version(Version::V1_1),
    );
    assert_eq!(
        de.config().clone(),
        DeserializerConfig::new().version(Version::V1_1)
    );
    assert_eq!(de.deserialize::<Value>().unwrap(), Value::Bool(true));
    assert_eq!(de.deserialize::<Value>().unwrap(), "yes".into());
    assert_eq!(de.deserialize::<Value>().unwrap(), Value::Bool(true));
}

#[test]
fn test_from_slice() {
    assert_eq!(from_slice::<Vec<u32>>(b"[1, 2]").unwrap(), [1, 2]);
    let err = from_slice::<Value>(b"a: \xff").unwrap_err();
    assert_eq!(err.to_string(), "Unexpected: invalid UTF-8 at offset 3");
    // a byte order mark is skipped
    assert_eq!(from_slice::<u32>(b"\xef\xbb\xbf42").unwrap(), 42);
}

#[test]
fn test_borrowed_strings() {
    use std::borrow::Cow;

    use deser::State;
    use deser::de::{DeserializeDriver, Sink, SinkHandle};
    use deser::{Atom, Error};

    // records whether strings are borrowed from the input
    struct Borrowed(Vec<bool>);

    impl<'de> Sink<'de> for Borrowed {
        fn atom(&mut self, atom: Atom, _state: &mut State) -> Result<(), Error> {
            if let Atom::Str(s) = atom {
                self.0.push(matches!(s, Cow::Borrowed(_)));
            }
            Ok(())
        }

        fn seq(&mut self, _state: &mut State) -> Result<(), Error> {
            Ok(())
        }

        fn next_value(&mut self, _state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
            Ok(SinkHandle::to(self))
        }
    }

    let mut sink = Borrowed(Vec::new());
    {
        let mut driver = DeserializeDriver::from_sink(SinkHandle::to(&mut sink));
        Deserializer::from_str(
            "[plain words, 'quoted', \"double\", 'it''s', \"esc\\n\", multi\n line]",
        )
        .drive(&mut driver)
        .unwrap();
    }
    assert_eq!(sink.0, [true, true, true, false, false, false]);
}

#[test]
fn test_merge_keys() {
    // the example from the YAML 1.1 merge key specification: all maps are
    // the same
    let input = "
- &CENTER { x: 1, y: 2 }
- &LEFT { x: 0, y: 2 }
- &BIG { r: 10 }
- &SMALL { r: 1 }

# Explicit keys
- x: 1
  y: 2
  r: 10
  label: center/big

# Merge one map
- << : *CENTER
  r: 10
  label: center/big

# Merge multiple maps
- << : [ *CENTER, *BIG ]
  label: center/big

# Override
- << : [ *BIG, *LEFT, *SMALL ]
  x: 1
  label: center/big
";
    let value: Vec<BTreeMap<String, Value>> = from_str(input).unwrap();
    let expected: BTreeMap<String, Value> = [
        ("x".into(), Value::Int(1)),
        ("y".into(), Value::Int(2)),
        ("r".into(), Value::Int(10)),
        ("label".into(), "center/big".into()),
    ]
    .into();
    for map in &value[4..] {
        assert_eq!(map, &expected);
    }
}

#[test]
fn test_merge_key_semantics() {
    // merged entries come after the entries of the map, keys of the map win
    // even if they come after the merge key
    let value: Value = from_str("- &a {x: 1, y: 2}\n- {x: 0, <<: *a, z: 3, y: 0}\n").unwrap();
    assert_eq!(
        value,
        seq![
            map! { "x" => 1, "y" => 2 },
            map! { "x" => 0, "z" => 3, "y" => 0 },
        ]
    );

    // inline mappings, keys are compared by value
    let value: Value = from_str("<<: {1: a, b: c, '2': d}\n0x1: x\n2: y\n").unwrap();
    assert_eq!(
        value,
        Value::Map(vec![
            (Value::Int(1), "x".into()),
            (Value::Int(2), "y".into()),
            ("b".into(), "c".into()),
            ("2".into(), "d".into()),
        ])
    );

    // merges are applied recursively, earlier sources win
    let value: Value = from_str(
        "
- &a {a: 1, x: a}
- &b {<<: *a, b: 2, x: b}
- &c {c: 3, x: c}
- {<<: [*b, *c]}
- {<<: [*c, *b]}
",
    )
    .unwrap();
    let Value::Seq(items) = value else { panic!() };
    assert_eq!(items[1], map! { "b" => 2, "x" => "b", "a" => 1 });
    assert_eq!(items[3], map! { "b" => 2, "x" => "b", "a" => 1, "c" => 3 });
    assert_eq!(items[4], map! { "c" => 3, "x" => "c", "b" => 2, "a" => 1 });

    // an alias to a map with merge keys is merged as well
    let value: Value = from_str("a: &a {<<: {x: 1}, y: 2}\nb: *a\n").unwrap();
    assert_eq!(
        value,
        map! { "a" => map! { "y" => 2, "x" => 1 }, "b" => map! { "y" => 2, "x" => 1 } }
    );

    // nested maps with merge keys inside of merged maps
    let value: Value = from_str("a: &a {n: {<<: {x: 1}}}\nb: {<<: *a}\n").unwrap();
    assert_eq!(
        value,
        map! { "a" => map! { "n" => map! { "x" => 1 } }, "b" => map! { "n" => map! { "x" => 1 } } }
    );

    // anchors within merged inline maps can be used later
    let value: Value = from_str("a: {<<: &m {x: 1}}\nb: *m\n").unwrap();
    assert_eq!(
        value,
        map! { "a" => map! { "x" => 1 }, "b" => map! { "x" => 1 } }
    );
}

#[test]
fn test_merge_keys_disabled_or_quoted() {
    let value: Value = from_str("'<<': 1\n\"<<\": 2\n").unwrap();
    assert_eq!(value, map! { "<<" => 1, "<<" => 2 });
    let value: Value = DeserializerConfig::new()
        .merge_keys(false)
        .from_str("<<: {x: 1}")
        .unwrap();
    assert_eq!(value, map! { "<<" => map! { "x" => 1 } });
    // `<<` as a value or in a sequence is a string
    let value: Value = from_str("a: <<\nb: [<<]").unwrap();
    assert_eq!(value, map! { "a" => "<<", "b" => seq!["<<"] });
}

#[test]
fn test_merge_key_errors() {
    let err = from_str::<Value>("<<: 1").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: the value of a merge key must be a mapping or a sequence of mappings at line 1 column 5"
    );
    let err = from_str::<Value>("<<: [{a: 1}, [1]]").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: merge keys can only merge mappings or sequences of mappings at line 1 column 14"
    );
    let err = from_str::<Value>("- &a 1\n- <<: *a").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: merge keys can only merge mappings or sequences of mappings at line 1 column 3"
    );

    // merges count towards the alias limit
    let mut input = String::from("a0: &a0 {x: 1, y: 2}\n");
    for i in 1..30 {
        input.push_str(&format!(
            "a{i}: &a{i} {{<<: [*a{p}, *a{p}]}}\n",
            i = i,
            p = i - 1
        ));
    }
    let err = from_str::<Value>(&input).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: aliases expand to too many events"
    );
}

#[test]
fn test_borrowing() {
    #[derive(Deserialize, Debug)]
    struct Doc<'a> {
        name: &'a str,
        quoted: &'a str,
        list: Vec<&'a str>,
    }

    let input = "name: demo\nquoted: 'text'\nlist: [a, b]\n";
    let doc: Doc = from_str(input).unwrap();
    assert_eq!(doc.name, "demo");
    assert_eq!(doc.quoted, "text");
    assert_eq!(doc.list, ["a", "b"]);
    assert!(input.as_bytes().as_ptr_range().contains(&doc.name.as_ptr()));

    // scalars that are unescaped or folded cannot be borrowed
    let err = from_str::<&str>("\"a\\tb\"").unwrap_err();
    assert!(err.to_string().contains("expected a borrowed string"));
    assert_eq!(from_str::<String>("\"a\\tb\"").unwrap(), "a\tb");
}

#[test]
fn test_error_locations() {
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Server {
        host: String,
        port: u16,
    }

    let err = from_str::<Vec<Server>>("- host: a\n  port: 1\n- host: b\n  port: x\n").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected string, expected u16 at line 4 column 9"
    );

    // values produced by aliases report the anchored node
    let err = from_str::<Vec<u32>>("- &a x\n- 1\n- *a\n").unwrap_err();
    assert_eq!((err.line(), err.column()), (Some(1), Some(3)));

    // the limits of the configuration are enforced by a layer
    use deser::de::{Format, Limits};
    let err = Deserializer::from_str("a: [1, 2, 3]")
        .deserialize_with::<Value, _>(|driver| driver.push_layer(Limits::new().max_items(2)))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: too many items at line 1 column 11"
    );
}
