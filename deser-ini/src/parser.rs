//! Parses INI files and git's config files into a [`Document`].
use std::borrow::Cow;
use std::collections::HashMap;

use deser_core::{Error, ErrorKind};

use crate::de::DeserializerConfig;
use crate::{Continuation, InlineComments, Quotes, Syntax};

/// A byte range in the input.
pub(crate) type Range = (usize, usize);

/// What a node of the document is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeKind {
    /// The root, a section or a subsection.
    Table,
    /// A key with its values.
    Key,
}

/// A key or a table of the document.
pub(crate) struct Node<'a> {
    pub(crate) name: Cow<'a, str>,
    /// The range of the name where the node was created.
    pub(crate) range: Range,
    pub(crate) kind: NodeKind,
    /// The values of a key in the order in which they appear, `None` for a
    /// key without value.
    pub(crate) values: Vec<(Option<Cow<'a, str>>, Range)>,
    /// The keys and tables of a table in the order in which they appear.
    pub(crate) children: Vec<usize>,
}

/// The parsed input: the keys and sections as a tree.
///
/// Sections that are given more than once are merged, keys that are given
/// more than once have more than one value.
pub(crate) struct Document<'a> {
    pub(crate) nodes: Vec<Node<'a>>,
    lookup: HashMap<(usize, Cow<'a, str>), usize>,
}

impl<'a> Document<'a> {
    fn new(len: usize) -> Document<'a> {
        Document {
            nodes: vec![Node {
                name: Cow::Borrowed(""),
                range: (0, len),
                kind: NodeKind::Table,
                values: Vec::new(),
                children: Vec::new(),
            }],
            lookup: HashMap::new(),
        }
    }

    /// Returns the child of a table, creates it if needed.
    fn child(
        &mut self,
        parent: usize,
        name: Cow<'a, str>,
        range: Range,
        kind: NodeKind,
    ) -> Result<usize, Error> {
        let id = (parent, name);
        if let Some(&node) = self.lookup.get(&id) {
            if self.nodes[node].kind != kind {
                return Err(Error::with_offset(
                    ErrorKind::Syntax,
                    format!("`{}` is a key and a section", id.1),
                    range.0,
                ));
            }
            return Ok(node);
        }
        let node = self.nodes.len();
        self.nodes.push(Node {
            name: id.1.clone(),
            range,
            kind,
            values: Vec::new(),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(node);
        self.lookup.insert(id, node);
        Ok(node)
    }

    /// Returns a section (or subsection), creates it if needed.
    fn table(&mut self, parent: usize, name: Cow<'a, str>, range: Range) -> Result<usize, Error> {
        self.child(parent, name, range, NodeKind::Table)
    }

    /// Adds the value of a key.
    fn value(
        &mut self,
        table: usize,
        key: Cow<'a, str>,
        key_range: Range,
        value: Option<Cow<'a, str>>,
        value_range: Range,
    ) -> Result<(), Error> {
        let node = self.child(table, key, key_range, NodeKind::Key)?;
        self.nodes[node].values.push((value, value_range));
        Ok(())
    }
}

/// Parses the input.
pub(crate) fn parse<'a>(
    input: &'a str,
    config: &DeserializerConfig,
) -> Result<Document<'a>, Error> {
    // a byte order mark is skipped, the offsets stay those of the input
    let start = if input.starts_with('\u{feff}') { 3 } else { 0 };
    match config.syntax {
        Syntax::Ini => IniParser {
            input,
            config,
            lines: Lines { input, pos: start },
            doc: Document::new(input.len()),
        }
        .parse(),
        Syntax::Git => GitParser {
            bytes: input.as_bytes(),
            pos: start,
            eof: false,
            doc: Document::new(input.len()),
        }
        .parse(),
    }
}

/// Returns `true` for the whitespace of INI files.
#[inline]
fn is_ws(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// The lines of the input (ended by `\n`, `\r\n` or `\r`).
struct Lines<'a> {
    input: &'a str,
    pos: usize,
}

impl Iterator for Lines<'_> {
    /// The range of the line without its line break.
    type Item = Range;

    fn next(&mut self) -> Option<Range> {
        let bytes = self.input.as_bytes();
        if self.pos >= bytes.len() {
            return None;
        }
        let start = self.pos;
        let end = bytes[start..]
            .iter()
            .position(|&b| b == b'\n' || b == b'\r')
            .map_or(bytes.len(), |pos| start + pos);
        self.pos = match bytes.get(end) {
            Some(b'\r') if bytes.get(end + 1) == Some(&b'\n') => end + 2,
            Some(_) => end + 1,
            None => end,
        };
        Some((start, end))
    }
}

