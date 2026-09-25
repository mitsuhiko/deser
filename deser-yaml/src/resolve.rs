//! Resolution of scalars to atoms.
//!
//! In YAML the type of a plain (unquoted) scalar is determined by its
//! content: `true` is a boolean, `42` an integer and so on.  The rules for
//! this differ between YAML versions.  Quoted and block scalars are always
//! strings unless they have an explicit tag.
use std::borrow::Cow;

use deser::ext::{Date, Datetime, ExtValue, Offset, Time};
use deser::Atom;

/// The YAML version that determines how plain scalars are resolved.
///
/// A document can declare its version with a `%YAML` directive.  The
/// version configured with
/// [`DeserializerConfig::version`](crate::DeserializerConfig::version) applies
/// to documents that do not declare a version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Version {
    /// YAML 1.1.
    ///
    /// In addition to what YAML 1.2 supports, `y`, `yes`, `on` (and their
    /// negations) are booleans, integers can be written in binary (`0b1010`),
    /// octal (`0777`) and base 60 (`1:30`) and numbers can contain `_` as
    /// digit separators.  Unlike YAML 1.2, `0o777` is a string.
    V1_1,
    /// YAML 1.2 with the core schema.
    #[default]
    V1_2,
}

const TAG_PREFIX: &str = "tag:yaml.org,2002:";

/// How a tag affects a scalar.
pub enum ScalarTag<'t> {
    /// The scalar is a string (the `!` tag or a quoted scalar).
    Str,
    /// A standard tag that determines the type.
    Standard(&'t str),
    /// A tag that is not known.  The value is a string, the tag is passed on.
    Custom,
}

/// Classifies a tag.
pub fn classify_tag(tag: &str) -> ScalarTag<'_> {
    if tag == "!" {
        return ScalarTag::Str;
    }
    match tag.strip_prefix(TAG_PREFIX) {
        Some(
            name @ ("str" | "int" | "float" | "bool" | "null" | "binary" | "timestamp" | "seq"
            | "map"),
        ) => ScalarTag::Standard(name),
        _ => ScalarTag::Custom,
    }
}

/// Returns `true` if the tag is a standard tag for a collection.
pub fn is_collection_tag(tag: &str, is_map: bool) -> Result<bool, &'static str> {
    match classify_tag(tag) {
        ScalarTag::Str => Ok(true),
        ScalarTag::Standard("seq") if !is_map => Ok(true),
        ScalarTag::Standard("map") if is_map => Ok(true),
        ScalarTag::Standard(_) => Err("tag does not apply to this kind of node"),
        _ => Ok(false),
    }
}

/// Resolves a plain scalar without tag.
#[inline]
pub fn resolve_plain(value: Cow<'_, str>, version: Version) -> Atom<'_> {
    match resolve_plain_str(&value, version) {
        Some(atom) => atom,
        None => Atom::Str(value),
    }
}

fn resolve_plain_str(s: &str, version: Version) -> Option<Atom<'static>> {
    let first = match s.as_bytes().first() {
        Some(&first) => first,
        None => return Some(Atom::Null),
    };
    match first {
        b'0'..=b'9' | b'+' | b'-' | b'.' => match version {
            Version::V1_2 => parse_core_int(s).or_else(|| parse_core_float(s)),
            Version::V1_1 => parse_yaml11_int(s).or_else(|| parse_yaml11_float(s)),
        },
        b'~' if s.len() == 1 => Some(Atom::Null),
        b'n' | b'N' | b't' | b'T' | b'f' | b'F' | b'y' | b'Y' | b'o' | b'O' => {
            parse_null(s).or_else(|| parse_bool(s, version))
        }
        _ => None,
    }
}

/// Resolves a scalar with a standard tag.
pub fn resolve_standard<'x>(
    name: &str,
    value: Cow<'x, str>,
    version: Version,
) -> Result<Atom<'x>, &'static str> {
    let s = &*value;
    match name {
        "str" => Ok(Atom::Str(value)),
        "null" => parse_null(s).ok_or("invalid !!null value"),
        "bool" => parse_bool(s, version).ok_or("invalid !!bool value"),
        "int" => match version {
            Version::V1_2 => parse_core_int(s),
            Version::V1_1 => parse_yaml11_int(s),
        }
        .ok_or("invalid !!int value"),
        "float" => match version {
            Version::V1_2 => parse_core_float(s).or_else(|| parse_core_int(s).map(int_to_float)),
            Version::V1_1 => {
                parse_yaml11_float(s).or_else(|| parse_yaml11_int(s).map(int_to_float))
            }
        }
        .ok_or("invalid !!float value"),
        "binary" => decode_base64(s)
            .map(|bytes| Atom::Bytes(Cow::Owned(bytes)))
            .ok_or("invalid !!binary value"),
        "timestamp" => parse_timestamp(s)
            .map(|value| Atom::Ext(ExtValue::owned(value)))
            .ok_or("invalid !!timestamp value"),
        _ => Err("tag does not apply to scalars"),
    }
}

