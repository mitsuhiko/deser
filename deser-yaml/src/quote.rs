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
/// version is YAML 1.1, by readers of YAML 1.1.
pub fn is_plain_safe(s: &str, compat: Version) -> bool {
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

/// A literal block scalar.
pub struct Literal<'a> {
    /// The lines of the content without the trailing line breaks.
    content: Cow<'a, str>,
    /// The number of trailing line breaks.
    trailing: usize,
    /// `true` if an indentation indicator is required.
    needs_indicator: bool,
}

impl<'a> Literal<'a> {
    /// Returns the literal block for a string if it can be written as one.
    pub fn new(s: &'a str) -> Option<Literal<'a>> {
        if !s.contains('\n') || !is_block_safe(s) {
            return None;
        }
        let content = s.trim_end_matches('\n');
        // the indentation is detected from the first line with content,
        // leading spaces (or lines of spaces) need an explicit indicator
        let needs_indicator = content
            .split('\n')
            .find(|line| !line.is_empty())
            .is_some_and(|line| line.starts_with(' '));
        Some(Literal {
            content: Cow::Borrowed(content),
            trailing: s.len() - content.len(),
            needs_indicator,
        })
    }

    /// Creates a literal block from lines without leading spaces.
    pub fn from_lines(lines: String) -> Literal<'static> {
        Literal {
            content: Cow::Owned(lines),
            trailing: 1,
            needs_indicator: false,
        }
    }

    /// Writes the header (`|` with indicators).
    ///
    /// `indicator` is the indentation of the content relative to the parent
    /// node, it's only written if required.
    pub fn write_header(&self, out: &mut String, indicator: usize) {
        out.push('|');
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
        for line in self.content.split('\n') {
            if !line.is_empty() {
                push_indent(out, indent);
                out.push_str(line);
            }
            out.push('\n');
        }
        for _ in 1..self.trailing {
            out.push('\n');
        }
    }
}

/// Writes spaces for an indentation.
pub fn push_indent(out: &mut String, indent: usize) {
    out.extend(std::iter::repeat_n(' ', indent));
}

/// Writes a float so that readers of YAML 1.1 and 1.2 read it as float.
///
/// YAML 1.1 requires a `.` in floats and a sign in exponents.
pub fn write_float(out: &mut String, value: f64) {
    if value.is_nan() {
        out.push_str(".nan");
    } else if value.is_infinite() {
        out.push_str(if value > 0.0 { ".inf" } else { "-.inf" });
    } else {
        let formatted = format!("{:?}", value);
        match formatted.split_once('e') {
            Some((mantissa, exponent)) => {
                out.push_str(mantissa);
                if !mantissa.contains('.') {
                    out.push_str(".0");
                }
                out.push('e');
                if !exponent.starts_with('-') {
                    out.push('+');
                }
                out.push_str(exponent);
            }
            None => out.push_str(&formatted),
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
            assert!(is_plain_safe(s, Version::V1_2), "{}", s);
        }
        for s in [
            "", " a", "a ", "- a", "-", "a: b", "a:", "a #b", "#a", "---", "...", "--- a", "<<",
            "true", "null", "~", "1", "1.5", "0x10", ".inf", "a\nb", "a\tb", "'a", "!a", "&a",
            "*a", "%a", "@a", "`a", "|", ">",
        ] {
            assert!(!is_plain_safe(s, Version::V1_2), "{:?}", s);
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
            assert!(is_plain_safe(s, Version::V1_2), "{}", s);
            assert!(!is_plain_safe(s, Version::V1_1), "{}", s);
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
