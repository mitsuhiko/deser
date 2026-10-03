//! The arguments of the text opcodes of protocol 0 and integers.
//!
//! The text opcodes are read the way CPython's unpickler (`_pickle.c`)
//! reads them, including its quirks: lines are C strings (they end at the
//! first NUL byte) and `INT` tries `strtol` before Python's `int()`.
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use deser_core::ext::BigInt;

/// The maximum number of digits of integers in text (CPython's default
/// limit of `sys.set_int_max_str_digits`).
const MAX_DIGITS: usize = 4300;

/// An integer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Int {
    Small(i64),
    Big(BigInt),
}

/// Returns `true` for the bytes that are whitespace for C and Python.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Returns the line as C string: up to the first NUL byte.
pub(crate) fn c_str(line: &[u8]) -> &[u8] {
    match line.iter().position(|&c| c == 0) {
        Some(end) => &line[..end],
        None => line,
    }
}

/// Parses an integer like CPython's `PyLong_FromString` with base 10.
///
/// Whitespace around the integer and underscores between digits are
/// permitted.  Returns `None` for invalid text and integers with more than
/// 4300 digits.
pub(crate) fn parse_int(s: &[u8]) -> Option<Int> {
    let mut pos = 0;
    while pos < s.len() && is_space(s[pos]) {
        pos += 1;
    }
    let negative = match s.get(pos) {
        Some(b'+') => {
            pos += 1;
            false
        }
        Some(b'-') => {
            pos += 1;
            true
        }
        _ => false,
    };
    if s.get(pos) == Some(&b'_') {
        return None;
    }
    let mut digits = Vec::new();
    let mut prev = 0u8;
    while let Some(&c) = s.get(pos) {
        if c == b'_' {
            if prev == b'_' {
                return None;
            }
        } else if c.is_ascii_digit() {
            digits.push(c - b'0');
        } else {
            break;
        }
        prev = c;
        pos += 1;
    }
    if prev == b'_' || digits.is_empty() {
        return None;
    }
    while pos < s.len() && is_space(s[pos]) {
        pos += 1;
    }
    if pos != s.len() || digits.len() > MAX_DIGITS {
        return None;
    }
    Some(int_from_digits(&digits, negative))
}

/// Converts decimal digits to an integer.
fn int_from_digits(digits: &[u8], negative: bool) -> Int {
    let mut small: u64 = 0;
    let mut fits = true;
    for &d in digits {
        match small.checked_mul(10).and_then(|x| x.checked_add(d.into())) {
            Some(value) => small = value,
            None => {
                fits = false;
                break;
            }
        }
    }
    if fits {
        if !negative && small <= i64::MAX as u64 {
            return Int::Small(small as i64);
        }
        if negative && small <= i64::MAX as u64 + 1 {
            return Int::Small((small as i64).wrapping_neg());
        }
    }
    // little endian 32 bit limbs
    let mut limbs: Vec<u32> = vec![0];
    for &d in digits {
        let mut carry = u64::from(d);
        for limb in limbs.iter_mut() {
            let value = u64::from(*limb) * 10 + carry;
            *limb = value as u32;
            carry = value >> 32;
        }
        if carry != 0 {
            limbs.push(carry as u32);
        }
    }
    let mut magnitude = Vec::with_capacity(limbs.len() * 4);
    for limb in limbs.iter().rev() {
        magnitude.extend_from_slice(&limb.to_be_bytes());
    }
    big(negative, magnitude)
}

/// Creates an integer from its sign and big-endian magnitude.
fn big(negative: bool, magnitude: Vec<u8>) -> Int {
    let value = BigInt {
        negative,
        magnitude,
    };
    let significant = value.significant_magnitude();
    if significant.len() <= 8 {
        let mut buf = [0u8; 8];
        buf[8 - significant.len()..].copy_from_slice(significant);
        let small = u64::from_be_bytes(buf);
        if !negative && small <= i64::MAX as u64 {
            return Int::Small(small as i64);
        }
        if negative && small <= i64::MAX as u64 + 1 {
            return Int::Small((small as i64).wrapping_neg());
        }
    }
    if value.is_zero() {
        return Int::Small(0);
    }
    Int::Big(value)
}

/// Converts little-endian two's complement bytes (`LONG1` and `LONG4`).
pub(crate) fn int_from_le_bytes(bytes: &[u8]) -> Int {
    let Some(&last) = bytes.last() else {
        return Int::Small(0);
    };
    let negative = last & 0x80 != 0;
    if bytes.len() <= 8 {
        let mut buf = [if negative { 0xff } else { 0 }; 8];
        buf[..bytes.len()].copy_from_slice(bytes);
        return Int::Small(i64::from_le_bytes(buf));
    }
    let mut magnitude: Vec<u8> = bytes.iter().rev().copied().collect();
    if negative {
        // the magnitude of a negative number is its two's complement
        for byte in magnitude.iter_mut() {
            *byte = !*byte;
        }
        for byte in magnitude.iter_mut().rev() {
            let (value, overflow) = byte.overflowing_add(1);
            *byte = value;
            if !overflow {
                break;
            }
        }
    }
    big(negative, magnitude)
}

