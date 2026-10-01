//! The syntax of JSON5 other than comments and trailing commas.
use std::collections::BTreeMap;

use deser::Deserialize;
use deser::ext::Decimal;
use deser_location::Spanned;

use super::common::{NEWLINE, STOP, STRICT, check_stream};
use super::dialect;
use dialect::{DeserializerConfig, from_slice, from_str};

/// The example from <https://json5.org/>.
const EXAMPLE: &str = r#"{
  // comments
  unquoted: 'and you can quote me on that',
  singleQuotes: 'I can use "double quotes" here',
  lineBreaks: "Look, Mom! \
No \\n's!",
  hexadecimal: 0xdecaf,
  leadingDecimalPoint: .8675309, andTrailing: 8675309.,
  positiveSign: +1,
  trailingComma: 'in objects', andIn: ['arrays',],
  "backwardsCompatible": "with JSON",
}"#;

#[test]
fn test_example() {
    #[derive(Deserialize, Debug)]
    #[deser(rename_all = "camelCase")]
    struct Example<'a> {
        unquoted: &'a str,
        single_quotes: &'a str,
        line_breaks: String,
        hexadecimal: u32,
        leading_decimal_point: f64,
        and_trailing: f64,
        positive_sign: i32,
        trailing_comma: &'a str,
        and_in: Vec<&'a str>,
        backwards_compatible: &'a str,
    }

    let example: Example = from_str(EXAMPLE).unwrap();
    assert_eq!(example.unquoted, "and you can quote me on that");
    assert_eq!(example.single_quotes, r#"I can use "double quotes" here"#);
    assert_eq!(example.line_breaks, r"Look, Mom! No \n's!");
    assert_eq!(example.hexadecimal, 0xdecaf);
    assert_eq!(example.leading_decimal_point, 0.8675309);
    assert_eq!(example.and_trailing, 8675309.0);
    assert_eq!(example.positive_sign, 1);
    assert_eq!(example.trailing_comma, "in objects");
    assert_eq!(example.and_in, ["arrays"]);
    assert_eq!(example.backwards_compatible, "with JSON");

    // the same from bytes
    let example: Example = from_slice(EXAMPLE.as_bytes()).unwrap();
    assert_eq!(example.hexadecimal, 0xdecaf);
}

#[test]
fn test_identifiers_are_borrowed() {
    let map: BTreeMap<&str, u32> = from_str("{a: 1, $b: 2, _c3: 3, 'd': 4, ünïcödé: 5}").unwrap();
    assert_eq!(
        map.keys().copied().collect::<Vec<_>>(),
        ["$b", "_c3", "a", "d", "ünïcödé"]
    );
}

#[test]
fn test_non_finite() {
    let values: Vec<f64> = from_str("[Infinity, -Infinity, +Infinity, NaN]").unwrap();
    assert_eq!(
        values[..3],
        [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY]
    );
    assert!(values[3].is_nan());
}

#[test]
fn test_non_finite_roundtrip() {
    let values = vec![1.5, f64::INFINITY, f64::NEG_INFINITY, f64::NAN];
    let json5 = dialect::to_string(&values).unwrap();
    assert_eq!(json5, "[1.5,Infinity,-Infinity,NaN]");
    let back: Vec<f64> = from_str(&json5).unwrap();
    assert_eq!(back[..3], values[..3]);
    assert!(back[3].is_nan());

    let values = vec![f32::NEG_INFINITY, f32::NAN];
    assert_eq!(dialect::to_string(&values).unwrap(), "[-Infinity,NaN]");

    let mut out = Vec::new();
    dialect::to_writer(&mut out, &values).unwrap();
    assert_eq!(out, b"[-Infinity,NaN]");

    // a configuration has to enable it, the default writes JSON
    let config = dialect::SerializerConfig::new();
    assert_eq!(config.to_string(&values).unwrap(), "[null,null]");
    let config = config.non_finite_floats(true);
    assert_eq!(config.to_string(&values).unwrap(), "[-Infinity,NaN]");
    let config = config.pretty(dialect::Indent::Spaces(2));
    assert_eq!(
        config.to_string(&values).unwrap(),
        "[\n  -Infinity,\n  NaN\n]"
    );
    let mut serializer = dialect::Serializer::with_config(&config);
    serializer.serialize(&f64::NAN).unwrap();
    assert_eq!(serializer.finish(), "NaN");
}

