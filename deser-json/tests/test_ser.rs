use deser::Serialize;
use deser_json::to_string;

#[test]
fn test_basic() {
    assert_eq!(to_string(&[1, 2, 3, 4]).unwrap(), "[1,2,3,4]");
}

#[test]
fn test_flatten() {
    #[derive(Serialize, PartialEq, Eq, Debug)]
    pub struct User {
        id: u64,
        #[deser(flatten)]
        attrs: Attrs,
    }

    #[derive(Serialize, PartialEq, Eq, Debug)]
    pub struct Attrs {
        is_active: bool,
        is_admin: bool,
        flags: Vec<String>,
    }

    let json = to_string(&User {
        id: 42,
        attrs: Attrs {
            is_active: true,
            is_admin: true,
            flags: vec!["german".into(), "staff".into()],
        },
    })
    .unwrap();
    assert_eq!(
        json,
        r#"{"id":42,"is_active":true,"is_admin":true,"flags":["german","staff"]}"#
    );
}

#[test]
fn test_char() {
    assert_eq!(to_string(&'a').unwrap(), r#""a""#);
    assert_eq!(to_string(&'"').unwrap(), r#""\"""#);
}

#[test]
fn test_map_keys() {
    use std::collections::BTreeMap;

    let mut map = BTreeMap::new();
    map.insert(42u32, 23u32);
    map.insert(1, 2);
    assert_eq!(to_string(&map).unwrap(), r#"{"1":2,"42":23}"#);

    let mut map = BTreeMap::new();
    map.insert(-1i32, true);
    assert_eq!(to_string(&map).unwrap(), r#"{"-1":true}"#);

    let mut map = BTreeMap::new();
    map.insert('x', 1u32);
    assert_eq!(to_string(&map).unwrap(), r#"{"x":1}"#);

    let map = BTreeMap::from([(false, 1u32), (true, 2)]);
    let json = to_string(&map).unwrap();
    assert_eq!(json, r#"{"false":1,"true":2}"#);
    assert_eq!(
        deser_json::from_str::<BTreeMap<bool, u32>>(&json).unwrap(),
        map
    );
}

#[test]
fn test_generics() {
    #[derive(Serialize)]
    pub struct Wrapper<T> {
        value: T,
    }

    #[derive(Serialize)]
    pub struct Newtype<T>(T);

    assert_eq!(
        to_string(&Wrapper { value: vec![1u32] }).unwrap(),
        r#"{"value":[1]}"#
    );
    assert_eq!(to_string(&Newtype(42u32)).unwrap(), "42");
}

#[test]
fn test_string_escapes() {
    assert_eq!(to_string(&"").unwrap(), r#""""#);
    assert_eq!(to_string(&"plain").unwrap(), r#""plain""#);
    assert_eq!(
        to_string(&"a \"quoted\" \\ string\nwith\tcontrol \x01 chars").unwrap(),
        r#""a \"quoted\" \\ string\nwith\tcontrol \u0001 chars""#
    );
    assert_eq!(to_string(&"\"").unwrap(), r#""\"""#);
    assert_eq!(
        to_string(&"日本語のテキスト\u{1f600}").unwrap(),
        "\"日本語のテキスト\u{1f600}\""
    );
}

#[test]
fn test_key_escapes() {
    let mut map = std::collections::BTreeMap::new();
    map.insert("plain", 1);
    map.insert("a \"quoted\" key", 2);
    map.insert("\n", 3);
    map.insert("日本語", 4);
    assert_eq!(
        to_string(&map).unwrap(),
        r#"{"\n":3,"a \"quoted\" key":2,"plain":1,"日本語":4}"#
    );
}

#[test]
fn test_string_lengths() {
    // strings of all lengths around the copy thresholds, with and without
    // escapes at the end.
    for len in 0..70 {
        let s: String = (0..len).map(|x| (b'a' + (x % 26) as u8) as char).collect();
        assert_eq!(to_string(&s).unwrap(), format!("\"{}\"", s));
        let escaped = format!("{}\n", s);
        assert_eq!(to_string(&escaped).unwrap(), format!("\"{}\\n\"", s));
        let values = vec![s.clone(), escaped.clone(), s.clone()];
        assert_eq!(
            to_string(&values).unwrap(),
            format!("[\"{}\",\"{}\\n\",\"{}\"]", s, s, s)
        );
    }
}

#[test]
fn test_nested_containers() {
    use std::collections::BTreeMap;

    assert_eq!(
        to_string(&vec![vec![1u32, 2], vec![], vec![3]]).unwrap(),
        "[[1,2],[],[3]]"
    );

    let mut inner = BTreeMap::new();
    inner.insert("x", vec![1u32]);
    inner.insert("y", vec![]);
    let mut map = BTreeMap::new();
    map.insert("a", inner.clone());
    map.insert("b", BTreeMap::new());
    map.insert("c", inner);
    assert_eq!(
        to_string(&vec![map]).unwrap(),
        r#"[{"a":{"x":[1],"y":[]},"b":{},"c":{"x":[1],"y":[]}}]"#
    );
}

#[test]
fn test_wide_integers() {
    assert_eq!(to_string(&42u128).unwrap(), "42");
    assert_eq!(to_string(&-42i128).unwrap(), "-42");
    assert_eq!(to_string(&u128::MAX).unwrap(), u128::MAX.to_string());
    assert_eq!(to_string(&i128::MIN).unwrap(), i128::MIN.to_string());

    let mut map = std::collections::BTreeMap::new();
    map.insert(u128::MAX, 1u32);
    assert_eq!(
        to_string(&map).unwrap(),
        format!(r#"{{"{}":1}}"#, u128::MAX)
    );
}

#[test]
fn test_well_known_types() {
    use deser::ext::{BigInt, Datetime, Decimal, Duration, Timestamp, Uuid};
    use std::collections::BTreeMap;

    // decimals and big integers are written as numbers
    let decimal: Decimal = "-12.50".parse().unwrap();
    assert_eq!(to_string(&decimal).unwrap(), "-12.50");
    let big: BigInt = "123456789012345678901234567890123456789012"
        .parse()
        .unwrap();
    assert_eq!(
        to_string(&big).unwrap(),
        "123456789012345678901234567890123456789012"
    );
    let mut map = BTreeMap::new();
    map.insert(big.clone(), decimal.clone());
    assert_eq!(
        to_string(&map).unwrap(),
        r#"{"123456789012345678901234567890123456789012":-12.50}"#
    );
    // numbers are passed on exactly
    let value: Decimal = deser_json::from_str("-12.50").unwrap();
    assert_eq!(value, decimal);
    let value: Decimal = deser_json::from_str("\"-12.50\"").unwrap();
    assert_eq!(value, decimal);
    // unless disabled
    let value: Decimal = deser_json::DeserializerConfig::new()
        .exact_numbers(false)
        .from_str("-12.50")
        .unwrap();
    assert_eq!(value.as_str(), "-12.5");
    let value: BigInt =
        deser_json::from_str("\"123456789012345678901234567890123456789012\"").unwrap();
    assert_eq!(value, big);

    // the others are written as strings
    let datetime: Datetime = "1979-05-27T07:32:00Z".parse().unwrap();
    assert_eq!(to_string(&datetime).unwrap(), r#""1979-05-27T07:32:00Z""#);
    let timestamp = Timestamp {
        seconds: 0,
        nanosecond: 0,
    };
    assert_eq!(to_string(&timestamp).unwrap(), r#""1970-01-01T00:00:00Z""#);
    let duration = Duration {
        seconds: 90,
        nanosecond: 0,
    };
    assert_eq!(to_string(&duration).unwrap(), r#""PT1M30S""#);
    let uuid: Uuid = "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap();
    assert_eq!(
        to_string(&uuid).unwrap(),
        r#""67e55044-10b1-426f-9247-bb680e5fe0c8""#
    );
    let value: Uuid = deser_json::from_str(r#""67e55044-10b1-426f-9247-bb680e5fe0c8""#).unwrap();
    assert_eq!(value, uuid);
}

#[test]
fn test_extension_fallback() {
    use deser::State;
    use deser::ext::{ExtValue, Extension};
    use deser::ser::Chunk;
    use deser::{Atom, Error};

    #[derive(Debug, Clone, PartialEq)]
    struct Timestamp(i64);

    impl Extension for Timestamp {
        fn name(&self) -> &str {
            "timestamp"
        }

        fn fallback(&self) -> Atom<'_> {
            Atom::I64(self.0)
        }
    }

    impl Serialize for Timestamp {
        fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
            Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(self))))
        }
    }

    assert_eq!(to_string(&vec![Timestamp(-1)]).unwrap(), "[-1]");
}

#[test]
fn test_float_precision() {
    // f32 values are written with the shortest text for their precision
    assert_eq!(to_string(&0.1f64).unwrap(), "0.1");
    assert_eq!(to_string(&0.5f32).unwrap(), "0.5");
    assert_eq!(to_string(&0.1f32).unwrap(), "0.1");
    assert_eq!(
        to_string(&f64::from(0.1f32)).unwrap(),
        "0.10000000149011612"
    );
    assert_eq!(
        to_string(&vec![1.1f32, 16777216.0, 3.4028235e38, 1e-45]).unwrap(),
        "[1.1,16777216.0,3.4028235e+38,1e-45]"
    );
    assert_eq!(deser_json::from_str::<f32>("0.1").unwrap(), 0.1f32);
    assert_eq!(to_string(&f32::NAN).unwrap(), "null");
    assert_eq!(to_string(&f32::INFINITY).unwrap(), "null");
}

#[test]
fn test_float_format() {
    // the output does not depend on the zmij feature
    for (value, expected) in [
        (0.0, "0.0"),
        (-0.0, "-0.0"),
        (1.0, "1.0"),
        (-1.5, "-1.5"),
        (0.1, "0.1"),
        (123.0, "123.0"),
        (1e15, "1000000000000000.0"),
        (1e16, "1e+16"),
        (1.5e16, "1.5e+16"),
        (1e300, "1e+300"),
        (0.001, "0.001"),
        (1e-5, "0.00001"),
        (1.5e-5, "0.000015"),
        (1e-7, "1e-7"),
        (1.25e-7, "1.25e-7"),
        (f64::MAX, "1.7976931348623157e+308"),
        (5e-324, "5e-324"),
        // exactly between two shortest candidates, the even one is used
        (-(1149636667324797.0 + 0.25), "-1149636667324797.2"),
        (165793407361858.0 + 0.125, "165793407361858.12"),
    ] {
        assert_eq!(to_string(&value).unwrap(), expected);
    }
    for (value, expected) in [
        (0.0f32, "0.0"),
        (-0.0, "-0.0"),
        (1.0, "1.0"),
        (0.1, "0.1"),
        (1e12, "1000000000000.0"),
        (1e13, "1e+13"),
        (1.5e13, "1.5e+13"),
        (1e-5, "0.00001"),
        (1e-6, "0.000001"),
        (1e-7, "1e-7"),
        (f32::MAX, "3.4028235e+38"),
        (1e-45, "1e-45"),
        // exactly between two shortest candidates, the even one is used
        (f32::from_bits(0x3980_0000), "0.00024414062"),
        (f32::from_bits(0x3b90_0000), "0.0043945312"),
    ] {
        assert_eq!(to_string(&value).unwrap(), expected);
    }
}

#[test]
fn test_serializer() {
    use deser::ser::{Layer, Next};
    use deser::{Atom, Error, Event};
    use deser_json::{Serializer, SerializerConfig, Trailing};

    // a single value by default
    let mut serializer = Serializer::new();
    serializer.serialize(&vec![1, 2]).unwrap();
    assert!(serializer.serialize(&3).is_err());
    assert_eq!(serializer.finish(), "[1,2]");

    // values are separated according to `trailing`
    let mut serializer = Serializer::with_config(&SerializerConfig::new().trailing(Trailing::Stop));
    serializer.serialize(&1).unwrap();
    serializer.serialize(&2).unwrap();
    assert_eq!(serializer.as_str(), "1\n2");
    let mut serializer =
        Serializer::with_config(&SerializerConfig::new().trailing(Trailing::Newline));
    serializer.serialize(&1).unwrap();
    // a value that fails writes nothing
    let invalid = std::collections::BTreeMap::from([(vec![1u32], 1u32)]);
    assert!(serializer.serialize(&invalid).is_err());
    serializer.serialize(&2).unwrap();
    assert_eq!(serializer.finish(), "1\n2\n");

    /// Writes all numbers as strings.
    struct NumbersAsStrings;

    impl Layer for NumbersAsStrings {
        fn event(&mut self, event: Event<'_>, next: &mut Next<'_>) -> Result<(), Error> {
            match event {
                Event::Atom(Atom::U64(value)) => next.emit(value.to_string().into()),
                event => next.emit(event),
            }
        }
    }

    let mut serializer = Serializer::new();
    serializer
        .serialize_with(&vec![1u64], |driver| driver.push_layer(NumbersAsStrings))
        .unwrap();
    assert_eq!(serializer.finish(), r#"["1"]"#);

    // the default configuration is the same as `new`
    assert_eq!(SerializerConfig::default(), SerializerConfig::new());
}

#[test]
fn test_implicit_text() {
    use deser::{Atom, Implicit, ImplicitValue};

    let write = |text: &'static str, value| {
        let recording: deser::de::Recording = record(Atom::Implicit(Implicit::new(text, value)));
        deser_json::to_string(&recording).unwrap()
    };
    // text that is the JSON literal of the value is kept
    assert_eq!(write("1.10", ImplicitValue::F64(1.1)), "1.10");
    assert_eq!(write("1e3", ImplicitValue::F64(1000.0)), "1e3");
    assert_eq!(write("-12", ImplicitValue::I64(-12)), "-12");
    assert_eq!(write("null", ImplicitValue::Null), "null");
    // other text is written as value
    assert_eq!(write("0x1F", ImplicitValue::U64(31)), "31");
    assert_eq!(write("+12", ImplicitValue::U64(12)), "12");
    assert_eq!(write("012", ImplicitValue::U64(12)), "12");
    assert_eq!(write("~", ImplicitValue::Null), "null");
    assert_eq!(write("True", ImplicitValue::Bool(true)), "true");
    assert_eq!(write(".inf", ImplicitValue::F64(f64::INFINITY)), "null");
    assert_eq!(write("1.0", ImplicitValue::U64(1)), "1");
    assert_eq!(write("1", ImplicitValue::F64(1.0)), "1.0");
    // text that is a different value is written as value
    assert_eq!(write("1.10", ImplicitValue::F64(2.0)), "2.0");
}

#[test]
fn test_recording() {
    use deser::de::Recording;

    let roundtrip = |json: &str| {
        let recording: Recording = deser_json::from_str(json).unwrap();
        assert_eq!(to_string(&recording).unwrap(), json);
    };
    roundtrip("1");
    roundtrip("[]");
    roundtrip("[1,2,3]");
    roundtrip(r#"{"a":1,"b":[]}"#);
    roundtrip(r#"[[],[1],{},{"a":[1,[2,{}]]},3]"#);
    roundtrip(r#"{"a":{"b":{"c":[1,{"d":null}]},"e":[[[]]]},"f":true}"#);

    // deeply nested values take time linear in the number of events (it
    // used to be quadratic in the depth, which took minutes here)
    let depth = if cfg!(miri) { 20 } else { 100_000 };
    roundtrip(&(r#"{"a":["#.repeat(depth) + &"]}".repeat(depth)));
    // nested values of all kinds, in and after deeply nested ones
    let depths: &[usize] = if cfg!(miri) { &[9] } else { &[9, 20] };
    for &depth in depths {
        let open = r#"{"a":[1,"s",{},[],{"b":[null]},"#;
        let close = r#",true],"c":{"d":[[]]}}"#;
        roundtrip(&format!(
            "[{}0{},[{}0{}],2]",
            open.repeat(depth),
            close.repeat(depth),
            open.repeat(depth / 2),
            close.repeat(depth / 2)
        ));
    }
}

/// Records a single atom.
fn record(atom: deser::Atom<'static>) -> deser::de::Recording {
    let mut out = None;
    {
        let mut driver = deser::de::DeserializeDriver::new(&mut out);
        driver.emit(atom).unwrap();
    }
    out.unwrap()
}