/// Parses a YAML timestamp (<https://yaml.org/type/timestamp.html>).
///
/// Dates without time are local dates, timestamps without time zone are in
/// UTC.
pub fn parse_timestamp(s: &str) -> Option<Datetime> {
    let bytes = s.as_bytes();
    let mut pos = 0;
    // reads between `min` and `max` digits
    let number = |pos: &mut usize, min: usize, max: usize| -> Option<u32> {
        let len = bytes[*pos..]
            .iter()
            .take(max)
            .take_while(|x| x.is_ascii_digit())
            .count();
        if len < min {
            return None;
        }
        let rv = s[*pos..*pos + len].parse().ok()?;
        *pos += len;
        Some(rv)
    };

    let year = number(&mut pos, 4, 4)? as u16;
    let date_only = bytes.len() == 10;
    let expect =
        |pos: &mut usize, c: u8| -> Option<()> { (bytes.get(*pos) == Some(&c)).then(|| *pos += 1) };
    expect(&mut pos, b'-')?;
    let (min, max) = if date_only { (2, 2) } else { (1, 2) };
    let month = number(&mut pos, min, max)? as u8;
    expect(&mut pos, b'-')?;
    let day = number(&mut pos, min, max)? as u8;
    let date = Date { year, month, day };
    if !date.is_valid() {
        return None;
    }
    if date_only {
        return Some(Datetime::from(date));
    }

    match bytes.get(pos)? {
        b'T' | b't' => pos += 1,
        b' ' | b'\t' => {
            while let Some(b' ' | b'\t') = bytes.get(pos) {
                pos += 1;
            }
        }
        _ => return None,
    }
    let hour = number(&mut pos, 1, 2)? as u8;
    expect(&mut pos, b':')?;
    let minute = number(&mut pos, 2, 2)? as u8;
    expect(&mut pos, b':')?;
    let second = number(&mut pos, 2, 2)? as u8;
    let mut nanosecond = 0;
    if bytes.get(pos) == Some(&b'.') {
        pos += 1;
        let start = pos;
        while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        // digits beyond nanoseconds are truncated
        let digits = &s[start..pos.min(start + 9)];
        if !digits.is_empty() {
            nanosecond = digits.parse::<u32>().ok()? * 10u32.pow(9 - digits.len() as u32);
        }
    }
    let time = Time {
        hour,
        minute,
        second,
        nanosecond,
    };
    if !time.is_valid() {
        return None;
    }

    while let Some(b' ' | b'\t') = bytes.get(pos) {
        pos += 1;
    }
    let offset = match bytes.get(pos) {
        // timestamps without time zone are in UTC
        None => Offset::Z,
        Some(b'Z') => {
            pos += 1;
            Offset::Z
        }
        Some(&sign @ (b'+' | b'-')) => {
            pos += 1;
            let hours = number(&mut pos, 1, 2)?;
            let minutes = if bytes.get(pos) == Some(&b':') {
                pos += 1;
                number(&mut pos, 2, 2)?
            } else {
                0
            };
            if hours > 23 || minutes > 59 {
                return None;
            }
            let minutes = (hours * 60 + minutes) as i16;
            Offset::Custom {
                minutes: if sign == b'-' { -minutes } else { minutes },
            }
        }
        _ => return None,
    };
    if pos != bytes.len() {
        return None;
    }
    Some(Datetime {
        date: Some(date),
        time: Some(time),
        offset: Some(offset),
    })
}