/// A key whose value can still be continued.
struct Pending<'a> {
    table: usize,
    key: Cow<'a, str>,
    key_range: Range,
    /// The indentation of the line of the key.
    indent: usize,
    value: Option<Cow<'a, str>>,
    value_range: Range,
    /// Empty lines since the last line of the value.
    blanks: usize,
    /// Continuation lines were added.
    continued: bool,
}

/// Where the text of a value is.
enum Bounds {
    /// Unquoted text.
    Plain(usize, usize),
    /// The text between quotes, `escapes` is `true` if it has `\"` or `\\`.
    Quoted {
        start: usize,
        end: usize,
        escapes: bool,
    },
}

/// Parses INI files.
struct IniParser<'a, 'c> {
    input: &'a str,
    config: &'c DeserializerConfig,
    lines: Lines<'a>,
    doc: Document<'a>,
}

impl<'a> IniParser<'a, '_> {
    fn parse(mut self) -> Result<Document<'a>, Error> {
        let mut table = 0;
        let mut pending: Option<Pending<'a>> = None;
        while let Some((start, end)) = self.lines.next() {
            let line = &self.input[start..end];
            let trimmed = line.trim_start_matches(is_ws);
            let content_start = end - trimmed.len();
            let indent = content_start - start;
            let content = trimmed.trim_end_matches(is_ws);
            if content.is_empty() {
                if let Some(ref mut pending) = pending {
                    pending.blanks += 1;
                }
                continue;
            }
            if content.starts_with([';', '#']) {
                continue;
            }
            if self.config.continuation == Continuation::Indented
                && let Some(ref mut pending) = pending
                && indent > pending.indent
            {
                self.continue_value(pending, content, content_start)?;
                continue;
            }
            if let Some(pending) = pending.take() {
                self.finish(pending)?;
            }
            if content.starts_with('[') {
                table = self.section(content, content_start)?;
            } else {
                pending = Some(self.entry(table, content, content_start, indent)?);
            }
        }
        if let Some(pending) = pending.take() {
            self.finish(pending)?;
        }
        Ok(self.doc)
    }

    /// Adds the value of a key that is complete.
    fn finish(&mut self, pending: Pending<'a>) -> Result<(), Error> {
        self.doc.value(
            pending.table,
            pending.key,
            pending.key_range,
            pending.value,
            pending.value_range,
        )
    }

    /// Returns a name (lowercased if configured).
    fn name(&self, name: &'a str) -> Cow<'a, str> {
        if self.config.lowercase_names && name.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(name.to_ascii_lowercase())
        } else {
            Cow::Borrowed(name)
        }
    }

    /// Parses a section header (the line starts with `[`).
    fn section(&mut self, content: &'a str, start: usize) -> Result<usize, Error> {
        // the first `]` that is followed by nothing or a comment closes the
        // header, so `]` can be in names
        let close = content.match_indices(']').map(|(pos, _)| pos).find(|&pos| {
            let rest = content[pos + 1..].trim_start_matches(is_ws);
            rest.is_empty() || rest.starts_with([';', '#'])
        });
        let Some(close) = close else {
            let msg = if content.contains(']') {
                "unexpected text after the section header"
            } else {
                "missing `]` of the section header"
            };
            return Err(Error::with_offset(ErrorKind::Syntax, msg, start));
        };
        let name = self.name(&content[1..close]);
        self.doc.table(0, name, (start, start + close + 1))
    }

    /// Parses a line with a key.
    fn entry(
        &mut self,
        table: usize,
        content: &'a str,
        start: usize,
        indent: usize,
    ) -> Result<Pending<'a>, Error> {
        let colon = self.config.colon_delimiter;
        let Some(delimiter) = content.find(|c| c == '=' || (colon && c == ':')) else {
            if !self.config.allow_no_value {
                return Err(Error::with_offset(
                    ErrorKind::Syntax,
                    if colon {
                        "expected `=` or `:` after the key"
                    } else {
                        "expected `=` after the key"
                    },
                    start,
                ));
            }
            let end = comment_start(content, self.config.inline_comments);
            let key = content[..end].trim_end_matches(is_ws);
            let key_range = (start, start + key.len());
            return Ok(Pending {
                table,
                key: self.name(key),
                key_range,
                indent,
                value: None,
                value_range: (key_range.1, key_range.1),
                blanks: 0,
                continued: false,
            });
        };
        let key = content[..delimiter].trim_end_matches(is_ws);
        if key.is_empty() {
            return Err(Error::with_offset(
                ErrorKind::Syntax,
                "missing key before the delimiter",
                start,
            ));
        }
        let key_range = (start, start + key.len());
        let raw_start = start + delimiter + 1;
        let raw = &content[delimiter + 1..];
        let (value, value_range) =
            if self.config.continuation == Continuation::Backslash && raw.ends_with('\\') {
                // lines that end with a backslash continue on the next line,
                // the backslash and the line break are removed
                let mut text = raw[..raw.len() - 1].to_string();
                let mut end = start + content.len();
                for (line_start, line_end) in self.lines.by_ref() {
                    let line = self.input[line_start..line_end].trim_end_matches(is_ws);
                    end = line_start + line.len();
                    match line.strip_suffix('\\') {
                        Some(line) => text.push_str(line),
                        None => {
                            text.push_str(line);
                            break;
                        }
                    }
                }
                let bounds = value_bounds(&text, self.config);
                let value = value_text(&text, &bounds).into_owned();
                (Cow::Owned(value), (raw_start, end))
            } else {
                let bounds = value_bounds(raw, self.config);
                let range = match bounds {
                    Bounds::Plain(s, e) => (raw_start + s, raw_start + e),
                    Bounds::Quoted {
                        start: s, end: e, ..
                    } => (raw_start + s - 1, raw_start + e + 1),
                };
                (value_text(raw, &bounds), range)
            };
        Ok(Pending {
            table,
            key: self.name(key),
            key_range,
            indent,
            value: Some(value),
            value_range,
            blanks: 0,
            continued: false,
        })
    }

    /// Adds a continuation line to the value of a key.
    fn continue_value(
        &mut self,
        pending: &mut Pending<'a>,
        content: &'a str,
        start: usize,
    ) -> Result<(), Error> {
        let Some(value) = pending.value.take() else {
            return Err(Error::with_offset(
                ErrorKind::Syntax,
                "a key without value cannot be continued on the next line",
                start,
            ));
        };
        let text =
            content[..comment_start(content, self.config.inline_comments)].trim_end_matches(is_ws);
        let mut value = value.into_owned();
        if value.is_empty() && !pending.continued {
            // a value that starts on the next line has no line break in
            // front
            pending.value_range.0 = start;
        } else {
            value.extend(std::iter::repeat_n('\n', pending.blanks + 1));
        }
        value.push_str(text);
        pending.value = Some(Cow::Owned(value));
        pending.value_range.1 = start + text.len();
        pending.blanks = 0;
        pending.continued = true;
        Ok(())
    }
}