/// Reads the line of an `INT` opcode (with its newline).
///
/// `I01` and `I00` are `True` and `False` (returned as `Err(bool)`).
pub(crate) fn load_int(line: &[u8]) -> Option<Result<Int, bool>> {
    let s = c_str(line);
    // strtol(s, &end, 10)
    let mut pos = 0;
    while pos < s.len() && is_space(s[pos]) {
        pos += 1;
    }
    let negative = match s.get(pos) {
        Some(b'+') => {
            pos += 1;
            false
        }
        Some(b'-') => {
            pos += 1;
            true
        }
        _ => false,
    };
    let start = pos;
    let mut value: i64 = 0;
    let mut overflow = false;
    while let Some(&c) = s.get(pos) {
        if !c.is_ascii_digit() {
            break;
        }
        let digit = i64::from(c - b'0');
        let next = value.checked_mul(10).and_then(|x| match negative {
            true => x.checked_sub(digit),
            false => x.checked_add(digit),
        });
        match next {
            Some(next) => value = next,
            None => overflow = true,
        }
        pos += 1;
    }
    // without digits strtol does not consume anything
    let end = if pos == start { 0 } else { pos };
    if overflow || (end < s.len() && s[end] != b'\n') {
        return parse_int(s).map(Ok);
    }
    if line.len() == 3 && (value == 0 || value == 1) {
        return Some(Err(value == 1));
    }
    Some(Ok(Int::Small(value)))
}

/// Reads the line of a `LONG` opcode (with its newline).
pub(crate) fn load_long(line: &[u8]) -> Option<Int> {
    let len = line.len();
    let s = if line[len - 2] == b'L' {
        &line[..len - 2]
    } else {
        line
    };
    parse_int(c_str(s))
}

/// Reads the line of a `PUT` or `GET` opcode (with its newline).
pub(crate) fn load_index(line: &[u8]) -> Option<i64> {
    match parse_int(c_str(line))? {
        Int::Small(value) => Some(value),
        Int::Big(_) => None,
    }
}

/// Reads the line of a `FLOAT` opcode (with its newline) like CPython's
/// `PyOS_string_to_double`.
///
/// Returns `None` for invalid text and finite numbers that overflow.
pub(crate) fn load_float(line: &[u8]) -> Option<f64> {
    let s = c_str(line);
    let mut pos = 0;
    if matches!(s.first(), Some(b'+' | b'-')) {
        pos += 1;
    }
    let rest = &s[pos..];
    let special = [
        (&b"infinity"[..], f64::INFINITY),
        (&b"inf"[..], f64::INFINITY),
        (&b"nan"[..], f64::NAN),
    ];
    for (name, value) in special {
        if rest.len() >= name.len() && rest[..name.len()].eq_ignore_ascii_case(name) {
            let end = pos + name.len();
            if end < s.len() && s[end] != b'\n' {
                return None;
            }
            return Some(if s[0] == b'-' { -value } else { value });
        }
    }
    // hex floats are not accepted
    if rest.len() >= 2 && rest[0] == b'0' && matches!(rest[1], b'x' | b'X') {
        return None;
    }
    let digits = |pos: &mut usize| {
        let start = *pos;
        while s.get(*pos).is_some_and(u8::is_ascii_digit) {
            *pos += 1;
        }
        *pos - start
    };
    let mut count = digits(&mut pos);
    if s.get(pos) == Some(&b'.') {
        pos += 1;
        count += digits(&mut pos);
    }
    if count == 0 {
        return None;
    }
    if matches!(s.get(pos), Some(b'e' | b'E')) {
        let mut exp = pos + 1;
        if matches!(s.get(exp), Some(b'+' | b'-')) {
            exp += 1;
        }
        if digits(&mut exp) > 0 {
            pos = exp;
        }
    }
    if pos < s.len() && s[pos] != b'\n' {
        return None;
    }
    let value: f64 = core::str::from_utf8(&s[..pos]).ok()?.parse().ok()?;
    if value.is_infinite() {
        return None;
    }
    Some(value)
}

/// Decodes the escapes of the argument of a `STRING` opcode (the escapes of
/// Python's bytes literals).
pub(crate) fn decode_escape(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    let mut pos = 0;
    while pos < s.len() {
        let c = s[pos];
        pos += 1;
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let &c = s.get(pos)?;
        pos += 1;
        match c {
            b'\n' => {}
            b'\\' => out.push(b'\\'),
            b'\'' => out.push(b'\''),
            b'"' => out.push(b'"'),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b't' => out.push(b'\t'),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b'v' => out.push(0x0b),
            b'a' => out.push(0x07),
            b'0'..=b'7' => {
                let mut value = u32::from(c - b'0');
                for _ in 0..2 {
                    match s.get(pos) {
                        Some(&d @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(d - b'0');
                            pos += 1;
                        }
                        _ => break,
                    }
                }
                out.push(value as u8);
            }
            b'x' => {
                let hi = hex_digit(*s.get(pos)?)?;
                let lo = hex_digit(*s.get(pos + 1)?)?;
                pos += 2;
                out.push(hi << 4 | lo);
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    Some(out)
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Decodes the argument of a `UNICODE` opcode (Python's `raw_unicode_escape`
/// codec).
///
/// Returns `None` for invalid escapes and for surrogates which Python
/// accepts but Rust strings cannot hold.
pub(crate) fn decode_raw_unicode_escape(s: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;
    while pos < s.len() {
        let c = s[pos];
        pos += 1;
        if c != b'\\' || pos >= s.len() {
            out.push(char::from(c));
            continue;
        }
        let count = match s[pos] {
            b'u' => 4,
            b'U' => 8,
            other => {
                out.push('\\');
                out.push(char::from(other));
                pos += 1;
                continue;
            }
        };
        pos += 1;
        let mut value: u32 = 0;
        for _ in 0..count {
            value = value << 4 | u32::from(hex_digit(*s.get(pos)?)?);
            pos += 1;
        }
        out.push(char::from_u32(value)?);
    }
    Some(out)
}