#[test]
fn test_parse_timestamp() {
    let ts = |s: &str| parse_timestamp(s).map(|x| x.to_string());
    assert_eq!(ts("2002-12-14").as_deref(), Some("2002-12-14"));
    assert_eq!(
        ts("2001-12-14t21:59:43.10-05:00").as_deref(),
        Some("2001-12-14T21:59:43.1-05:00")
    );
    assert_eq!(
        ts("2001-12-14 21:59:43.10 -5").as_deref(),
        Some("2001-12-14T21:59:43.1-05:00")
    );
    assert_eq!(
        ts("2001-12-15 2:59:43.10").as_deref(),
        Some("2001-12-15T02:59:43.1Z")
    );
    assert_eq!(
        ts("2001-12-15T02:59:43.1Z").as_deref(),
        Some("2001-12-15T02:59:43.1Z")
    );
    assert_eq!(
        ts("2001-1-5 02:59:43").as_deref(),
        Some("2001-01-05T02:59:43Z")
    );
    for invalid in [
        "",
        "2002-12-1",
        "2002-13-14",
        "2002-12-14 ",
        "2002-12-14T25:00:00",
        "2002-12-14T02:59",
        "2002-12-14T02:59:43X",
        "2002-12-14T02:59:43+24",
        "02002-12-14",
    ] {
        assert!(parse_timestamp(invalid).is_none(), "{}", invalid);
    }
}

fn int_to_float(atom: Atom) -> Atom<'static> {
    Atom::F64(match atom {
        Atom::U64(value) => value as f64,
        Atom::I64(value) => value as f64,
        Atom::F64(value) => value,
        Atom::Ext(ref ext) => match (ext.downcast_ref::<u128>(), ext.downcast_ref::<i128>()) {
            (Some(&value), _) => value as f64,
            (_, Some(&value)) => value as f64,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    })
}

fn parse_null(s: &str) -> Option<Atom<'static>> {
    match s {
        "" | "~" | "null" | "Null" | "NULL" => Some(Atom::Null),
        _ => None,
    }
}

fn parse_bool(s: &str, version: Version) -> Option<Atom<'static>> {
    match s {
        "true" | "True" | "TRUE" => Some(Atom::Bool(true)),
        "false" | "False" | "FALSE" => Some(Atom::Bool(false)),
        "y" | "Y" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" if version == Version::V1_1 => {
            Some(Atom::Bool(true))
        }
        "n" | "N" | "no" | "No" | "NO" | "off" | "Off" | "OFF" if version == Version::V1_1 => {
            Some(Atom::Bool(false))
        }
        _ => None,
    }
}

/// Splits off an optional sign.  Returns `true` for negative numbers.
fn split_sign(s: &str) -> (bool, &str) {
    match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    }
}

/// Accumulates digits of a radix, skipping `_` if allowed.
///
/// Returns `None` if there are no digits or an invalid character.  The
/// value saturates to `None` in the magnitude if it overflows 128 bits, in
/// which case the approximate float value is returned as well.
fn accumulate(digits: &str, radix: u32, underscores: bool) -> Option<Magnitude> {
    let mut value = Some(0u128);
    let mut approx = 0f64;
    let mut seen = false;
    for c in digits.chars() {
        if c == '_' && underscores {
            continue;
        }
        let digit = c.to_digit(radix)?;
        seen = true;
        value = value
            .and_then(|v| v.checked_mul(radix as u128))
            .and_then(|v| v.checked_add(digit as u128));
        approx = approx * radix as f64 + digit as f64;
    }
    if value.is_none() && radix == 10 {
        // parsing the text rounds correctly
        approx = digits.replace('_', "").parse().unwrap_or(approx);
    }
    if seen {
        Some(Magnitude { value, approx })
    } else {
        None
    }
}

struct Magnitude {
    value: Option<u128>,
    approx: f64,
}

fn make_int(negative: bool, magnitude: Magnitude) -> Atom<'static> {
    match magnitude.value {
        Some(value) if !negative => match u64::try_from(value) {
            Ok(value) => Atom::U64(value),
            Err(_) => Atom::Ext(ExtValue::owned(value)),
        },
        Some(value) if value <= 1u128 << 63 => Atom::I64((value as i128).wrapping_neg() as i64),
        Some(value) if value <= 1u128 << 127 => {
            Atom::Ext(ExtValue::owned((value as i128).wrapping_neg()))
        }
        // integers that do not even fit into 128 bits are approximated
        _ => Atom::F64(if negative {
            -magnitude.approx
        } else {
            magnitude.approx
        }),
    }
}

