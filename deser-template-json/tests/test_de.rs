use super::{DIALECT, dialect};
use deser::Deserialize;
use dialect::from_str;

#[test]
fn test_basic() {
    let x: Vec<u32> = from_str(r#"[1, 2, 3, 4]"#).unwrap();
    assert_eq!(x, vec![1, 2, 3, 4]);
}

#[test]
fn test_flatten() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct User {
        id: u64,
        #[deser(flatten)]
        attrs: Attrs,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Attrs {
        is_active: bool,
        is_admin: bool,
        flags: Vec<String>,
    }

    let user: User = from_str(
        r#"
        {
            "id": 42,
            "is_active": true,
            "is_admin": true,
            "flags": ["german", "staff"]
        }
    "#,
    )
    .unwrap();

    assert_eq!(
        user,
        User {
            id: 42,
            attrs: Attrs {
                is_active: true,
                is_admin: true,
                flags: vec!["german".into(), "staff".into()],
            }
        }
    )
}

#[test]
fn test_optional_compounds() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Inner {
        a: u32,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Outer {
        inner: Option<Inner>,
        list: Option<Vec<u32>>,
        boxed: Option<Box<Inner>>,
        missing: Option<Inner>,
        null: Option<Inner>,
    }

    let outer: Outer =
        from_str(r#"{"inner": {"a": 1}, "list": [1, 2], "boxed": {"a": 2}, "null": null}"#)
            .unwrap();
    assert_eq!(
        outer,
        Outer {
            inner: Some(Inner { a: 1 }),
            list: Some(vec![1, 2]),
            boxed: Some(Box::new(Inner { a: 2 })),
            missing: None,
            null: None,
        }
    );
}

#[test]
fn test_flatten_map() {
    use std::collections::{BTreeMap, HashMap};

    // flattened maps take the keys no field took
    #[derive(Deserialize, deser::Serialize, PartialEq, Eq, Debug)]
    pub struct User {
        id: u64,
        #[deser(flatten)]
        extra: BTreeMap<String, u32>,
    }

    let json = r#"{"id":42,"a":1,"b":2}"#;
    let user: User = from_str(json).unwrap();
    assert_eq!(
        user,
        User {
            id: 42,
            extra: BTreeMap::from([("a".into(), 1), ("b".into(), 2)]),
        }
    );
    assert_eq!(dialect::to_string(&user).unwrap(), json);

    let user: User = from_str(r#"{"id":42}"#).unwrap();
    assert!(user.extra.is_empty());
    let err = from_str::<User>(r#"{"id":42,"a":"x"}"#).unwrap_err();
    assert_eq!(err.message(), "unexpected string, expected u32");
    let err = from_str::<User>(r#"{"id":42,"a":1,"a":2}"#).unwrap_err();
    assert_eq!(err.message(), "duplicate key in map");

    // keys are parsed like other map keys
    #[derive(Deserialize, deser::Serialize, PartialEq, Eq, Debug)]
    pub struct Numbers {
        name: String,
        #[deser(flatten)]
        numbers: HashMap<u32, bool>,
    }

    let json = r#"{"name":"x","2":true}"#;
    let numbers: Numbers = from_str(json).unwrap();
    assert_eq!(numbers.numbers, HashMap::from([(2, true)]));
    assert_eq!(dialect::to_string(&numbers).unwrap(), json);
    let err = from_str::<Numbers>(r#"{"name":"x","y":true}"#).unwrap_err();
    assert_eq!(err.message(), "invalid value \"y\", expected u32");
}

#[test]
fn test_flatten_optional() {
    #[derive(Deserialize, deser::Serialize, PartialEq, Eq, Debug)]
    pub struct User {
        id: u64,
        #[deser(flatten)]
        attrs: Option<Attrs>,
    }

    #[derive(Deserialize, deser::Serialize, PartialEq, Eq, Debug)]
    pub struct Attrs {
        is_admin: bool,
    }

    let json = r#"{"id":42,"is_admin":true}"#;
    let user: User = from_str(json).unwrap();
    assert_eq!(
        user,
        User {
            id: 42,
            attrs: Some(Attrs { is_admin: true }),
        }
    );
    assert_eq!(dialect::to_string(&user).unwrap(), json);

    // without any of its keys the value is missing
    let json = r#"{"id":42}"#;
    let user: User = from_str(json).unwrap();
    assert_eq!(
        user,
        User {
            id: 42,
            attrs: None
        }
    );
    assert_eq!(dialect::to_string(&user).unwrap(), json);
    let user: User = from_str(r#"{"id":42,"other":true}"#).unwrap();
    assert_eq!(
        user,
        User {
            id: 42,
            attrs: None
        }
    );

    // errors are not swallowed
    let err = from_str::<User>(r#"{"id":42,"is_admin":1}"#).unwrap_err();
    assert_eq!(err.message(), "unexpected unsigned integer, expected bool");
}

#[test]
fn test_maps() {
    use std::collections::{BTreeMap, HashMap};

    let map: HashMap<String, u32> = from_str(r#"{"a": 1, "b": 2}"#).unwrap();
    assert_eq!(map.len(), 2);
    assert_eq!(map["a"], 1);
    assert_eq!(map["b"], 2);

    let map: BTreeMap<String, u32> = from_str(r#"{"a": 1, "b": 2}"#).unwrap();
    assert_eq!(
        map.into_iter().collect::<Vec<_>>(),
        vec![("a".into(), 1), ("b".into(), 2)]
    );
}

#[test]
fn test_generics() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Wrapper<T> {
        value: T,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Newtype<T>(T);

    let w: Wrapper<Vec<u32>> = from_str(r#"{"value": [1, 2]}"#).unwrap();
    assert_eq!(w, Wrapper { value: vec![1, 2] });

    let n: Newtype<u32> = from_str(r#"42"#).unwrap();
    assert_eq!(n, Newtype(42));
}

#[test]
fn test_numeric_keys() {
    use std::collections::{BTreeMap, HashMap};

    let map: HashMap<u32, u32> = from_str(r#"{"42": 23}"#).unwrap();
    assert_eq!(map[&42], 23);

    let map: BTreeMap<i64, bool> = from_str(r#"{"-1": true, "2": false}"#).unwrap();
    assert_eq!(
        map.into_iter().collect::<Vec<_>>(),
        vec![(-1, true), (2, false)]
    );

    // only keys are lexical, strings are not parsed as numbers
    assert!(from_str::<u32>(r#""42""#).is_err());
    assert!(from_str::<Vec<u32>>(r#"["42"]"#).is_err());
    assert!(from_str::<HashMap<u32, u32>>(r#"{"x": 1}"#).is_err());
}

#[test]
fn test_char() {
    let c: char = from_str(r#""x""#).unwrap();
    assert_eq!(c, 'x');
}

#[test]
fn test_unknown_keys() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Simple {
        a: u32,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct WithFlatten {
        a: u32,
        #[deser(flatten)]
        simple: Inner,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Inner {
        #[deser(alias = "bee")]
        b: u32,
    }

    let s: Simple = from_str(r#"{"x": {"y": [1, 2]}, "a": 1, "z": null}"#).unwrap();
    assert_eq!(s, Simple { a: 1 });

    let s: WithFlatten = from_str(r#"{"x": [1], "bee": 2, "a": 1, "z": null}"#).unwrap();
    assert_eq!(
        s,
        WithFlatten {
            a: 1,
            simple: Inner { b: 2 }
        }
    );
}

#[test]
fn test_strings() {
    let s: String = from_str(r#""hello world, this is a longer string""#).unwrap();
    assert_eq!(s, "hello world, this is a longer string");
    let s: String = from_str(r#""a longer string with \"escapes\" and \u00e9 and \\""#).unwrap();
    assert_eq!(s, "a longer string with \"escapes\" and \u{e9} and \\");
    let s: String = from_str("\"日本語のテキストもちゃんと動く\"").unwrap();
    assert_eq!(s, "日本語のテキストもちゃんと動く");
    // JSON5 and Hjson only disallow line breaks in strings
    if !DIALECT.single_quotes {
        assert!(from_str::<String>("\"control \x01 character\"").is_err());
    }
    assert!(from_str::<String>("\"line \n break\"").is_err());
    assert!(from_str::<String>("\"unterminated string").is_err());
}

#[test]
fn test_syntax_errors() {
    use std::collections::BTreeMap;

    if !DIALECT.trailing_commas {
        assert!(from_str::<Vec<u32>>("[1,]").is_err());
        assert!(from_str::<BTreeMap<String, u32>>(r#"{"a":1,}"#).is_err());
    }
    for json in [
        "[,1]", "]", "[1 2]", "[1]]", "[1] x", "", "[", "[1", "[1,", "[}", "[1,,]",
    ] {
        assert!(from_str::<Vec<u32>>(json).is_err(), "accepted {:?}", json);
    }
    // in Hjson `1` is a key without quotes
    if !DIALECT.hjson {
        assert!(from_str::<BTreeMap<String, u32>>(r#"{1: 2}"#).is_err());
    }
    for json in [
        r#"{"a" 1}"#,
        r#"{"a":"#,
        r#"{"a"}"#,
        r#"{,"a":1}"#,
        r#"{"a":1"b":2}"#,
        r#"{"a":1]"#,
    ] {
        assert!(
            from_str::<BTreeMap<String, u32>>(json).is_err(),
            "accepted {:?}",
            json
        );
    }

    let map: BTreeMap<String, Vec<u32>> = from_str(r#" { "a" : [ ] , "b" : [ 1 , 2 ] } "#).unwrap();
    assert_eq!(map["a"], Vec::<u32>::new());
    assert_eq!(map["b"], vec![1, 2]);
}

#[test]
fn test_wide_integers() {
    use std::collections::BTreeMap;

    assert_eq!(from_str::<u128>(&u128::MAX.to_string()).unwrap(), u128::MAX);
    assert_eq!(from_str::<i128>(&i128::MIN.to_string()).unwrap(), i128::MIN);
    assert_eq!(
        from_str::<u128>("18446744073709551616").unwrap(),
        1u128 << 64
    );
    // just below i64::MIN
    assert_eq!(
        from_str::<i128>("-9223372036854775809").unwrap(),
        i64::MIN as i128 - 1
    );
    assert_eq!(
        from_str::<i128>("-18446744073709551616").unwrap(),
        -(1i128 << 64)
    );
    assert_eq!(from_str::<u128>("42").unwrap(), 42);
    assert_eq!(from_str::<i128>("-42").unwrap(), -42);

    // wide integers still work for floats
    assert_eq!(
        from_str::<f64>("18446744073709551616").unwrap(),
        18446744073709551616.0
    );
    assert_eq!(
        from_str::<f64>("-9223372036854775809").unwrap(),
        -9223372036854775809.0
    );
    // and floats stay floats
    assert_eq!(
        from_str::<f64>("18446744073709551616.5").unwrap(),
        18446744073709551616.5
    );
    assert_eq!(
        from_str::<f64>("18446744073709551616e2").unwrap(),
        1844674407370955161600.0
    );
    // too large for 128 bits
    let huge = format!("{}0", u128::MAX);
    assert_eq!(from_str::<f64>(&huge).unwrap(), u128::MAX as f64 * 10.0);
    assert!(from_str::<u128>(&huge).is_err());

    // out of range for narrower types
    assert!(from_str::<u64>("18446744073709551616").is_err());

    let map: BTreeMap<u128, bool> = from_str(&format!(r#"{{"{}": true}}"#, u128::MAX)).unwrap();
    assert!(map[&u128::MAX]);

    // roundtrip
    let values = vec![u128::MAX, 0, 1 << 100];
    assert_eq!(
        from_str::<Vec<u128>>(&dialect::to_string(&values).unwrap()).unwrap(),
        values
    );
}

#[test]
fn test_internally_tagged_buffering() {
    use std::collections::HashMap;

    #[derive(Deserialize, PartialEq, Debug)]
    #[deser(tag = "type")]
    enum Message {
        Stats {
            // integer keys work because the keys are lexical atoms which
            // are retained when buffered.
            counts: HashMap<u32, u32>,
            // extension values are retained when buffered
            total: u128,
            label: Option<String>,
        },
    }

    let msg: Message = from_str(
        r#"{
            "counts": {"1": 10, "2": 20},
            "total": 340282366920938463463374607431768211455,
            "label": null,
            "type": "Stats"
        }"#,
    )
    .unwrap();
    let mut counts = HashMap::new();
    counts.insert(1, 10);
    counts.insert(2, 20);
    assert_eq!(
        msg,
        Message::Stats {
            counts,
            total: u128::MAX,
            label: None,
        }
    );
}

#[test]
fn test_enum_representations() {
    use std::collections::BTreeMap;

    #[derive(Deserialize, PartialEq, Debug)]
    #[deser(tag = "t", content = "c")]
    enum Adjacent {
        Counts(BTreeMap<u32, u32>),
    }

    // content is buffered as it comes before the tag
    let value: Adjacent = from_str(r#"{"c": {"1": 2}, "t": "Counts"}"#).unwrap();
    let mut counts = BTreeMap::new();
    counts.insert(1, 2);
    assert_eq!(value, Adjacent::Counts(counts));

    #[derive(Deserialize, PartialEq, Debug)]
    #[deser(untagged)]
    enum Value {
        Big(u128),
        Text(String),
        List(Vec<Value>),
    }

    let value: Value = from_str(r#"[340282366920938463463374607431768211455, "x", []]"#).unwrap();
    assert_eq!(
        value,
        Value::List(vec![
            Value::Big(u128::MAX),
            Value::Text("x".into()),
            Value::List(vec![])
        ])
    );

    #[derive(Deserialize, PartialEq, Debug)]
    enum External {
        Point(i32, i32),
        Name { first: String },
    }
    let value: Vec<External> =
        from_str(r#"[{"Point": [1, -2]}, {"Name": {"first": "x"}}]"#).unwrap();
    assert_eq!(
        value,
        vec![External::Point(1, -2), External::Name { first: "x".into() }]
    );
}

#[test]
fn test_from_slice() {
    use dialect::from_slice;

    let x: Vec<u32> = from_slice(b"[1, 2, 3]").unwrap();
    assert_eq!(x, vec![1, 2, 3]);

    // valid UTF-8 in plain and escaped strings
    let x: Vec<String> =
        from_slice("[\"\u{fc}ber\", \"\u{6c34}\\n\u{10151}\", \"\\u00e4\u{e4}\"]".as_bytes())
            .unwrap();
    assert_eq!(x, vec!["\u{fc}ber", "\u{6c34}\n\u{10151}", "\u{e4}\u{e4}"]);
    let map: std::collections::BTreeMap<String, u32> =
        from_slice("{\"\u{e4}\": 1}".as_bytes()).unwrap();
    assert_eq!(map["\u{e4}"], 1);

    // invalid UTF-8 in strings of all lengths, with and without escapes.
    // Under miri only the lengths which cover every offset in the 8 and 16
    // byte blocks of the scanner are checked.
    let max_len = if cfg!(miri) { 18 } else { 40 };
    for len in 0..max_len {
        let s = "a".repeat(len);
        for bad in [&b"\xff"[..], b"\xc3", b"\xe6\xb0", b"\xed\xa0\x80", b"\x80"] {
            for (prefix, suffix) in [("", ""), ("\\n", ""), ("", "\\n")] {
                let mut input = format!("\"{}{}", prefix, s).into_bytes();
                input.extend_from_slice(bad);
                input.extend_from_slice(format!("{}\"", suffix).as_bytes());
                let err = from_slice::<String>(&input).unwrap_err();
                assert!(err.to_string().contains("utf-8"), "{:?}: {}", input, err);
                // map keys too
                let mut key = b"{".to_vec();
                key.extend_from_slice(&input);
                key.extend_from_slice(b": 1}");
                assert!(from_slice::<std::collections::BTreeMap<String, u32>>(&key).is_err());
            }
        }
    }

    // a sequence split by an escape is invalid even if the escape decodes
    // to bytes that could complete it
    assert!(from_slice::<String>(b"\"\xc3\\u00a9\"").is_err());

    // non-ASCII bytes outside of strings are rejected (in JSON5 they can be
    // whitespace)
    if !DIALECT.json5 {
        assert!(from_slice::<Vec<u32>>(b"[1,\xc2\xa0 2]").is_err());
    }
    assert!(from_slice::<Vec<u32>>(b"[1,\xc2 2]").is_err());
    assert!(from_slice::<Vec<u32>>(b"[1]\xff").is_err());
    assert!(from_slice::<bool>(b"tru\xc3").is_err());
    assert!(from_slice::<u32>(b"1\xff").is_err());

    // from_str is unaffected
    let x: String = from_str("\"\u{fc}\"").unwrap();
    assert_eq!(x, "\u{fc}");
}

#[test]
fn test_borrowing() {
    use deser::adapters::Borrowed;
    use std::borrow::Cow;

    #[derive(deser::Deserialize, Debug)]
    struct Doc<'a> {
        name: &'a str,
        #[deser(as = Borrowed)]
        text: Cow<'a, str>,
        tags: Vec<&'a str>,
    }

    let json = r#"{"name": "demo", "text": "a \"quoted\" text", "tags": ["x", "y"]}"#;
    let doc: Doc = dialect::from_str(json).unwrap();
    assert_eq!(doc.name, "demo");
    // strings without escapes are slices of the input
    let range = json.as_bytes().as_ptr_range();
    assert!(range.contains(&doc.name.as_ptr()));
    assert_eq!(doc.tags, ["x", "y"]);
    // strings with escapes cannot be borrowed
    assert!(matches!(doc.text, Cow::Owned(_)));
    assert_eq!(doc.text, "a \"quoted\" text");

    // keys and values from byte slices borrow too
    let value: std::collections::BTreeMap<&str, &str> =
        dialect::from_slice(br#"{"a": "b"}"#).unwrap();
    assert_eq!(value["a"], "b");

    let err = dialect::from_str::<&str>(r#""\n""#).unwrap_err();
    assert!(err.to_string().contains("expected a borrowed string"));
}

#[test]
fn test_number_forms() {
    use deser::ext::Number;

    // integers of up to nine digits are parsed by the parser, everything
    // else continues (or starts again) out of line: longer integers, the
    // fractions and exponents of floats, leading zeros and numbers at the
    // end of the input
    let ints: Vec<i64> = from_str(
        "[0, -0, 7, -7, 12, -123, 99999999, 100000000, 123456789, -123456789,
          1234567890, -1234567890, 9007199254740993, -9223372036854775808]",
    )
    .unwrap();
    assert_eq!(
        ints,
        [
            0,
            0,
            7,
            -7,
            12,
            -123,
            99999999,
            100000000,
            123456789,
            -123456789,
            1234567890,
            -1234567890,
            9007199254740993,
            i64::MIN
        ]
    );
    assert_eq!(from_str::<u64>("18446744073709551615").unwrap(), u64::MAX);
    for (input, value) in [
        ("7", 7),
        ("-7", -7),
        ("12345678", 12345678),
        ("123456789", 123456789),
        ("-123456789", -123456789),
    ] {
        assert_eq!(from_str::<i64>(input).unwrap(), value, "{input}");
    }

    let floats: Vec<f64> = from_str(
        "[0.5, -0.5, 1.5, -12.25, 123456789.5, 1234567890.5, 1e3, -1E3, 12e-2,
          123456789e1, 3.0e2, 1.5E+2, 12345678.875e-3]",
    )
    .unwrap();
    assert_eq!(
        floats,
        [
            0.5,
            -0.5,
            1.5,
            -12.25,
            123456789.5,
            1234567890.5,
            1e3,
            -1e3,
            0.12,
            1234567890.0,
            300.0,
            150.0,
            12345.678875
        ]
    );
    let zero: f64 = from_str("-0.0").unwrap();
    assert!(zero == 0.0 && zero.is_sign_negative());
    assert_eq!(from_str::<f64>("12.5").unwrap(), 12.5);
    assert_eq!(from_str::<f64>("-1e2").unwrap(), -100.0);

    // the text of numbers starts at their sign
    let numbers: Vec<Number> = from_str("[12.50, -3.10e2, 7E0]").unwrap();
    let texts = numbers.iter().map(|x| x.as_str()).collect::<Vec<_>>();
    assert_eq!(texts, ["12.50", "-3.10e2", "7E0"]);
}

#[test]
fn test_exact_numbers() {
    use deser::ext::{BigInt, Decimal, Number};

    // floats are emitted as numbers, types that do not know about them get
    // the float value
    assert_eq!(dialect::from_str::<f64>("1.5e3").unwrap(), 1500.0);
    assert_eq!(dialect::from_str::<f32>("0.1").unwrap(), 0.1);
    assert_eq!(
        dialect::from_str::<Vec<f64>>("[1.0, -0.5, 2]").unwrap(),
        [1.0, -0.5, 2.0]
    );

    // decimals and numbers get the exact text
    let value: Decimal = dialect::from_str("123456789.123456789123456789").unwrap();
    assert_eq!(value.as_str(), "123456789.123456789123456789");
    let value: Number = dialect::from_str("1.50E+3").unwrap();
    assert_eq!(value.as_str(), "1.50E+3");
    assert_eq!(value.value(), 1500.0);

    // integers that do not fit into 128 bits
    let big = "123456789012345678901234567890123456789012345";
    let value: BigInt = dialect::from_str(big).unwrap();
    assert_eq!(value.to_string(), big);
    let value: f64 = dialect::from_str(big).unwrap();
    assert_eq!(value, 1.2345678901234567e44);

    // numbers roundtrip exactly through the serializer
    #[derive(deser::Deserialize, deser::Serialize)]
    struct Doc<'a> {
        value: Number<'a>,
        values: Vec<Number<'a>>,
    }
    let json = r#"{"value":0.10000000000000000001,"values":[1.0,1E-400,-0.00]}"#;
    let doc: Doc = dialect::from_str(json).unwrap();
    assert_eq!(dialect::to_string(&doc).unwrap(), json);

    // numbers are ignored like other values
    #[derive(deser::Deserialize)]
    struct Empty {}
    let _: Empty = dialect::from_str(r#"{"a": 1.5, "b": [2.5]}"#).unwrap();
}

#[test]
fn test_exact_number_text() {
    use deser::ext::Number;

    // the text of every number is retained, either as number or as float
    // whose text is the shortest representation of its value.
    let mut rng: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = |n: u64| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng % n
    };
    let check = |text: &str| {
        let number: Number = dialect::from_str(text).unwrap();
        assert_eq!(number.as_str(), text);
    };
    for text in [
        "0.0",
        "-0.0",
        "0.00",
        "1.0",
        "1.10",
        "0.0001",
        "0.00001",
        "0.00010",
        "100.0",
        "123456789012345.0",
        "12345678901234.5",
        "0.123456789012345",
        "0.1234567890123456",
        "1e5",
        "1E-5",
        "1.5e+300",
        "-0.5e-3",
        "18446744073709551615.5",
        "184467440737095516150.5",
    ] {
        check(text);
    }
    // miri is too slow for many iterations
    let iterations = if cfg!(miri) { 200 } else { 100_000 };
    for _ in 0..iterations {
        let mut text = String::new();
        if next(2) == 0 {
            text.push('-');
        }
        let int_len = next(20) as usize;
        if int_len == 0 {
            text.push('0');
        } else {
            text.push(char::from(b'1' + next(9) as u8));
            for _ in 1..int_len {
                text.push(char::from(b'0' + next(10) as u8));
            }
        }
        text.push('.');
        let zeros = if int_len == 0 { next(8) as usize } else { 0 };
        text.extend(std::iter::repeat_n('0', zeros));
        for _ in 0..1 + next(18) {
            let digit = if next(4) == 0 { 0 } else { next(10) as u8 };
            text.push(char::from(b'0' + digit));
        }
        if next(8) == 0 {
            text.push_str(&format!("e{}", next(40) as i32 - 20));
        }
        check(&text);
    }
}

#[test]
fn test_error_locations() {
    fn fails<'de, T: deser::de::Deserialize<'de> + std::fmt::Debug>(input: &'de str) -> String {
        from_str::<T>(input).unwrap_err().to_string()
    }

    // syntax errors
    if DIALECT.hjson {
        // `2 x]` is a string without quotes
        assert_eq!(
            fails::<Vec<u32>>("[1,\n 2 x]"),
            "InvalidType: unexpected string, expected u32 at line 2 column 2"
        );
        assert_eq!(
            fails::<Vec<u32>>("  \n  [:]"),
            "Syntax: unexpected colon at line 2 column 4"
        );
    } else {
        assert_eq!(
            fails::<Vec<u32>>("[1,\n 2 x]"),
            "Syntax: expected a comma at line 2 column 4"
        );
        assert_eq!(
            fails::<Vec<u32>>("  \n  @"),
            "Syntax: unexpected character at line 2 column 3"
        );
    }
    assert_eq!(
        fails::<Vec<u32>>("[1, , 2]"),
        "Syntax: unexpected comma at line 1 column 5"
    );
    assert_eq!(
        fails::<Vec<u32>>("[1, 2"),
        "EndOfFile: unexpected end of file at line 1 column 6"
    );

    // errors of the values
    assert_eq!(
        fails::<Vec<u32>>("[1,\n  true]"),
        "InvalidType: unexpected bool, expected u32 at line 2 column 3"
    );
    // columns are counted in characters
    assert_eq!(
        fails::<Vec<(String, u32)>>("[[\"äöü\", \"x\"]]"),
        "InvalidType: unexpected string, expected u32 at line 1 column 10"
    );

    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Point {
        x: u32,
        y: u32,
    }
    // missing fields are reported at the end of the map
    let err = from_str::<Point>("{\n  \"x\": 1\n}").unwrap_err();
    assert_eq!(
        (err.line(), err.column(), err.offset()),
        (Some(3), Some(1), Some(11))
    );

    // from_slice works on bytes
    let err = dialect::from_slice::<Vec<u32>>(b"[1,\n \"x\"]").unwrap_err();
    assert_eq!((err.line(), err.column()), (Some(2), Some(2)));
}

#[test]
fn test_limits() {
    use deser::de::Limits;

    let input = r#"{"a": [[1]], "b": "hello"}"#;
    let parse = |limits: Limits| {
        dialect::Deserializer::from_str(input)
            .deserialize_with::<deser::de::Recording, _>(|driver| driver.push_layer(limits))
            .map(|_| ())
            .map_err(|err| err.to_string())
    };
    assert_eq!(parse(Limits::new().max_depth(3).max_len(5)), Ok(()));
    assert_eq!(
        parse(Limits::new().max_depth(2)),
        Err("LimitExceeded: recursion limit exceeded at line 1 column 8".into())
    );
    assert_eq!(
        parse(Limits::new().max_len(4)),
        Err("LimitExceeded: string or bytes too long at line 1 column 19".into())
    );
}

#[test]
fn test_lexical_keys() {
    use std::collections::{BTreeMap, HashMap};

    // keys are lexical, they parse into the type of the key
    let map: BTreeMap<u16, bool> = from_str(r#"{"80": true, "443": false}"#).unwrap();
    assert_eq!(map, BTreeMap::from([(80, true), (443, false)]));
    let map: BTreeMap<bool, u8> = from_str(r#"{"true": 1, "false": 0}"#).unwrap();
    assert_eq!(map, BTreeMap::from([(true, 1), (false, 0)]));
    // keys are strict, the spellings of query strings are not booleans
    let err = from_str::<BTreeMap<bool, u8>>(r#"{"no": 0}"#).unwrap_err();
    assert_eq!(
        err.message(),
        "invalid value \"no\", expected bool (true or false)"
    );

    // keys without escapes are borrowed
    let input = String::from(r#"{"a": 1, "b": 2}"#);
    let map: HashMap<&str, u8> = from_str(&input).unwrap();
    assert_eq!(map["a"], 1);
    let key = *map.keys().find(|x| **x == "a").unwrap();
    assert!(input.as_bytes().as_ptr_range().contains(&key.as_ptr()));
    let map: HashMap<String, u8> = from_str(r#"{"b\u0020c": 2}"#).unwrap();
    assert_eq!(map["b c"], 2);

    // string values are strings, they are not parsed
    let err = from_str::<BTreeMap<String, u16>>(r#"{"port": "42"}"#).unwrap_err();
    assert_eq!(err.message(), "unexpected string, expected u16");
    let err = from_str::<BTreeMap<u16, u16>>(r#"{"http": 80}"#).unwrap_err();
    assert_eq!(err.message(), "invalid value \"http\", expected u16");
}

#[test]
fn test_duplicate_keys() {
    use deser::de::DuplicateKeys;
    use dialect::Deserializer;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Config {
        port: u16,
    }

    let json = r#"{"port": 80, "port": 81}"#;
    let err = from_str::<Config>(json).unwrap_err();
    assert_eq!(err.message(), "duplicate field `port`");
    // the error points at the value of the duplicate key
    assert_eq!((err.line(), err.column()), (Some(1), Some(22)));
    let config = Deserializer::from_str(json)
        .deserialize_with::<Config, _>(|driver| DuplicateKeys::Last.set(driver.state_mut()))
        .unwrap();
    assert_eq!(config, Config { port: 81 });
}

#[test]
fn test_flattened_internally_tagged() {
    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "type")]
    enum Tagged {
        Page { number: u32 },
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Query {
        a: u32,
        #[deser(flatten)]
        tagged: Tagged,
    }

    assert_eq!(
        from_str::<Query>(r#"{"a": 1, "number": 3, "type": "Page"}"#).unwrap(),
        Query {
            a: 1,
            tagged: Tagged::Page { number: 3 },
        }
    );
    // errors in buffered values point at their location
    let err = from_str::<Query>(r#"{"number": "3", "a": 1, "type": "Page"}"#).unwrap_err();
    assert_eq!(err.message(), "unexpected string, expected u32");
    assert_eq!(err.column(), Some(12));
}

#[test]
fn test_error_messages() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    enum Unit {
        A,
        B,
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    enum Data {
        A(u32),
        B { x: u32 },
        C,
    }

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct S {
        x: u32,
    }

    let msg = |err: deser::Error| err.message().to_string();
    assert_eq!(
        msg(from_str::<u8>("300").unwrap_err()),
        "invalid value 300, expected u8"
    );
    assert_eq!(
        msg(from_str::<u32>("-1").unwrap_err()),
        "invalid value -1, expected u32"
    );
    assert_eq!(
        msg(from_str::<Unit>(r#""D""#).unwrap_err()),
        "unknown variant `D` of Unit, expected `A` or `B`"
    );
    assert_eq!(
        msg(from_str::<Data>(r#""D""#).unwrap_err()),
        "unknown variant `D` of Data, expected one of `A`, `B`, `C`"
    );
    assert_eq!(
        msg(from_str::<f64>("1e400").unwrap_err()),
        // in Hjson numbers out of range are strings
        if DIALECT.hjson {
            "unexpected string, expected f64"
        } else {
            "number out of range"
        }
    );
    assert_eq!(
        msg(from_str::<String>(r#""\ud800""#).unwrap_err()),
        "lone surrogate in unicode escape in string"
    );
    assert_eq!(
        msg(from_str::<String>(r#""\1""#).unwrap_err()),
        "invalid escape in string"
    );
    assert_eq!(msg(from_str::<S>("{}").unwrap_err()), "missing field `x`");
}

#[test]
fn test_error_categories() {
    use deser::ErrorCategory;

    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Item {
        id: u16,
        name: String,
    }

    // the input is not well-formed (HTTP 400 Bad Request)
    let syntax = |input: &str| {
        let err = from_str::<Vec<Item>>(input).unwrap_err();
        assert_eq!(err.category(), ErrorCategory::Syntax, "{input}: {err}");
    };
    syntax(r#"[{"id": 1, "name": "a"}}"#);
    syntax(r#"[{"id": 1, "name": "a"}] x"#);
    syntax(r#"[{"id": 1, "name": "\u12"}]"#);
    let err = from_str::<Vec<Item>>(r#"[{"id": 1, "name": "a""#).unwrap_err();
    assert_eq!(err.category(), ErrorCategory::Eof, "{err}");
    let err = from_str::<Vec<Item>>("").unwrap_err();
    assert_eq!(err.category(), ErrorCategory::Eof, "{err}");

    // the input is well-formed but does not fit (HTTP 422 Unprocessable
    // Entity)
    let data = |input: &str| {
        let err = from_str::<Vec<Item>>(input).unwrap_err();
        assert_eq!(err.category(), ErrorCategory::Data, "{input}: {err}");
    };
    data(r#"[{"id": "1", "name": "a"}]"#);
    data(r#"[{"id": 70000, "name": "a"}]"#);
    data(r#"[{"id": 1}]"#);
    data(r#"{"id": 1, "name": "a"}"#);
}
