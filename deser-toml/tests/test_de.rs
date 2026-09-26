use std::collections::{BTreeMap, HashMap};

use deser::{Deserialize, ErrorKind};
use deser_toml::{Date, Datetime, Offset, Time, from_slice, from_str};

use crate::common;

use common::Value;

fn parse(input: &str) -> Value {
    match from_str(input) {
        Ok(value) => value,
        Err(err) => panic!("failed to parse {:?}: {}", input, err),
    }
}

fn fails(input: &str) -> String {
    match from_str::<Value>(input) {
        Ok(value) => panic!("expected {:?} to fail, got {:?}", input, value),
        Err(err) => err.to_string(),
    }
}

fn datetime(s: &str) -> Datetime {
    s.parse().unwrap()
}

#[derive(Deserialize, Debug, PartialEq)]
struct Config {
    name: String,
    port: u16,
    debug: bool,
    ratio: f64,
    tags: Vec<String>,
    owner: Option<String>,
    limits: HashMap<String, u32>,
    servers: Vec<Server>,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Server {
    host: String,
    #[deser(default)]
    backup: bool,
}

#[test]
fn test_struct() {
    let config: Config = from_str(
        r#"
# a comment
name = "web"
port = 8080
debug = false
ratio = 0.5
tags = ["a", 'b', """c"""]

[limits]
cpu = 2
memory = 512

[[servers]]
host = "alpha"

[[servers]]
host = "beta"
backup = true
"#,
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
            servers: vec![
                Server {
                    host: "alpha".into(),
                    backup: false,
                },
                Server {
                    host: "beta".into(),
                    backup: true,
                },
            ],
        }
    );
}

#[test]
fn test_key_order() {
    let value = parse("b = 1\na.y = 2\n[c]\n[a.x]\n");
    let Value::Table(items) = value else { panic!() };
    let keys: Vec<_> = items.iter().map(|x| x.0.as_str()).collect();
    assert_eq!(keys, ["b", "a", "c"]);
}

#[test]
fn test_empty() {
    assert_eq!(parse(""), table! {});
    assert_eq!(parse("\n# comment\n\n"), table! {});
    // a BOM at the start is ignored
    assert_eq!(parse("\u{feff}a = 1"), table! {"a" => 1});
    fails("a = 1\n\u{feff}");
}

#[test]
fn test_integers() {
    assert_eq!(parse("a = +99"), table! {"a" => 99});
    assert_eq!(parse("a = -17"), table! {"a" => -17});
    assert_eq!(parse("a = 1_000"), table! {"a" => 1000});
    assert_eq!(parse("a = -0"), table! {"a" => 0});
    assert_eq!(parse("a = 0xDEAD_beef"), table! {"a" => 0xdead_beef});
    assert_eq!(parse("a = 0o755"), table! {"a" => 0o755});
    assert_eq!(parse("a = 0b1101"), table! {"a" => 0b1101});
    assert_eq!(parse("a = 0x0001"), table! {"a" => 1});

    for invalid in [
        "01", "+01", "-01", "0_0", "1__0", "_1", "1_", "0x", "0x_1", "0x1_", "+0x1", "-0o1", "0X1",
        "0B1", "0O1", "0b2", "0o8", "0xg", "1a", "- 1", "++1",
    ] {
        fails(&format!("a = {}", invalid));
    }
}

#[test]
fn test_integer_range() {
    let value: BTreeMap<String, i64> = from_str("a = -9223372036854775808").unwrap();
    assert_eq!(value["a"], i64::MIN);
    // integers larger than i64 are supported up to u64
    let value: BTreeMap<String, u64> = from_str("a = 18446744073709551615").unwrap();
    assert_eq!(value["a"], u64::MAX);
    let value: BTreeMap<String, u64> = from_str("a = 0xffff_ffff_ffff_ffff").unwrap();
    assert_eq!(value["a"], u64::MAX);

    // integers that cannot be represented are an error
    for invalid in [
        "18446744073709551616",
        "-9223372036854775809",
        "0x1_0000_0000_0000_0000",
    ] {
        let err = from_str::<Value>(&format!("a = {}", invalid)).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::OutOfRange);
    }
}