/// YAML 1.2 core schema integers: `[-+]?[0-9]+`, `0o[0-7]+`, `0x[0-9a-fA-F]+`.
fn parse_core_int(s: &str) -> Option<Atom<'static>> {
    if let Some(rest) = s.strip_prefix("0o") {
        return accumulate(rest, 8, false).map(|m| make_int(false, m));
    }
    if let Some(rest) = s.strip_prefix("0x") {
        return accumulate(rest, 16, false).map(|m| make_int(false, m));
    }
    let (negative, digits) = split_sign(s);
    accumulate(digits, 10, false).map(|m| make_int(negative, m))
}

/// YAML 1.1 integers: binary, octal, decimal, hexadecimal and base 60, all
/// with an optional sign and `_` separators.
fn parse_yaml11_int(s: &str) -> Option<Atom<'static>> {
    let (negative, rest) = split_sign(s);
    let magnitude = if let Some(digits) = rest.strip_prefix("0b") {
        accumulate(digits, 2, true)?
    } else if let Some(digits) = rest.strip_prefix("0x") {
        accumulate(digits, 16, true)?
    } else if rest == "0" {
        accumulate(rest, 10, false)?
    } else if let Some(digits) = rest.strip_prefix('0') {
        accumulate(digits, 8, true)?
    } else if rest
        .as_bytes()
        .first()
        .is_some_and(|b| (b'1'..=b'9').contains(b))
    {
        if rest.contains(':') {
            parse_base60(rest)?
        } else {
            accumulate(rest, 10, true)?
        }
    } else {
        return None;
    };
    Some(make_int(negative, magnitude))
}

/// Parses the base 60 integer `[1-9][0-9_]*(:[0-5]?[0-9])+`.
fn parse_base60(s: &str) -> Option<Magnitude> {
    let mut parts = s.split(':');
    let mut rv = accumulate(parts.next()?, 10, true)?;
    for part in parts {
        if !is_base60_digit(part) {
            return None;
        }
        let digit = part.parse::<u8>().ok()?;
        rv.value = rv
            .value
            .and_then(|v| v.checked_mul(60))
            .and_then(|v| v.checked_add(digit as u128));
        rv.approx = rv.approx * 60.0 + digit as f64;
    }
    Some(rv)
}

fn is_base60_digit(s: &str) -> bool {
    matches!(s.as_bytes(), [b'0'..=b'9'] | [b'0'..=b'5', b'0'..=b'9'])
}

fn parse_special_float(s: &str) -> Option<Atom<'static>> {
    match s {
        ".nan" | ".NaN" | ".NAN" => return Some(Atom::F64(f64::NAN)),
        _ => {}
    }
    let (negative, rest) = split_sign(s);
    match rest {
        ".inf" | ".Inf" | ".INF" => Some(Atom::F64(if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        })),
        _ => None,
    }
}

/// Skips ASCII digits (and `_` if allowed) and returns the number of
/// digits.
fn skip_digits(bytes: &[u8], pos: &mut usize, underscores: bool) -> usize {
    let mut count = 0;
    while let Some(&b) = bytes.get(*pos) {
        if b.is_ascii_digit() {
            count += 1;
        } else if !(b == b'_' && underscores) {
            break;
        }
        *pos += 1;
    }
    count
}

/// Skips an exponent (`[eE][-+]?[0-9]+`) if there is one.  Returns `false`
/// if the exponent is malformed.
fn skip_exponent(bytes: &[u8], pos: &mut usize, sign_required: bool) -> bool {
    if !matches!(bytes.get(*pos), Some(b'e' | b'E')) {
        return true;
    }
    *pos += 1;
    if matches!(bytes.get(*pos), Some(b'-' | b'+')) {
        *pos += 1;
    } else if sign_required {
        return false;
    }
    skip_digits(bytes, pos, false) > 0
}

fn parse_float_text(s: &str, underscores: bool) -> Option<Atom<'static>> {
    let value = if underscores && s.contains('_') {
        s.replace('_', "").parse()
    } else {
        s.parse()
    };
    value.ok().map(Atom::F64)
}

/// YAML 1.2 core schema floats:
/// `[-+]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?` and the special
/// values.
fn parse_core_float(s: &str) -> Option<Atom<'static>> {
    if let Some(atom) = parse_special_float(s) {
        return Some(atom);
    }
    let bytes = s.as_bytes();
    let mut pos = usize::from(matches!(bytes.first(), Some(b'-' | b'+')));
    let int_digits = skip_digits(bytes, &mut pos, false);
    let mut frac_digits = 0;
    if bytes.get(pos) == Some(&b'.') {
        pos += 1;
        frac_digits = skip_digits(bytes, &mut pos, false);
    } else if int_digits == 0 {
        return None;
    }
    if int_digits == 0 && frac_digits == 0 {
        return None;
    }
    if !skip_exponent(bytes, &mut pos, false) || pos != bytes.len() {
        return None;
    }
    parse_float_text(s, false)
}

