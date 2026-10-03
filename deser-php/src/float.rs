//! Reading and writing floats like PHP.
use alloc::format;
use alloc::string::String;
use core::fmt::Display;
use core::fmt::LowerExp;

/// Writes a float like PHP's `serialize` does.
///
/// PHP writes the shortest text that reads back as the same value: in
/// positional notation for exponents from -4 to 16 (without fraction for
/// integers) and in scientific notation with at least one fractional digit
/// otherwise (`1.0E+25`, `2.5E-7`).  The special values are `INF`, `-INF`
/// and `NAN`.
pub(crate) fn write_f64(value: f64, out: &mut String) {
    if value.is_nan() {
        out.push_str("NAN");
    } else if value.is_infinite() {
        out.push_str(if value > 0.0 { "INF" } else { "-INF" });
    } else {
        write_shortest(value, out);
    }
}

/// Writes a single precision float.
///
/// It's written with the shortest text that reads back as the same `f32`,
/// PHP reads it as the double closest to that text.
pub(crate) fn write_f32(value: f32, out: &mut String) {
    if value.is_nan() || value.is_infinite() {
        write_f64(value.into(), out);
    } else {
        write_shortest(value, out);
    }
}

/// Writes a finite float with its shortest digits.
fn write_shortest<F: LowerExp + Display>(value: F, out: &mut String) {
    // `{:e}` gives the shortest digits that read back as the same value,
    // for instance `-1.25e-7` or `1e100`
    let text = format!("{:e}", value);
    let (mantissa, exponent) = text.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    let (negative, mantissa) = match mantissa.strip_prefix('-') {
        Some(mantissa) => (true, mantissa),
        None => (false, mantissa),
    };
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    if negative {
        out.push('-');
    }
    if !(-4..17).contains(&exponent) {
        out.push_str(&digits[..1]);
        out.push('.');
        out.push_str(if digits.len() > 1 { &digits[1..] } else { "0" });
        out.push_str(if exponent < 0 { "E-" } else { "E+" });
        out.push_str(&format!("{}", exponent.unsigned_abs()));
    } else if exponent < 0 {
        out.push_str("0.");
        for _ in 0..(-exponent - 1) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let int_len = exponent as usize + 1;
        if digits.len() <= int_len {
            out.push_str(&digits);
            for _ in digits.len()..int_len {
                out.push('0');
            }
        } else {
            out.push_str(&digits[..int_len]);
            out.push('.');
            out.push_str(&digits[int_len..]);
        }
    }
}

/// Returns `true` if the text is a float of PHP's serialization format.
///
/// These are decimal numbers with an optional sign, fraction and exponent
/// where either the integer part or the fraction has digits (`5.` and `.5`
/// are floats) and the special values `INF`, `-INF` and `NAN`.
pub(crate) fn is_float(text: &[u8]) -> bool {
    if matches!(text, b"INF" | b"-INF" | b"NAN") {
        return true;
    }
    let text = match text {
        [b'+' | b'-', rest @ ..] => rest,
        _ => text,
    };
    let int_len = text.iter().take_while(|c| c.is_ascii_digit()).count();
    let mut rest = &text[int_len..];
    let mut frac_len = 0;
    if let [b'.', after @ ..] = rest {
        frac_len = after.iter().take_while(|c| c.is_ascii_digit()).count();
        rest = &after[frac_len..];
    }
    if int_len == 0 && frac_len == 0 {
        return false;
    }
    match rest {
        [] => true,
        [b'e' | b'E', exponent @ ..] => {
            let exponent = match exponent {
                [b'+' | b'-', digits @ ..] => digits,
                _ => exponent,
            };
            !exponent.is_empty() && exponent.iter().all(u8::is_ascii_digit)
        }
        _ => false,
    }
}

/// Parses a float that passed [`is_float`].
pub(crate) fn parse_float(text: &[u8]) -> f64 {
    match text {
        b"INF" => f64::INFINITY,
        b"-INF" => f64::NEG_INFINITY,
        b"NAN" => f64::NAN,
        // the syntax is a subset of Rust's
        _ => core::str::from_utf8(text)
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(f64::NAN),
    }
}

#[test]
fn test_write_f64() {
    fn write(value: f64) -> String {
        let mut out = String::new();
        write_f64(value, &mut out);
        out
    }
    assert_eq!(write(0.0), "0");
    assert_eq!(write(-0.0), "-0");
    assert_eq!(write(0.1), "0.1");
    assert_eq!(write(1.0), "1");
    assert_eq!(write(100.0), "100");
    assert_eq!(write(-1.5), "-1.5");
    assert_eq!(write(1e16), "10000000000000000");
    assert_eq!(write(1e17), "1.0E+17");
    assert_eq!(write(1.2345678901234568e17), "1.2345678901234568E+17");
    assert_eq!(write(0.0001), "0.0001");
    assert_eq!(write(0.00012), "0.00012");
    assert_eq!(write(1e-5), "1.0E-5");
    assert_eq!(write(2.5e-7), "2.5E-7");
    assert_eq!(write(5e-324), "5.0E-324");
    assert_eq!(write(f64::MAX), "1.7976931348623157E+308");
    assert_eq!(write(0.1 + 0.2), "0.30000000000000004");
    assert_eq!(write(f64::INFINITY), "INF");
    assert_eq!(write(f64::NEG_INFINITY), "-INF");
    assert_eq!(write(f64::NAN), "NAN");
}

#[test]
fn test_is_float() {
    for text in [
        "0", "-0", "+1.5", ".5", "5.", "-.5", "1e3", "1E3", "1.0E+25", "1.0E-25", ".5e1", "007.5",
        "INF", "-INF", "NAN",
    ] {
        assert!(is_float(text.as_bytes()), "{}", text);
    }
    for text in [
        "", ".", "-", "1e", "1e+", "+INF", "-NAN", "inf", "nan", "INFINITY", "0x10", " 1", "1_0",
    ] {
        assert!(!is_float(text.as_bytes()), "{}", text);
    }
}