/// Returns where the inline comment of a text starts (its length if there
/// is none).
///
/// With [`InlineComments::AfterWhitespace`] a `;` that follows whitespace
/// starts a comment and so does a `#` that follows whitespace after other
/// text (a value can start with `#`, like the colors `#ff0000`).
pub(crate) fn comment_start(text: &str, mode: InlineComments) -> usize {
    let bytes = text.as_bytes();
    match mode {
        InlineComments::None => bytes.len(),
        InlineComments::Anywhere => bytes
            .iter()
            .position(|&b| b == b';' || b == b'#')
            .unwrap_or(bytes.len()),
        InlineComments::AfterWhitespace => {
            let mut text_before = false;
            for (pos, &b) in bytes.iter().enumerate() {
                let after_ws = pos > 0 && matches!(bytes[pos - 1], b' ' | b'\t');
                if after_ws && (b == b';' || (b == b'#' && text_before)) {
                    return pos;
                }
                if b != b' ' && b != b'\t' {
                    text_before = true;
                }
            }
            bytes.len()
        }
    }
}

/// Finds the text of a value (the text after the delimiter).
fn value_bounds(raw: &str, config: &DeserializerConfig) -> Bounds {
    let lead = raw.len() - raw.trim_start_matches(is_ws).len();
    if config.quotes == Quotes::Value
        && let Some(quote @ (b'"' | b'\'')) = raw.as_bytes().get(lead).copied()
    {
        let bytes = &raw.as_bytes()[lead + 1..];
        let mut pos = 0;
        let mut escapes = false;
        while pos < bytes.len() {
            match bytes[pos] {
                b'\\' if quote == b'"' && matches!(bytes.get(pos + 1), Some(b'"' | b'\\')) => {
                    escapes = true;
                    pos += 2;
                    continue;
                }
                b if b == quote => break,
                _ => pos += 1,
            }
        }
        if pos < bytes.len() {
            // only a value that is quoted as a whole is unquoted, it can be
            // followed by a comment
            let rest = raw[lead + 1 + pos + 1..].trim_start_matches(is_ws);
            if rest.is_empty()
                || (config.inline_comments != InlineComments::None && rest.starts_with([';', '#']))
            {
                return Bounds::Quoted {
                    start: lead + 1,
                    end: lead + 1 + pos,
                    escapes,
                };
            }
        }
    }
    let end = comment_start(raw, config.inline_comments);
    let start = lead.min(end);
    Bounds::Plain(start, start + raw[start..end].trim_end_matches(is_ws).len())
}