/// YAML 1.1 floats: `[-+]?[0-9][0-9_]*\.[0-9_]*([eE][-+][0-9]+)?`,
/// `[-+]?\.[0-9][0-9_]*([eE][-+][0-9]+)?`, base 60 floats and the special
/// values.
fn parse_yaml11_float(s: &str) -> Option<Atom<'static>> {
    if let Some(atom) = parse_special_float(s) {
        return Some(atom);
    }
    let (negative, rest) = split_sign(s);
    let bytes = rest.as_bytes();
    let mut pos = 0;
    match bytes.first() {
        Some(b'0'..=b'9') => {
            skip_digits(bytes, &mut pos, true);
            if bytes.get(pos) == Some(&b':') {
                return parse_base60_float(negative, rest);
            }
            if bytes.get(pos) != Some(&b'.') {
                return None;
            }
            pos += 1;
            skip_digits(bytes, &mut pos, true);
        }
        Some(b'.') if matches!(bytes.get(1), Some(b'0'..=b'9')) => {
            pos += 1;
            skip_digits(bytes, &mut pos, true);
        }
        _ => return None,
    }
    if !skip_exponent(bytes, &mut pos, true) || pos != bytes.len() {
        return None;
    }
    parse_float_text(s, true)
}

/// Parses `[0-9][0-9_]*(:[0-5]?[0-9])+\.[0-9_]*`.
fn parse_base60_float(negative: bool, s: &str) -> Option<Atom<'static>> {
    let (int_part, frac_part) = s.split_once('.')?;
    let mut parts = int_part.split(':');
    let mut value = accumulate(parts.next()?, 10, true)?.approx;
    let mut count = 0;
    for part in parts {
        if !is_base60_digit(part) {
            return None;
        }
        value = value * 60.0 + part.parse::<u8>().ok()? as f64;
        count += 1;
    }
    if count == 0 || !frac_part.bytes().all(|b| b.is_ascii_digit() || b == b'_') {
        return None;
    }
    let frac: f64 = format!("0.{}", frac_part.replace('_', "")).parse().ok()?;
    value += frac;
    Some(Atom::F64(if negative { -value } else { value }))
}

/// Decodes base64 as used by `!!binary`.  Whitespace is ignored.
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    fn value(b: u8) -> Option<u32> {
        Some(match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }

    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    let mut padding = 0;
    let mut count = 0;
    for b in s.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        count += 1;
        if b == b'=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return None;
        }
        acc = (acc << 6) | value(b)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if count % 4 != 0 || padding > 2 {
        return None;
    }
    Some(out)
}

#[test]
fn test_base64() {
    assert_eq!(decode_base64("").unwrap(), b"");
    assert_eq!(decode_base64("Zg==").unwrap(), b"f");
    assert_eq!(decode_base64("Zm8=").unwrap(), b"fo");
    assert_eq!(decode_base64("Zm9v").unwrap(), b"foo");
    assert_eq!(decode_base64("Zm9v\n YmFy").unwrap(), b"foobar");
    assert_eq!(decode_base64("Zm9"), None);
    assert_eq!(decode_base64("Z=9v"), None);
    assert_eq!(decode_base64("Zm9!"), None);
}

#[test]
fn test_big_ints() {
    assert_eq!(
        parse_core_int("18446744073709551616"),
        Some(Atom::Ext(ExtValue::owned(18446744073709551616u128)))
    );
    assert_eq!(
        parse_core_int("-9223372036854775808"),
        Some(Atom::I64(i64::MIN))
    );
    assert_eq!(
        parse_core_int("-9223372036854775809"),
        Some(Atom::Ext(ExtValue::owned(-9223372036854775809i128)))
    );
    assert_eq!(
        parse_core_int("-170141183460469231731687303715884105728"),
        Some(Atom::Ext(ExtValue::owned(i128::MIN)))
    );
    assert_eq!(
        parse_core_int("1000000000000000000000000000000000000000000"),
        Some(Atom::F64(1e42))
    );
}