#[test]
fn test_floats() {
    assert_eq!(parse("a = +1.0"), table! {"a" => 1.0});
    assert_eq!(parse("a = -0.01"), table! {"a" => -0.01});
    assert_eq!(parse("a = 5e+22"), table! {"a" => 5e22});
    assert_eq!(parse("a = 1e06"), table! {"a" => 1e6});
    assert_eq!(parse("a = -2E-2"), table! {"a" => -0.02});
    assert_eq!(parse("a = 6.626e-34"), table! {"a" => 6.626e-34});
    assert_eq!(
        parse("a = 224_617.445_991_228"),
        table! {"a" => 224617.445991228}
    );
    assert_eq!(parse("a = 0e2"), table! {"a" => 0.0});
    assert_eq!(parse("a = 0.0_1"), table! {"a" => 0.01});
    assert_eq!(parse("a = inf"), table! {"a" => f64::INFINITY});
    assert_eq!(parse("a = +inf"), table! {"a" => f64::INFINITY});
    assert_eq!(parse("a = -inf"), table! {"a" => f64::NEG_INFINITY});
    let value: BTreeMap<String, f64> = from_str("a = nan\nb = -nan\nc = -0.0").unwrap();
    assert!(value["a"].is_nan());
    assert!(value["b"].is_nan());
    assert!(value["c"] == 0.0 && value["c"].is_sign_negative());

    for invalid in [
        ".7", "7.", "3.e+20", "+.7", "-.7", "1.2.3", "1e", "1e+", "1e_2", "1_e2", "1._2", "1.2_",
        "00.1", "01.1", "1e2.3", "0x1.5", "Inf", "NaN", "infinity", "inf_", "+-inf", "1.0e1e1",
        "1..2",
    ] {
        fails(&format!("a = {}", invalid));
    }

    // floats that overflow are not silently turned into infinity
    let err = from_str::<Value>("a = 1e400").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::OutOfRange);
}

#[test]
fn test_booleans() {
    assert_eq!(
        parse("a = true\nb = false"),
        table! {"a" => true, "b" => false}
    );
    fails("a = True");
    fails("a = truex");
    fails("a = tru");
}