#[test]
fn test_exact_numbers() {
    // the text of numbers is passed on in the syntax of JSON
    let value: Vec<Decimal> = from_str("[+1.50, .50, 5.e2, -.1e-1]").unwrap();
    let texts: Vec<&str> = value.iter().map(|value| value.as_str()).collect();
    assert_eq!(texts, ["1.50", "0.50", "5e2", "-0.1e-1"]);
}

#[test]
fn test_strings() {
    let value: String = from_str(r"'\x41\u0042\'\v\0'").unwrap();
    assert_eq!(value, "AB'\x0b\0");
    let value: String = from_str("'tab\there'").unwrap();
    assert_eq!(value, "tab\there");
    assert!(from_str::<String>("'line\nbreak'").is_err());
    assert!(from_str::<String>(r"'\01'").is_err());
    assert!(from_slice::<String>(b"'\xff'").is_err());
}

#[test]
fn test_whitespace() {
    let value: Vec<u32> = from_str("\u{feff}[\u{a0}1,\u{2028}2\u{3000}]\u{2029}").unwrap();
    assert_eq!(value, [1, 2]);
    assert!(from_str::<Vec<u32>>("[1,\u{a1}2]").is_err());
    assert!(from_slice::<Vec<u32>>(b"[1,\xc2 2]").is_err());
}

#[test]
fn test_spans() {
    #[derive(Deserialize)]
    struct Doc {
        key: Spanned<String>,
        hex: Spanned<u32>,
    }

    let doc: Doc = DeserializerConfig::new()
        .track_locations(true)
        .from_str("{\n  key: 'välue', // c\n  hex: 0xFF,\n}")
        .unwrap();
    let span = |s: Option<deser_location::Span>| format!("{:?}", s.unwrap());
    assert_eq!(span(doc.key.span), "2:8-2:15");
    assert_eq!(span(doc.hex.span), "3:8-3:12");
}

#[test]
fn test_errors() {
    fn fails(input: &str) -> String {
        let err = from_str::<BTreeMap<String, f64>>(input).unwrap_err();
        format!(
            "{} at {}:{}",
            err.message(),
            err.line().unwrap(),
            err.column().unwrap()
        )
    }

    assert_eq!(fails("{a: b}"), "unexpected character at 1:5");
    assert_eq!(fails("{1: 2}"), "expected map key at 1:2");
    assert_eq!(fails("{a\n  b: 1}"), "expected colon at 2:3");
    assert_eq!(fails("{a: 0x}"), "expected a hex digit at 1:7");
    assert_eq!(fails("{a: Infinit}"), "unexpected character at 1:13");
}

#[test]
fn test_json5_stop() {
    check_stream(
        &STOP,
        "{ünï: 'a]\"}', b: [.5, +1, 0xFF, Infinity,],} 'x\\'y' -Infinity\u{a0}'}' {c: \"'\"}",
        5,
    );
}

#[test]
fn test_json5_strict() {
    check_stream(
        &STRICT,
        "\u{feff}// c\n{key: 'val\\\nue', n: -.5e1,}\u{2028}",
        1,
    );
}

#[test]
fn test_json5_newline() {
    check_stream(&NEWLINE, "'/*'\n{a: 'b\\\nc', d: 1}\n'e\\\r\nf'\n", 3);
}

#[test]
fn test_json5_scalars_before_unicode_whitespace() {
    check_stream(&STOP, "1\u{a0}2\u{2028}Infinity\u{3000}0x10\u{feff}", 4);
}
