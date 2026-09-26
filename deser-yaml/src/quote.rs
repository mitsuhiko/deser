//! Decides how scalars are written.
use std::borrow::Cow;
use std::fmt::Write;

use crate::resolve::{Version, is_plain_str, is_yaml11_implicit};

/// The longest simple (implicit) key the YAML specification allows.
pub const MAX_SIMPLE_KEY_LEN: usize = 1024;

/// Returns `true` if the character is printable in YAML.
fn is_printable(c: char) -> bool {
    matches!(c,
        '\t' | '\n' | '\r' | '\u{20}'..='\u{7e}' | '\u{85}' | '\u{a0}'..='\u{d7ff}'
        | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

/// Returns `true` if the character can only be written escaped in a
/// double-quoted scalar.
///
/// Besides non-printable characters these are line breaks (which fold in
/// plain and single-quoted scalars), tabs and the byte order mark.
fn needs_escape(c: char) -> bool {
    !is_printable(c)
        || matches!(
            c,
            '\n' | '\r' | '\t' | '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{feff}'
        )
}

/// Returns `true` if the string can be written as literal block scalar.
fn is_block_safe(s: &str) -> bool {
    s.chars().all(|c| {
        is_printable(c) && !matches!(c, '\r' | '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{feff}')
    })
}

/// Returns `true` if a string can be written as plain scalar.
///
/// It must not contain anything that has a meaning in YAML, and it must be
/// resolved as string by readers of YAML 1.2 and, if the compatibility
/// version is YAML 1.1, by readers of YAML 1.1.  In flow collections the
/// flow indicators are not allowed either.
pub fn is_plain_safe(s: &str, compat: Version, flow: bool) -> bool {
    let bytes = s.as_bytes();
    let (Some(&first), Some(&last)) = (bytes.first(), bytes.last()) else {
        return false;
    };
    if first == b' ' || last == b' ' {
        return false;
    }
    match first {
        // indicators, `-`, `?` and `:` are fine if followed by a character
        b'-' | b'?' | b':' => {
            if matches!(bytes.get(1), None | Some(b' ')) {
                return false;
            }
        }
        b',' | b'[' | b']' | b'{' | b'}' | b'#' | b'&' | b'*' | b'!' | b'|' | b'>' | b'\''
        | b'"' | b'%' | b'@' | b'`' => return false,
        _ => {}
    }
    if s.chars().any(needs_escape) {
        return false;
    }
    if flow && s.contains([',', '[', ']', '{', '}']) {
        return false;
    }
    // YAML 1.1 readers (like PyYAML) end plain scalars in flow collections
    // at `:` and `?`
    if flow && compat == Version::V1_1 && s.contains([':', '?']) {
        return false;
    }
    // mapping values and comments, `a:b` and `a#b` are fine
    if s.contains(": ") || s.ends_with(':') || s.contains(" #") {
        return false;
    }
    // document markers
    if (s.starts_with("---") || s.starts_with("...")) && matches!(bytes.get(3), None | Some(b' ')) {
        return false;
    }
    // merge keys
    if s == "<<" {
        return false;
    }
    if !is_plain_str(s, Version::V1_2) {
        return false;
    }
    if compat == Version::V1_1 && (!is_plain_str(s, Version::V1_1) || is_yaml11_implicit(s)) {
        return false;
    }
    true
}

/// Returns `true` if a string can be written single-quoted.
pub fn is_single_quote_safe(s: &str) -> bool {
    !s.chars().any(needs_escape)
}

/// Writes a single-quoted scalar.
pub fn write_single_quoted(out: &mut String, s: &str) {
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
}

/// Writes a double-quoted scalar.
pub fn write_double_quoted(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\0' => out.push_str("\\0"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{b}' => out.push_str("\\v"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '\u{1b}' => out.push_str("\\e"),
            '\u{85}' => out.push_str("\\N"),
            '\u{a0}' => out.push_str("\\_"),
            '\u{2028}' => out.push_str("\\L"),
            '\u{2029}' => out.push_str("\\P"),
            c if needs_escape(c) => match c as u32 {
                n @ 0..=0xff => write!(out, "\\x{:02x}", n).unwrap(),
                n @ 0x100..=0xffff => write!(out, "\\u{:04x}", n).unwrap(),
                n => write!(out, "\\U{:08x}", n).unwrap(),
            },
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The smallest width that folded lines are wrapped to, regardless of the
/// indentation.
const MIN_FOLD_WIDTH: usize = 20;

/// A block scalar (literal `|` or folded `>`).
pub struct BlockScalar<'a> {
    /// The content without the trailing line breaks.
    content: Cow<'a, str>,
    /// The number of trailing line breaks.
    trailing: usize,
    /// `true` if an indentation indicator is required.
    needs_indicator: bool,
    /// The width to fold lines at, `None` for literal block scalars.
    fold: Option<usize>,
}

impl<'a> BlockScalar<'a> {
    /// Returns a literal block scalar for a string if it can be written as
    /// one.
    pub fn literal(s: &'a str) -> Option<BlockScalar<'a>> {
        if s.is_empty() || !is_block_safe(s) {
            return None;
        }
        let content = s.trim_end_matches('\n');
        // the indentation is detected from the first line with content,
        // leading spaces (or lines of spaces) need an explicit indicator
        let needs_indicator = content
            .split('\n')
            .find(|line| !line.is_empty())
            .is_some_and(|line| line.starts_with(' '));
        Some(BlockScalar {
            content: Cow::Borrowed(content),
            trailing: s.len() - content.len(),
            needs_indicator,
            fold: None,
        })
    }

    /// Returns a folded block scalar for a string if it can be written as
    /// one.
    ///
    /// Folding only changes the layout if the reader restores the string
    /// exactly: lines are only broken at single spaces between other
    /// characters, lines must not start or end with whitespace (these are not
    /// folded by readers) and the string must not start with a line break.
    pub fn folded(s: &'a str, width: usize) -> Option<BlockScalar<'a>> {
        let mut rv = BlockScalar::literal(s)?;
        let foldable = !s.starts_with('\n')
            && rv
                .content
                .split('\n')
                .all(|line| !line.starts_with([' ', '\t']) && !line.ends_with([' ', '\t']));
        if !foldable {
            return None;
        }
        rv.fold = Some(width);
        Some(rv)
    }

    /// Returns `true` if a single-line string is worth folding at the width.
    pub fn should_fold(s: &str, width: usize) -> bool {
        s.len() > width && fold_points(s).next().is_some()
    }

    /// Creates a literal block from lines without leading spaces.
    pub fn from_lines(lines: String) -> BlockScalar<'static> {
        BlockScalar {
            content: Cow::Owned(lines),
            trailing: 1,
            needs_indicator: false,
            fold: None,
        }
    }

    /// Writes the header (`|` or `>` with indicators).
    ///
    /// `indicator` is the indentation of the content relative to the parent
    /// node, it's only written if required.
    pub fn write_header(&self, out: &mut String, indicator: usize) {
        out.push(if self.fold.is_some() { '>' } else { '|' });
        if self.needs_indicator {
            write!(out, "{}", indicator).unwrap();
        }
        match self.trailing {
            0 => out.push('-'),
            // clipping keeps one line break but empty content has none
            1 if !self.content.is_empty() => {}
            _ => out.push('+'),
        }
    }

    /// Writes the lines of the content, each ending with a line break.
    pub fn write_body(&self, out: &mut String, indent: usize) {
        if self.content.is_empty() {
            // only line breaks (kept with the `+` indicator)
            for _ in 0..self.trailing {
                out.push('\n');
            }
            return;
        }
        let mut prev_empty = true;
        for (idx, line) in self.content.split('\n').enumerate() {
            // in folded scalars a line break between two lines is folded
            // into a space, a line break needs an additional empty line.
            if idx > 0 && self.fold.is_some() && !prev_empty {
                out.push('\n');
            }
            prev_empty = line.is_empty();
            if !line.is_empty() {
                push_indent(out, indent);
                match self.fold {
                    Some(width) => write_folded_line(out, line, indent, width),
                    None => out.push_str(line),
                }
            }
            out.push('\n');
        }
        for _ in 1..self.trailing {
            out.push('\n');
        }
    }
}

/// Returns the positions of the spaces a line can be folded at: single
/// spaces between other characters.
fn fold_points(line: &str) -> impl Iterator<Item = usize> + '_ {
    let bytes = line.as_bytes();
    (1..bytes.len().saturating_sub(1)).filter(move |&idx| {
        bytes[idx] == b' '
            && !bytes[idx - 1].is_ascii_whitespace()
            && !bytes[idx + 1].is_ascii_whitespace()
    })
}

/// Writes a line of a folded scalar, broken at spaces so that the lines do
/// not exceed the width if possible.
fn write_folded_line(out: &mut String, line: &str, indent: usize, width: usize) {
    let available = width.saturating_sub(indent).max(MIN_FOLD_WIDTH);
    let mut start = 0;
    let mut last_point = None;
    for point in fold_points(line) {
        if point - start > available
            && let Some(last) = last_point
        {
            out.push_str(&line[start..last]);
            out.push('\n');
            push_indent(out, indent);
            start = last + 1;
        }
        last_point = Some(point);
    }
    if line.len() - start > available
        && let Some(last) = last_point
        && last > start
    {
        out.push_str(&line[start..last]);
        out.push('\n');
        push_indent(out, indent);
        start = last + 1;
    }
    out.push_str(&line[start..]);
}

/// Writes spaces for an indentation.
pub fn push_indent(out: &mut String, indent: usize) {
    out.extend(std::iter::repeat_n(' ', indent));
}

/// The floats that are written (`f32` and `f64`).
#[cfg(feature = "speedups")]
pub trait Float: zmij::Float + deser::__float::Float {}

#[cfg(feature = "speedups")]
impl<F: zmij::Float + deser::__float::Float> Float for F {}

/// The floats that are written (`f32` and `f64`).
#[cfg(not(feature = "speedups"))]
pub trait Float: deser::__float::Float {}

#[cfg(not(feature = "speedups"))]
impl<F: deser::__float::Float> Float for F {}

/// Writes a float so that readers of YAML 1.1 and 1.2 read it as float.
///
/// YAML 1.1 requires a `.` in floats and a sign in exponents.  The text is
/// the shortest that reads back as the same value of its type (`f32` or
/// `f64`).
pub fn write_float<W: Write, F: Float>(out: &mut W, value: F) {
    let wide = value.to_f64();
    if wide.is_nan() {
        out.write_str(".nan").unwrap();
    } else if wide.is_infinite() {
        out.write_str(if wide > 0.0 { ".inf" } else { "-.inf" })
            .unwrap();
    } else {
        #[cfg(feature = "speedups")]
        let mut buffer = zmij::Buffer::new();
        #[cfg(feature = "speedups")]
        let formatted = buffer.format_finite(value);
        #[cfg(not(feature = "speedups"))]
        let formatted = &deser::__float::format_finite(value);
        // the exponent always has a sign, the mantissa needs a `.`
        match formatted.split_once('e') {
            Some((mantissa, exponent)) if !mantissa.contains('.') => {
                out.write_str(mantissa).unwrap();
                out.write_str(".0e").unwrap();
                out.write_str(exponent).unwrap();
            }
            _ => out.write_str(formatted).unwrap(),
        }
    }
}

/// Returns `true` if the character can be part of a tag shorthand suffix.
fn is_tag_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-#;/?:@&=+$_.~*'()".contains(c)
}

/// Returns `true` if the character can be part of a verbatim tag.
fn is_uri_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-#;/?:@&=+$,_.!~*'()[]".contains(c)
}

const YAML_TAG_PREFIX: &str = "tag:yaml.org,2002:";

/// Writes a tag, using the `!!` shorthand for tags of yaml.org, the tag
/// itself for local tags and the verbatim form for all others.
pub fn write_tag(out: &mut String, tag: &str) {
    if let Some(suffix) = tag.strip_prefix(YAML_TAG_PREFIX)
        && !suffix.is_empty()
        && suffix.chars().all(is_tag_char)
    {
        out.push_str("!!");
        out.push_str(suffix);
    } else if tag == "!" {
        out.push('!');
    } else if let Some(suffix) = tag.strip_prefix('!')
        && !suffix.is_empty()
        && suffix.chars().all(is_tag_char)
    {
        out.push_str(tag);
    } else {
        out.push_str("!<");
        for c in tag.chars() {
            if is_uri_char(c) || (c == '%') {
                out.push(c);
            } else {
                let mut buf = [0; 4];
                for byte in c.encode_utf8(&mut buf).bytes() {
                    write!(out, "%{:02X}", byte).unwrap();
                }
            }
        }
        out.push('>');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain() {
        for s in [
            "hello", "a b", "-a", "?a", "::1", "a:b", "a#b", "a,b", "yes!", "2001-1",
        ] {
            assert!(is_plain_safe(s, Version::V1_2, false), "{}", s);
        }
        for s in [
            "", " a", "a ", "- a", "-", "a: b", "a:", "a #b", "#a", "---", "...", "--- a", "<<",
            "true", "null", "~", "1", "1.5", "0x10", ".inf", "a\nb", "a\tb", "'a", "!a", "&a",
            "*a", "%a", "@a", "`a", "|", ">",
        ] {
            assert!(!is_plain_safe(s, Version::V1_2, false), "{:?}", s);
        }
        // strings that are only special in YAML 1.1
        for s in [
            "yes",
            "on",
            "Off",
            "y",
            "0b10",
            "1:30",
            "1_000",
            "2001-12-14",
            "=",
        ] {
            assert!(is_plain_safe(s, Version::V1_2, false), "{}", s);
            assert!(!is_plain_safe(s, Version::V1_1, false), "{}", s);
        }
    }

    #[test]
    fn test_plain_in_flow() {
        for s in ["a,b", "a[b", "a]", "a{", "a}"] {
            assert!(is_plain_safe(s, Version::V1_2, false), "{}", s);
            assert!(!is_plain_safe(s, Version::V1_2, true), "{}", s);
        }
        for s in [":a", "?a", "a?", "a:b"] {
            assert!(is_plain_safe(s, Version::V1_2, true), "{}", s);
            assert!(!is_plain_safe(s, Version::V1_1, true), "{}", s);
        }
    }

    #[test]
    fn test_float() {
        let f = |value: f64| {
            let mut out = String::new();
            write_float(&mut out, value);
            out
        };
        assert_eq!(f(1.0), "1.0");
        assert_eq!(f(1e20), "1.0e+20");
        assert_eq!(f(1.5e-7), "1.5e-7");
        assert_eq!(f(-0.0), "-0.0");
        assert_eq!(f(f64::NAN), ".nan");
        assert_eq!(f(f64::NEG_INFINITY), "-.inf");
    }

    #[test]
    fn test_tags() {
        let t = |tag: &str| {
            let mut out = String::new();
            write_tag(&mut out, tag);
            out
        };
        assert_eq!(t("tag:yaml.org,2002:set"), "!!set");
        assert_eq!(t("!color"), "!color");
        assert_eq!(t("!"), "!");
        assert_eq!(
            t("tag:example.com,2000:app/foo"),
            "!<tag:example.com,2000:app/foo>"
        );
        assert_eq!(t("!a b"), "!<!a%20b>");
    }
}