/// Returns the text of a value.
fn value_text<'t>(raw: &'t str, bounds: &Bounds) -> Cow<'t, str> {
    match *bounds {
        Bounds::Plain(start, end)
        | Bounds::Quoted {
            start,
            end,
            escapes: false,
        } => Cow::Borrowed(&raw[start..end]),
        Bounds::Quoted { start, end, .. } => {
            let mut out = String::with_capacity(end - start);
            let mut chars = raw[start..end].chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    // the quoted text only has `\"` and `\\` escapes
                    out.extend(chars.next());
                } else {
                    out.push(c);
                }
            }
            Cow::Owned(out)
        }
    }
}

/// Parses git's config files like git does (see `config.c` of git).
struct GitParser<'a> {
    bytes: &'a [u8],
    pos: usize,
    eof: bool,
    doc: Document<'a>,
}

/// Returns `true` for the whitespace of git (`isspace` of git).
#[inline]
fn is_git_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}

#[cold]
fn bad_line(offset: usize) -> Error {
    Error::with_offset(ErrorKind::Syntax, "invalid line in config file", offset)
}

impl<'a> GitParser<'a> {
    /// Returns the next character, `\r\n` is `\n` and the end is `\n`.
    fn next_char(&mut self) -> u8 {
        let Some(&c) = self.bytes.get(self.pos) else {
            self.eof = true;
            return b'\n';
        };
        self.pos += 1;
        if c == b'\r' && self.bytes.get(self.pos) == Some(&b'\n') {
            self.pos += 1;
            return b'\n';
        }
        c
    }