#[test]
fn test_basic_strings() {
    assert_eq!(
        parse(r#"a = "tab\there \"quoted\" \\ \u00e9 \U0001F600 \x41 \e""#),
        table! {"a" => "tab\there \"quoted\" \\ é 😀 A \x1b"}
    );
    assert_eq!(parse("a = \"raw\ttab\""), table! {"a" => "raw\ttab"});
    assert_eq!(parse(r#"a = """#), table! {"a" => ""});
    // ambiguous looking escapes only consume their digits (toml#675)
    assert_eq!(
        parse(r#"a = "\u01234567890ABCDEFGH""#),
        table! {"a" => "\u{123}4567890ABCDEFGH"}
    );

    for invalid in [
        r#""\a""#,
        r#""\ ""#,
        r#""\/""#,
        r#""\x4""#,
        r#""\u12""#,
        r#""\uD800""#,
        r#""\U00110000""#,
        "\"a\nb\"",
        "\"a\x01\"",
        "\"a\x7f\"",
        r#""unclosed"#,
    ] {
        fails(&format!("a = {}", invalid));
    }
}

#[test]
fn test_multiline_basic_strings() {
    assert_eq!(
        parse("a = \"\"\"\nRoses\nViolets\"\"\""),
        table! {"a" => "Roses\nViolets"}
    );
    // only the first newline is trimmed
    assert_eq!(parse("a = \"\"\"\n\nx\"\"\""), table! {"a" => "\nx"});
    // line ending backslashes
    assert_eq!(
        parse("a = \"\"\"\nThe quick brown \\\n\n\n  fox jumps over \\\n    the lazy dog.\"\"\""),
        table! {"a" => "The quick brown fox jumps over the lazy dog."}
    );
    assert_eq!(
        parse("a = \"\"\"a \\  \t\n  b\"\"\""),
        table! {"a" => "a b"}
    );
    // up to two quotes in the content and before the delimiter
    assert_eq!(
        parse(r#"a = """Here are two quotation marks: "". Simple enough.""""#),
        table! {"a" => "Here are two quotation marks: \"\". Simple enough."}
    );
    assert_eq!(
        parse("a = \"\"\"\"This\"\"\"\""),
        table! {"a" => "\"This\""}
    );
    assert_eq!(
        parse("a = \"\"\"\"This\"\"\"\"\""),
        table! {"a" => "\"This\"\""}
    );
    assert_eq!(parse("a = \"\"\"\"\"\"\""), table! {"a" => "\""});
    assert_eq!(parse("a = \"\"\"\"\"\"\"\""), table! {"a" => "\"\""});
    assert_eq!(parse(r#"a = """ \"\"\" """"#), table! {"a" => " \"\"\" "});
    // newlines are normalized
    assert_eq!(parse("a = \"\"\"\r\nx\r\ny\"\"\""), table! {"a" => "x\ny"});
    // a raw tab is fine (toml#571)
    assert_eq!(parse("a = \"\"\"\t\"\"\""), table! {"a" => "\t"});

    for invalid in [
        "\"\"\"a\"\"\"\"\"\"",
        // a backslash before the delimiter is an escaped quote (toml#824)
        r#""""\""""#,
        // the backslash has to be the last non-whitespace character
        r#""""\ """"#,
        "\"\"\"a\rb\"\"\"",
        "\"\"\"a\x0bb\"\"\"",
        r#""""unclosed"#,
    ] {
        fails(&format!("a = {}", invalid));
    }
}

#[test]
fn test_literal_strings() {
    assert_eq!(
        parse(r"a = 'C:\Users\nodejs\templates'"),
        table! {"a" => r"C:\Users\nodejs\templates"}
    );
    assert_eq!(
        parse("a = '''\nThe first newline is\ntrimmed.\n'''"),
        table! {"a" => "The first newline is\ntrimmed.\n"}
    );
    assert_eq!(
        parse("a = ''''That,' she said, 'is still pointless.''''"),
        table! {"a" => "'That,' she said, 'is still pointless.'"}
    );
    assert_eq!(parse("a = '''\r\nx\r\n'''"), table! {"a" => "x\n"});
    for invalid in [
        "'a\nb'",
        "'''a''''''",
        "'a\x01'",
        "'''a\x7f'''",
        "'unclosed",
    ] {
        fails(&format!("a = {}", invalid));
    }
}

#[test]
fn test_borrowed_strings() {
    #[derive(Deserialize)]
    struct Doc {
        a: String,
    }
    let doc: Doc = from_str("a = 'x'").unwrap();
    assert_eq!(doc.a, "x");
}

#[test]
fn test_datetimes() {
    let value: BTreeMap<String, Datetime> = from_str(
        "odt1 = 1979-05-27T07:32:00Z
odt2 = 1979-05-27T00:32:00-07:00
odt3 = 1979-05-27T00:32:00.999999-07:00
odt4 = 1979-05-27 07:32:00z
odt5 = 1979-05-27t07:32Z
ldt1 = 1979-05-27T07:32:00
ldt2 = 1979-05-27T07:32
ld1 = 1979-05-27
lt1 = 07:32:00
lt2 = 00:32:00.999999
lt3 = 07:32
leap = 1990-12-31T23:59:60Z
",
    )
    .unwrap();
    let text = |key: &str| value[key].to_string();
    assert_eq!(text("odt1"), "1979-05-27T07:32:00Z");
    assert_eq!(text("odt2"), "1979-05-27T00:32:00-07:00");
    assert_eq!(text("odt3"), "1979-05-27T00:32:00.999999-07:00");
    assert_eq!(text("odt4"), "1979-05-27T07:32:00Z");
    assert_eq!(text("odt5"), "1979-05-27T07:32:00Z");
    assert_eq!(text("ldt1"), "1979-05-27T07:32:00");
    assert_eq!(text("ldt2"), "1979-05-27T07:32:00");
    assert_eq!(text("ld1"), "1979-05-27");
    assert_eq!(text("lt1"), "07:32:00");
    assert_eq!(text("lt2"), "00:32:00.999999");
    assert_eq!(text("lt3"), "07:32:00");
    assert_eq!(text("leap"), "1990-12-31T23:59:60Z");

    assert_eq!(
        value["odt2"],
        Datetime {
            date: Some(Date {
                year: 1979,
                month: 5,
                day: 27
            }),
            time: Some(Time {
                hour: 0,
                minute: 32,
                second: 0,
                nanosecond: 0
            }),
            offset: Some(Offset::Custom { minutes: -420 }),
        }
    );

    // digits beyond nanoseconds are truncated, not rounded
    let value: BTreeMap<String, Datetime> = from_str("a = 00:00:00.1234567899").unwrap();
    assert_eq!(value["a"].time.unwrap().nanosecond, 123_456_789);

    // a local date followed by a space and a comment
    let value = parse("a = 1979-05-27 # comment\nb = [1979-05-27 , 07:32]");
    assert_eq!(
        value,
        table! {
            "a" => datetime("1979-05-27"),
            "b" => array![datetime("1979-05-27"), datetime("07:32:00")],
        }
    );

    for invalid in [
        "1979-05-27T",
        "1979-05-27 07",
        "1979-05-27T7:32:00",
        "1979-5-27",
        "01979-05-27",
        "1979-02-29",
        "2100-02-29",
        "1979-13-01",
        "1979-04-31",
        "1979-05-27T24:00:00",
        "1979-05-27T23:60:00",
        "1979-05-27T23:59:61",
        "1979-05-27T23:59:59.",
        "1979-05-27T23:59Z.5",
        "1979-05-27T23:59.5",
        "1979-05-27T23:59:59+24:00",
        "1979-05-27T23:59:59+01",
        "1979-05-27T23:59:59+0100",
        "07:32:00Z",
        "07:32:00+01:00",
        "07:32:00.5.5",
        "1979-05-27x",
        "1979-05-27T07:32:00Zx",
    ] {
        fails(&format!("a = {}", invalid));
    }
}

#[test]
fn test_datetime_fallback() {
    // consumers that do not know about date-times get strings
    let value: BTreeMap<String, String> =
        from_str("a = 1979-05-27 07:32:00.5+01:30\nb = 07:32").unwrap();
    assert_eq!(value["a"], "1979-05-27T07:32:00.5+01:30");
    assert_eq!(value["b"], "07:32:00");
}

#[test]
fn test_datetime_from_str() {
    let dt: Datetime = "1979-05-27 07:32:00.5-07:00".parse().unwrap();
    assert_eq!(dt.to_string(), "1979-05-27T07:32:00.5-07:00");
    assert_eq!(format!("{:?}", dt), "Datetime(1979-05-27T07:32:00.5-07:00)");
    assert!("1979-05-27 ".parse::<Datetime>().is_err());
    assert!("".parse::<Datetime>().is_err());
    assert!("x".parse::<Datetime>().is_err());
    assert!("07:32Z".parse::<Datetime>().is_err());
}

#[test]
fn test_arrays() {
    assert_eq!(
        parse("a = [ [ 1, 2 ], [\"a\", 'b'], [], [{}] ]"),
        table! {
            "a" => array![array![1, 2], array!["a", "b"], array![], array![table! {}]],
        }
    );
    // newlines, comments and trailing commas (toml#766)
    assert_eq!(
        parse("a = [\n  1 # one\n  , # comma\n  2, # two\n  # end\n]"),
        table! {"a" => array![1, 2]}
    );
    for invalid in [
        "[,]",
        "[1,,]",
        "[1,,2]",
        "[1 2]",
        "[1",
        "[1,",
        "[1,]]",
        "[# comment]",
        "[\"a\" \"b\"]",
    ] {
        fails(&format!("a = {}", invalid));
    }
}

#[test]
fn test_inline_tables() {
    assert_eq!(
        parse("a = { x = 1, y.z = 2, 'w' = { v = [] } }"),
        table! {
            "a" => table! {
                "x" => 1,
                "y" => table! {"z" => 2},
                "w" => table! {"v" => array![]},
            },
        }
    );
    // TOML 1.1 permits newlines, comments and trailing commas
    assert_eq!(
        parse("a = {\n  x = 1, # one\n  y = {\n    z = 2,\n  },\n}"),
        table! {"a" => table! {"x" => 1, "y" => table! {"z" => 2}}}
    );
    assert_eq!(
        parse("a = {}\nb = {\n}"),
        table! {"a" => table! {}, "b" => table! {}}
    );

    for invalid in [
        "{,}",
        "{x = 1,,}",
        "{x = 1 y = 2}",
        "{x = 1",
        "{x}",
        "{x =}",
        "{x = 1, x = 2}",
        "{x.y = 1, x = 2}",
        "{x = 1, x.y = 2}",
        "{x = {y = 1}, x.z = 2}",
        "{x\n= 1}",
        "{x =\n1}",
        "{= 1}",
    ] {
        fails(&format!("a = {}", invalid));
    }
}

#[test]
fn test_keys() {
    assert_eq!(
        parse("\"127.0.0.1\" = 1\n'quoted \"value\"' = 2\n\"\" = 3\n1234 = 4\ntrue = 5\ninf = 6"),
        table! {
            "127.0.0.1" => 1,
            "quoted \"value\"" => 2,
            "" => 3,
            "1234" => 4,
            "true" => 5,
            "inf" => 6,
        }
    );
    // dotted keys that look like floats (toml#616)
    assert_eq!(
        parse("3.14159 = \"pi\""),
        table! {"3" => table! {"14159" => "pi"}}
    );
    assert_eq!(
        parse("a . b\t.\"c\" = 1\n[ d .  e ]\n[[ f . g ]]"),
        table! {
            "a" => table! {"b" => table! {"c" => 1}},
            "d" => table! {"e" => table! {}},
            "f" => table! {"g" => array![table! {}]},
        }
    );
    // quoted and bare keys are the same key
    fails("spelling = 1\n\"spelling\" = 2");
    fails("'a' = 1\n\"a\" = 2");
    // keys are compared ordinally without normalization (toml#966)
    assert_eq!(
        parse(
            r#""pr\xe9nom" = 1
"pre\u0301nom" = 2"#
        ),
        table! {"pr\u{e9}nom" => 1, "pre\u{301}nom" => 2}
    );

    for invalid in [
        "= 1",
        "a = ",
        "a",
        "a.= 1",
        ".a = 1",
        "a..b = 1",
        "a b = 1",
        "a\tb = 1",
        "\"\"\"a\"\"\" = 1",
        "'''a''' = 1",
        "a = 1 b = 2",
        "a = 1 # ok\nb = 2 c",
        "a = \n1",
        "a\n= 1",
        "é = 1",
        "a = 1 = 2",
    ] {
        fails(invalid);
    }
}

#[test]
fn test_dotted_keys() {
    // out of order dotted keys are valid (toml#632)
    assert_eq!(
        parse("apple.type = 1\norange.type = 2\napple.skin = 3"),
        table! {
            "apple" => table! {"type" => 1, "skin" => 3},
            "orange" => table! {"type" => 2},
        }
    );
    assert_eq!(
        parse("fruit.apple.smooth = true\nfruit.orange = 2"),
        table! {"fruit" => table! {"apple" => table! {"smooth" => true}, "orange" => 2}}
    );
    fails("fruit.apple = 1\nfruit.apple.smooth = true");
    fails("a.b = 1\na = 2");
    fails("a.b = 1\na.b.c = 2");
}

#[test]
fn test_tables() {
    // super tables can be defined afterwards (toml#638)
    assert_eq!(
        parse("[x.y.z.w]\na = 1\n[x]\nb = 2"),
        table! {
            "x" => table! {
                "y" => table! {"z" => table! {"w" => table! {"a" => 1}}},
                "b" => 2,
            },
        }
    );
    // headers can define sub-tables of tables defined by dotted keys
    assert_eq!(
        parse(
            "[fruit]\napple.color = 'red'\napple.taste.sweet = true\n[fruit.apple.texture]\nsmooth = true"
        ),
        table! {
            "fruit" => table! {
                "apple" => table! {
                    "color" => "red",
                    "taste" => table! {"sweet" => true},
                    "texture" => table! {"smooth" => true},
                },
            },
        }
    );
    assert_eq!(
        parse("a.b = 1\n[a.c]\nd = 2"),
        table! {"a" => table! {"b" => 1, "c" => table! {"d" => 2}}}
    );

    // tables cannot be defined twice
    fails("[a]\n[a]");
    fails("[a]\nb = 1\n[a.b]");
    fails("[a.b]\n[a]\n[a]");
    fails("[a]\n[b]\n[a]");
    // tables defined by dotted keys cannot be defined by headers (toml#631)
    fails("[fruit]\napple.color = 'red'\n[fruit.apple]");
    fails("[fruit]\napple.taste.sweet = true\n[fruit.apple.taste]");
    fails("a.b = 1\n[a]");
    // dotted keys cannot add to tables defined elsewhere (toml#846)
    fails("[a.b.c]\nz = 9\n[a]\nb.c.t = 1");
    fails("[a.b.c.d]\nz = 9\n[a]\nb.c.d.k.t = 1");
    fails("[a.b.c]\n[a]\nb = 1");
    // tables that were only created by headers (not defined) can be defined
    // by dotted keys as the order of sections does not matter (toml#771)
    assert_eq!(
        parse("[a.b.c]\n[a]\nb.d = 1"),
        parse("[a]\nb.d = 1\n[a.b.c]")
    );
    // once defined by dotted keys they cannot be defined again
    fails("[a.b.c]\n[a]\nb.d = 1\n[a.b]");
    fails("[a.b.c]\n[a.b]\n[a]\nb.d = 1");
    // but they can get more sub-tables
    assert_eq!(
        parse("[a.b.c]\n[a]\nb.d = 1\n[a.b.e]"),
        table! {
            "a" => table! {
                "b" => table! {"c" => table! {}, "d" => 1, "e" => table! {}},
            },
        }
    );
    // or tables defined by dotted keys under another header
    fails("[a]\nb.c = 1\n[a.d]\n[a.b.e]\nf = 1\n[x]\n[a.b]");

    // headers must be on their own line
    for invalid in [
        "[a] b = 1",
        "[a]]",
        "[[a]",
        "[a",
        "[]",
        "[a.]",
        "[.a]",
        "[a..b]",
        "[ [a]]",
        "[[a] ]",
        "[a]\n[\"\"\"b\"\"\"]",
        "[a] [b]",
    ] {
        fails(invalid);
    }
}

#[test]
fn test_inline_tables_are_complete() {
    // nothing can be added to inline tables (toml#630)
    fails("[product]\ntype = { name = 'Nail' }\ntype.edible = false");
    fails("[product]\ntype.name = 'Nail'\ntype = { edible = false }");
    fails("a = {}\n[a.b]");
    fails("a = {b = {}}\n[a.b.c]");
    fails("a = {}\n[[a.b]]");
    fails("a = {b.c = 0}\na.b.z = 0");
    fails("a = {b.c = 0}\na.c = 0");
    fails("a = {b.c = 0}\n[a.d]");
}

#[test]
fn test_array_of_tables() {
    assert_eq!(
        parse(
            "[[fruits]]
name = 'apple'
[fruits.physical]
color = 'red'
[[fruits.varieties]]
name = 'red delicious'
[[fruits.varieties]]
name = 'granny smith'
[[fruits]]
name = 'banana'
[[fruits.varieties]]
name = 'plantain'
"
        ),
        table! {
            "fruits" => array![
                table! {
                    "name" => "apple",
                    "physical" => table! {"color" => "red"},
                    "varieties" => array![
                        table! {"name" => "red delicious"},
                        table! {"name" => "granny smith"},
                    ],
                },
                table! {
                    "name" => "banana",
                    "varieties" => array![table! {"name" => "plantain"}],
                },
            ],
        }
    );
    // arrays of tables do not need to be grouped together (toml#1103)
    assert_eq!(
        parse("[[a]]\nx = 1\n[b]\n[[a]]\nx = 2"),
        table! {"a" => array![table! {"x" => 1}, table! {"x" => 2}], "b" => table! {}}
    );
    // dotted keys in the tables of an array of tables
    assert_eq!(
        parse("[[a]]\nb.c = 1\n[[a]]\nb.c = 2"),
        table! {
            "a" => array![table! {"b" => table! {"c" => 1}}, table! {"b" => table! {"c" => 2}}],
        }
    );

    // the parent has to be an array of tables before children are defined
    fails("[fruit.physical]\ncolor = 'red'\n[[fruit]]");
    // static arrays cannot be extended (toml#908)
    fails("fruits = []\n[[fruits]]");
    fails("a = [{ b = 1 }]\n[a.c]");
    fails("a = [{ b = 1 }]\n[[a]]");
    // tables and arrays of tables conflict
    fails("[[fruits]]\n[[fruits.varieties]]\n[fruits.varieties]");
    fails("[[fruits]]\n[fruits.physical]\n[[fruits.physical]]");
    fails("[a]\n[[a]]");
    fails("[[a]]\n[a]");
    // arrays of tables cannot be extended with dotted keys
    fails("[[a.b]]\n[a]\nb.y = 2");
}

#[test]
fn test_control_characters() {
    fails("a = 1 # \x00");
    fails("a = 1 # \x7f");
    fails("a = 1 # a\rb");
    fails("a = 1\r");
    fails("a = 1\x0b");
    fails("\x0c");
    assert_eq!(
        parse("a = 1 # tab\there\r\nb = 2\r\n"),
        table! {"a" => 1, "b" => 2}
    );
    assert_eq!(parse("# ünïcödé comment\na = 1"), table! {"a" => 1});
}

#[test]
fn test_invalid_utf8() {
    let err = from_slice::<Value>(b"a = \"\xff\"").unwrap_err();
    assert!(err.to_string().contains("invalid UTF-8"));
    assert_eq!(from_slice::<Value>(b"a = 1").unwrap(), table! {"a" => 1});
}

#[test]
fn test_error_locations() {
    assert_eq!(
        fails("a = 1\nb = 2\nb = 3"),
        "Unexpected: key 'b' is already defined at line 3 column 1"
    );
    assert_eq!(
        fails("a = \"ä\\q\""),
        "Unexpected: invalid escape sequence at line 1 column 7"
    );
    assert_eq!(
        fails("[a]\nb = 1 c"),
        "Unexpected: unexpected 'c', expected end of line at line 2 column 7"
    );
    assert_eq!(
        fails("a = [1, 2"),
        "EndOfFile: unexpected end of input, expected ',' or ']' at line 1 column 10"
    );
    assert_eq!(
        fails("[a.b.c]\n[a]\nb.c.d = 1"),
        "Unexpected: cannot add keys to table 'c' which is defined elsewhere at line 3 column 3"
    );
    assert_eq!(
        fails("a = 0x-1"),
        "Unexpected: invalid number at line 1 column 5"
    );
}

#[test]
fn test_not_a_table() {
    // the document is a table
    assert!(from_str::<Vec<u32>>("a = 1").is_err());
    let value: HashMap<String, Vec<u32>> = from_str("a = [1, 2]").unwrap();
    assert_eq!(value["a"], [1, 2]);
}

#[test]
fn test_deep_nesting() {
    // neither parsing nor emitting the events recurses
    let depth = if cfg!(miri) { 300 } else { 100_000 };
    let input = format!("a = {}{}", "[".repeat(depth), "]".repeat(depth));
    let mut out = None::<()>;
    let mut driver = deser::de::DeserializeDriver::from_sink(deser::de::SinkHandle::null());
    deser_toml::Deserializer::from_str(&input)
        .drive(&mut driver)
        .unwrap();
    drop(driver);
    assert!(out.take().is_none());

    let input = format!("a = {}{}", "{b = ".repeat(depth), "}".repeat(depth));
    let mut driver = deser::de::DeserializeDriver::from_sink(deser::de::SinkHandle::null());
    assert!(
        deser_toml::Deserializer::from_str(&input)
            .drive(&mut driver)
            .is_err()
    );

    let input = format!("a = {}1{}", "{b = ".repeat(depth), "}".repeat(depth));
    let mut driver = deser::de::DeserializeDriver::from_sink(deser::de::SinkHandle::null());
    deser_toml::Deserializer::from_str(&input)
        .drive(&mut driver)
        .unwrap();

    let input = format!("[{}]", vec!["a"; depth].join("."));
    let mut driver = deser::de::DeserializeDriver::from_sink(deser::de::SinkHandle::null());
    deser_toml::Deserializer::from_str(&input)
        .drive(&mut driver)
        .unwrap();
}

#[test]
fn test_large_tables() {
    // tables with many keys are indexed
    let input: String = (0..1000).map(|x| format!("k{} = {}\n", x, x)).collect();
    let value: BTreeMap<String, u32> = from_str(&input).unwrap();
    assert_eq!(value.len(), 1000);
    assert_eq!(value["k999"], 999);
    let err = from_str::<Value>(&format!("{}k500 = 1", input)).unwrap_err();
    assert!(err.to_string().contains("'k500' is already defined"));
}

#[test]
fn test_borrowing() {
    #[derive(Deserialize, Debug)]
    struct Doc<'a> {
        name: &'a str,
        literal: &'a str,
        table: BTreeMap<&'a str, &'a str>,
    }

    let input = "name = \"demo\"\nliteral = 'C:\\path'\n[table]\nkey = \"value\"\n";
    let doc: Doc = from_str(input).unwrap();
    assert_eq!(doc.name, "demo");
    assert_eq!(doc.literal, "C:\\path");
    assert_eq!(doc.table["key"], "value");
    assert!(input.as_bytes().as_ptr_range().contains(&doc.name.as_ptr()));

    // strings with escapes cannot be borrowed
    let err = from_str::<BTreeMap<String, &str>>("a = \"\\n\"").unwrap_err();
    assert!(err.to_string().contains("expected a borrowed string"));
}

#[test]
fn test_lexical_keys() {
    // keys are lexical, they parse into the type of the key
    let map: BTreeMap<String, BTreeMap<u16, String>> =
        from_str("[ports]\n80 = \"http\"\n\"443\" = \"https\"\n").unwrap();
    assert_eq!(map["ports"][&80], "http");
    assert_eq!(map["ports"][&443], "https");
    let map: HashMap<bool, u8> = from_str("true = 1\nfalse = 0\n").unwrap();
    assert_eq!(map[&true], 1);
}