    fn parse(mut self) -> Result<Document<'a>, Error> {
        let mut table = 0;
        let mut comment = false;
        loop {
            let start = self.pos;
            let c = self.next_char();
            if c == b'\n' {
                if self.eof {
                    return Ok(self.doc);
                }
                comment = false;
                continue;
            }
            if comment || is_git_space(c) {
                continue;
            }
            if c == b'#' || c == b';' {
                comment = true;
                continue;
            }
            if c == b'[' {
                table = self.section(start)?;
                // a key can follow on the same line
                continue;
            }
            if !c.is_ascii_alphabetic() {
                return Err(bad_line(start));
            }
            self.entry(table, start)?;
        }
    }

    /// Parses a section header after the `[`.
    fn section(&mut self, start: usize) -> Result<usize, Error> {
        let mut name = String::new();
        loop {
            let c = self.next_char();
            if self.eof {
                return Err(bad_line(start));
            }
            if c == b']' {
                break;
            }
            if is_git_space(c) {
                return self.extended_section(name, c, start);
            }
            if !c.is_ascii_alphanumeric() && c != b'-' && c != b'.' {
                return Err(bad_line(start));
            }
            name.push(c.to_ascii_lowercase() as char);
        }
        if name.is_empty() {
            return Err(bad_line(start));
        }
        let range = (start, self.pos);
        // the deprecated `[section.subsection]` is the same as
        // `[section "subsection"]` (lowercased)
        match name.split_once('.') {
            Some((section, subsection)) => {
                let section = self.doc.table(0, Cow::Owned(section.to_string()), range)?;
                self.doc
                    .table(section, Cow::Owned(subsection.to_string()), range)
            }
            None => self.doc.table(0, Cow::Owned(name), range),
        }
    }

    /// Parses `"subsection"]` of `[section "subsection"]`.
    fn extended_section(&mut self, name: String, mut c: u8, start: usize) -> Result<usize, Error> {
        loop {
            if c == b'\n' {
                return Err(bad_line(start));
            }
            c = self.next_char();
            if !is_git_space(c) {
                break;
            }
        }
        if c != b'"' || name.is_empty() {
            return Err(bad_line(start));
        }
        let mut subsection = Vec::new();
        loop {
            let mut c = self.next_char();
            if c == b'\n' {
                return Err(bad_line(start));
            }
            if c == b'"' {
                break;
            }
            // every character can be escaped, the backslash is dropped
            if c == b'\\' {
                c = self.next_char();
                if c == b'\n' {
                    return Err(bad_line(start));
                }
            }
            subsection.push(c);
        }
        if self.next_char() != b']' {
            return Err(bad_line(start));
        }
        let range = (start, self.pos);
        let subsection = String::from_utf8(subsection).map_err(|_| bad_line(start))?;
        let section = self.doc.table(0, Cow::Owned(name), range)?;
        self.doc.table(section, Cow::Owned(subsection), range)
    }

    /// Parses a key (its first character was read) and its value.
    fn entry(&mut self, table: usize, start: usize) -> Result<(), Error> {
        let mut key = String::new();
        key.push(self.bytes[start].to_ascii_lowercase() as char);
        let mut key_end = self.pos;
        let mut c;
        loop {
            c = self.next_char();
            if self.eof || !(c.is_ascii_alphanumeric() || c == b'-') {
                break;
            }
            key.push(c.to_ascii_lowercase() as char);
            key_end = self.pos;
        }
        while c == b' ' || c == b'\t' {
            c = self.next_char();
        }
        let (value, value_range) = if c == b'\n' {
            (None, (key_end, key_end))
        } else if c == b'=' {
            let value_start = self.pos;
            let value = self.value(start)?;
            (Some(Cow::Owned(value)), (value_start, self.pos))
        } else {
            return Err(bad_line(start));
        };
        self.doc
            .value(table, Cow::Owned(key), (start, key_end), value, value_range)
    }

    /// Parses a value after the `=`.
    fn value(&mut self, start: usize) -> Result<String, Error> {
        let mut out = Vec::new();
        let (mut quote, mut comment) = (false, false);
        // where the whitespace at the end of the value starts
        let mut trim: Option<usize> = None;
        loop {
            let c = self.next_char();
            if c == b'\n' {
                if quote {
                    return Err(Error::with_offset(
                        ErrorKind::Syntax,
                        "missing closing quote of the value",
                        start,
                    ));
                }
                if let Some(trim) = trim {
                    out.truncate(trim);
                }
                // only ASCII is changed, the text stays UTF-8
                return String::from_utf8(out).map_err(|_| bad_line(start));
            }
            if comment {
                continue;
            }
            if is_git_space(c) && !quote {
                // whitespace at the start and the end of a value is
                // removed
                if !out.is_empty() {
                    trim.get_or_insert(out.len());
                    out.push(c);
                }
                continue;
            }
            if !quote && (c == b';' || c == b'#') {
                comment = true;
                continue;
            }
            trim = None;
            match c {
                b'\\' => {
                    let escape_start = self.pos - 1;
                    out.push(match self.next_char() {
                        // a line that ends with a backslash continues
                        b'\n' => continue,
                        b't' => b'\t',
                        b'b' => b'\x08',
                        b'n' => b'\n',
                        c @ (b'\\' | b'"') => c,
                        _ => {
                            return Err(Error::with_offset(
                                ErrorKind::Syntax,
                                "invalid escape sequence in the value",
                                escape_start,
                            ));
                        }
                    });
                }
                b'"' => quote = !quote,
                c => out.push(c),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comment_start() {
        let ws = InlineComments::AfterWhitespace;
        assert_eq!(comment_start(" a ; b", ws), 3);
        assert_eq!(comment_start(" ; b", ws), 1);
        assert_eq!(comment_start(";b", ws), 2);
        assert_eq!(comment_start(" a;b", ws), 4);
        assert_eq!(comment_start(" #fff", ws), 5);
        assert_eq!(comment_start(" a #b", ws), 3);
        assert_eq!(comment_start("a#b", ws), 3);
        assert_eq!(comment_start(" a;b#c", InlineComments::Anywhere), 2);
        assert_eq!(comment_start(" a ; b", InlineComments::None), 6);
    }

    #[test]
    fn test_lines() {
        let lines: Vec<_> = Lines {
            input: "a\nb\r\nc\rd\n\ne",
            pos: 0,
        }
        .collect();
        assert_eq!(lines, [(0, 1), (2, 3), (5, 6), (7, 8), (9, 9), (10, 11)]);
    }
}
