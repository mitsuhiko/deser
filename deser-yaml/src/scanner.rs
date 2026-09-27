//! The YAML scanner: turns text into tokens.
//!
//! The design follows the classic libyaml architecture: block structure is
//! derived from indentation (producing `BlockSequenceStart`,
//! `BlockMappingStart` and `BlockEnd` tokens) and implicit keys are detected
//! by remembering where a potential "simple key" started.  Once the `:`
//! indicator is found, a `Key` token (and possibly a `BlockMappingStart`
//! token) is inserted at that position.  The rules are adjusted to follow
//! YAML 1.2.
use std::borrow::Cow;
use std::collections::VecDeque;

use deser::Error;

use crate::event::{Mark, ScalarStyle};
use crate::parser::syntax_error;

/// The maximum length of an implicit key (YAML 1.2, 7.4.2).
const MAX_SIMPLE_KEY_LENGTH: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind<'a> {
    StreamStart,
    StreamEnd,
    VersionDirective(u32, u32),
    TagDirective(Cow<'a, str>, Cow<'a, str>),
    ReservedDirective,
    DocumentStart,
    DocumentEnd,
    BlockSequenceStart,
    BlockMappingStart,
    BlockEnd,
    FlowSequenceStart,
    FlowSequenceEnd,
    FlowMappingStart,
    FlowMappingEnd,
    BlockEntry,
    FlowEntry,
    Key,
    Value,
    Alias(Cow<'a, str>),
    Anchor(Cow<'a, str>),
    /// A tag as handle and suffix.  Verbatim tags have an empty handle.
    Tag(Cow<'a, str>, Cow<'a, str>),
    Scalar(ScalarStyle, Cow<'a, str>),
}

/// The kind of a token without payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    StreamStart,
    StreamEnd,
    VersionDirective,
    TagDirective,
    ReservedDirective,
    DocumentStart,
    DocumentEnd,
    BlockSequenceStart,
    BlockMappingStart,
    BlockEnd,
    FlowSequenceStart,
    FlowSequenceEnd,
    FlowMappingStart,
    FlowMappingEnd,
    BlockEntry,
    FlowEntry,
    Key,
    Value,
    Alias,
    Anchor,
    Tag,
    Scalar,
}

impl TokenKind<'_> {
    pub fn ty(&self) -> TokenType {
        match self {
            TokenKind::StreamStart => TokenType::StreamStart,
            TokenKind::StreamEnd => TokenType::StreamEnd,
            TokenKind::VersionDirective(..) => TokenType::VersionDirective,
            TokenKind::TagDirective(..) => TokenType::TagDirective,
            TokenKind::ReservedDirective => TokenType::ReservedDirective,
            TokenKind::DocumentStart => TokenType::DocumentStart,
            TokenKind::DocumentEnd => TokenType::DocumentEnd,
            TokenKind::BlockSequenceStart => TokenType::BlockSequenceStart,
            TokenKind::BlockMappingStart => TokenType::BlockMappingStart,
            TokenKind::BlockEnd => TokenType::BlockEnd,
            TokenKind::FlowSequenceStart => TokenType::FlowSequenceStart,
            TokenKind::FlowSequenceEnd => TokenType::FlowSequenceEnd,
            TokenKind::FlowMappingStart => TokenType::FlowMappingStart,
            TokenKind::FlowMappingEnd => TokenType::FlowMappingEnd,
            TokenKind::BlockEntry => TokenType::BlockEntry,
            TokenKind::FlowEntry => TokenType::FlowEntry,
            TokenKind::Key => TokenType::Key,
            TokenKind::Value => TokenType::Value,
            TokenKind::Alias(..) => TokenType::Alias,
            TokenKind::Anchor(..) => TokenType::Anchor,
            TokenKind::Tag(..) => TokenType::Tag,
            TokenKind::Scalar(..) => TokenType::Scalar,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token<'a> {
    pub kind: TokenKind<'a>,
    pub start: Mark,
    pub end: Mark,
}

/// A position where an implicit key might start.
#[derive(Debug, Clone, Copy, Default)]
struct SimpleKey {
    possible: bool,
    /// The key must be completed (it is at the indentation of a block
    /// mapping).
    required: bool,
    /// The number of the first token of the key.
    token_number: usize,
    mark: Mark,
    /// A tab was used in the whitespace before the key.
    tab_before: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlowKind {
    Sequence,
    Mapping,
}

pub struct Scanner<'a> {
    input: &'a str,
    mark: Mark,
    tokens: VecDeque<Token<'a>>,
    /// The number of tokens handed out so far.
    tokens_parsed: usize,
    stream_start_produced: bool,
    stream_end_produced: bool,
    /// The current block indentation (-1 outside of block collections).
    indent: isize,
    indents: Vec<isize>,
    flows: Vec<FlowKind>,
    simple_key_allowed: bool,
    /// One slot per flow level (plus one for the block context).
    simple_keys: Vec<SimpleKey>,
    /// The possible simple keys as `(level, token_number)`, ordered by
    /// level.  Keys are only ever saved on the innermost level, so the order
    /// by level is also the order by position.  Entries are removed lazily
    /// and must be validated with `is_possible_key`.
    possible_keys: VecDeque<(usize, usize)>,
    /// The subset of `possible_keys` that can become stale (all keys but
    /// the ones in flow mappings).  Keys go stale in order of their position
    /// so only the front needs to be checked.
    stale_keys: VecDeque<(usize, usize)>,
    /// The previous token was a JSON-like node (quoted scalar or flow
    /// collection) which allows `:` to be adjacent to the value in flow
    /// context.
    adjacent_value_allowed: bool,
    // -- per line tracking, maintained by `skip` --
    /// Non whitespace was seen on the current line.
    line_has_content: bool,
    /// The number of spaces at the beginning of the line.
    line_indent: usize,
    /// A tab was seen in the whitespace since the last content character.
    ws_has_tab: bool,
}

/// Adds a key to a key queue, replacing an older key of the same level.
fn push_key(keys: &mut VecDeque<(usize, usize)>, entry: (usize, usize)) {
    match keys.back_mut() {
        Some(last) if last.0 == entry.0 => *last = entry,
        _ => keys.push_back(entry),
    }
}

#[inline]
fn is_blank(c: Option<char>) -> bool {
    matches!(c, Some(' ' | '\t'))
}

#[inline]
fn is_break(c: Option<char>) -> bool {
    matches!(c, Some('\n' | '\r'))
}

#[inline]
fn is_blank_or_break(c: Option<char>) -> bool {
    matches!(c, Some(' ' | '\t' | '\n' | '\r'))
}

#[inline]
fn is_blankz(c: Option<char>) -> bool {
    matches!(c, None | Some(' ' | '\t' | '\n' | '\r'))
}

#[inline]
fn is_flow_indicator(c: Option<char>) -> bool {
    matches!(c, Some(',' | '[' | ']' | '{' | '}'))
}

#[inline]
fn is_word_char(c: Option<char>) -> bool {
    matches!(c, Some('0'..='9' | 'a'..='z' | 'A'..='Z' | '-'))
}

#[inline]
fn is_uri_char(c: Option<char>) -> bool {
    is_word_char(c)
        || matches!(
            c,
            Some(
                '%' | '#'
                    | ';'
                    | '/'
                    | '?'
                    | ':'
                    | '@'
                    | '&'
                    | '='
                    | '+'
                    | '$'
                    | ','
                    | '_'
                    | '.'
                    | '!'
                    | '~'
                    | '*'
                    | '\''
                    | '('
                    | ')'
                    | '['
                    | ']'
            )
        )
}

#[inline]
fn is_tag_char(c: Option<char>) -> bool {
    is_uri_char(c) && c != Some('!') && !is_flow_indicator(c)
}

impl<'a> Scanner<'a> {
    pub fn new(input: &'a str) -> Scanner<'a> {
        Scanner {
            input,
            mark: Mark::default(),
            tokens: VecDeque::new(),
            tokens_parsed: 0,
            stream_start_produced: false,
            stream_end_produced: false,
            indent: -1,
            indents: Vec::new(),
            flows: Vec::new(),
            simple_key_allowed: false,
            simple_keys: Vec::new(),
            possible_keys: VecDeque::new(),
            stale_keys: VecDeque::new(),
            adjacent_value_allowed: false,
            line_has_content: false,
            line_indent: 0,
            ws_has_tab: false,
        }
    }

    // -- reading ----------------------------------------------------------

    #[inline]
    fn peek(&self) -> Option<char> {
        let b = *self.input.as_bytes().get(self.mark.offset)?;
        if b < 0x80 {
            Some(b as char)
        } else {
            self.input[self.mark.offset..].chars().next()
        }
    }

    #[inline]
    fn peek_at(&self, n: usize) -> Option<char> {
        self.input[self.mark.offset..].chars().nth(n)
    }

    #[inline]
    fn rest(&self) -> &'a str {
        &self.input[self.mark.offset..]
    }

    /// Advances over one character that is not a line break.
    #[inline]
    fn skip(&mut self) {
        let c = match self.peek() {
            Some(c) => c,
            None => return,
        };
        debug_assert!(c != '\n' && c != '\r');
        self.mark.offset += c.len_utf8();
        self.mark.column += 1;
        match c {
            ' ' => {
                if !self.line_has_content && !self.ws_has_tab {
                    self.line_indent += 1;
                }
            }
            '\t' => self.ws_has_tab = true,
            _ => {
                self.line_has_content = true;
                self.ws_has_tab = false;
            }
        }
    }

    /// Advances over a line break (`\r\n`, `\r` or `\n`).
    fn skip_break(&mut self) {
        if self.rest().starts_with("\r\n") {
            self.mark.offset += 2;
        } else {
            self.mark.offset += 1;
        }
        self.mark.line += 1;
        self.mark.column = 0;
        self.line_has_content = false;
        self.line_indent = 0;
        self.ws_has_tab = false;
    }

    /// Advances over a character and pushes it to the string.
    #[inline]
    fn read(&mut self, s: &mut String) {
        if let Some(c) = self.peek() {
            s.push(c);
            self.skip();
        }
    }

    /// Advances over a line break and pushes a normalized `\n`.
    fn read_break(&mut self, s: &mut String) {
        self.skip_break();
        s.push('\n');
    }

    fn is_document_indicator(&self) -> bool {
        self.mark.column == 0
            && (self.rest().starts_with("---") || self.rest().starts_with("..."))
            && is_blankz(self.peek_at(3))
    }

    fn error<T>(&self, msg: &str) -> Result<T, Error> {
        Err(syntax_error(self.mark, msg))
    }

    // -- public interface -------------------------------------------------

    /// Returns the type of the next token.
    pub fn peek_type(&mut self) -> Result<TokenType, Error> {
        self.fetch_more_tokens()?;
        Ok(self.tokens.front().unwrap().kind.ty())
    }

    /// Returns the next token.
    pub fn peek_token(&mut self) -> Result<&Token<'a>, Error> {
        self.fetch_more_tokens()?;
        Ok(self.tokens.front().unwrap())
    }

    /// Removes the next token.  Must only be called after peeking.
    pub fn next_token(&mut self) -> Token<'a> {
        let token = self.tokens.pop_front().expect("no token peeked");
        self.tokens_parsed += 1;
        token
    }

    #[inline]
    fn fetch_more_tokens(&mut self) -> Result<(), Error> {
        // a token is available and no key can be inserted before it
        if !self.tokens.is_empty() && self.possible_keys.is_empty() && self.stale_keys.is_empty() {
            return Ok(());
        }
        self.fetch_more_tokens_slow()
    }

    #[inline(never)]
    fn fetch_more_tokens_slow(&mut self) -> Result<(), Error> {
        loop {
            if self.stream_end_produced && !self.tokens.is_empty() {
                return Ok(());
            }
            let mut need_more = self.tokens.is_empty();
            if !need_more {
                self.stale_simple_keys()?;
                // the first possible key has the lowest token number
                while let Some(&(level, number)) = self.possible_keys.front() {
                    if self.is_possible_key(level, number) {
                        break;
                    }
                    self.possible_keys.pop_front();
                }
                need_more = self
                    .possible_keys
                    .front()
                    .is_some_and(|&(_, number)| number == self.tokens_parsed);
            }
            if !need_more {
                return Ok(());
            }
            if self.stream_end_produced {
                return self.error("unexpected end of stream");
            }
            self.fetch_next_token()?;
        }
    }

    fn fetch_next_token(&mut self) -> Result<(), Error> {
        if !self.stream_start_produced {
            return self.fetch_stream_start();
        }

        self.scan_to_next_token()?;
        self.stale_simple_keys()?;
        self.unroll_indent(self.mark.column as isize);
        let adjacent_value_allowed = std::mem::take(&mut self.adjacent_value_allowed);

        let c = match self.peek() {
            Some(c) => c,
            None => return self.fetch_stream_end(),
        };

        // the first token of a line must be indented more than the block
        // it belongs to.  Tabs are never indentation.
        if !self.line_has_content
            && (self.flow_level() > 0 || self.ws_has_tab)
            && self.line_indent as isize <= self.indent
        {
            return self.error("invalid indentation");
        }

        if self.mark.column == 0 {
            if c == '%' {
                return self.fetch_directive();
            }
            if self.is_document_indicator() {
                return self.fetch_document_indicator(if c == '-' {
                    TokenKind::DocumentStart
                } else {
                    TokenKind::DocumentEnd
                });
            }
        }

        let next = self.peek_at(1);
        let in_flow = self.flow_level() > 0;
        match c {
            '[' => self.fetch_flow_collection_start(TokenKind::FlowSequenceStart),
            '{' => self.fetch_flow_collection_start(TokenKind::FlowMappingStart),
            ']' => self.fetch_flow_collection_end(TokenKind::FlowSequenceEnd),
            '}' => self.fetch_flow_collection_end(TokenKind::FlowMappingEnd),
            ',' => self.fetch_flow_entry(),
            '-' if is_blankz(next) => self.fetch_block_entry(),
            '?' if is_blankz(next) => self.fetch_key(),
            ':' if is_blankz(next)
                || (in_flow && (is_flow_indicator(next) || adjacent_value_allowed)) =>
            {
                self.fetch_value()
            }
            '*' => self.fetch_anchor(true),
            '&' => self.fetch_anchor(false),
            '!' => self.fetch_tag(),
            '|' | '>' if !in_flow => self.fetch_block_scalar(c == '>'),
            '\'' => self.fetch_flow_scalar(true),
            '"' => self.fetch_flow_scalar(false),
            '-' | '?' | ':' if !is_blankz(next) && !(in_flow && is_flow_indicator(next)) => {
                self.fetch_plain_scalar()
            }
            '-' | '?' | ':' | '#' | '|' | '>' | '%' | '@' | '`' => self.error(&format!(
                "found character {:?} that cannot start any token",
                c
            )),
            _ => self.fetch_plain_scalar(),
        }
    }

    // -- helpers ----------------------------------------------------------

    fn flow_level(&self) -> usize {
        self.flows.len()
    }

    fn push_token(&mut self, kind: TokenKind<'a>, start: Mark) {
        self.tokens.push_back(Token {
            kind,
            start,
            end: self.mark,
        });
    }

    /// Skips whitespace, comments and line breaks.
    fn scan_to_next_token(&mut self) -> Result<(), Error> {
        loop {
            if self.mark.offset == 0 && self.peek() == Some('\u{feff}') {
                self.mark.offset += 3;
            }
            while is_blank(self.peek()) {
                self.skip();
            }
            if self.peek() == Some('#') {
                // a comment must be separated from the preceding token
                let prev = self.input[..self.mark.offset].chars().next_back();
                if !matches!(prev, None | Some(' ' | '\t' | '\n' | '\r' | '\u{feff}')) {
                    return self.error("comments must be separated by whitespace");
                }
                while !is_break(self.peek()) && self.peek().is_some() {
                    self.skip();
                }
            }
            if is_break(self.peek()) {
                self.skip_break();
                if self.flow_level() == 0 {
                    self.simple_key_allowed = true;
                }
            } else {
                return Ok(());
            }
        }
    }

    fn is_possible_key(&self, level: usize, token_number: usize) -> bool {
        self.simple_keys
            .get(level)
            .is_some_and(|key| key.possible && key.token_number == token_number)
    }

    /// Keys in flow mappings may span lines, all other keys (implicit keys)
    /// are restricted to one line of 1024 characters.  This marks the keys
    /// that can no longer be completed as impossible.
    fn stale_simple_keys(&mut self) -> Result<(), Error> {
        while let Some(&(level, number)) = self.stale_keys.front() {
            if self.is_possible_key(level, number) {
                let key = self.simple_keys[level];
                if key.mark.line == self.mark.line
                    && key.mark.offset + MAX_SIMPLE_KEY_LENGTH >= self.mark.offset
                {
                    break;
                }
                if key.required {
                    return Err(syntax_error(key.mark, "could not find expected ':'"));
                }
                self.simple_keys[level].possible = false;
            }
            self.stale_keys.pop_front();
        }
        Ok(())
    }

    fn save_simple_key(&mut self) -> Result<(), Error> {
        if self.simple_key_allowed {
            let key = SimpleKey {
                possible: true,
                required: self.flow_level() == 0 && self.indent == self.mark.column as isize,
                token_number: self.tokens_parsed + self.tokens.len(),
                mark: self.mark,
                tab_before: self.ws_has_tab,
            };
            self.remove_simple_key()?;
            *self.simple_keys.last_mut().unwrap() = key;
            let level = self.simple_keys.len() - 1;
            let entry = (level, key.token_number);
            push_key(&mut self.possible_keys, entry);
            if level == 0 || self.flows[level - 1] != FlowKind::Mapping {
                push_key(&mut self.stale_keys, entry);
            }
        }
        Ok(())
    }

    fn remove_simple_key(&mut self) -> Result<(), Error> {
        let key = self.simple_keys.last_mut().unwrap();
        if key.possible && key.required {
            return Err(syntax_error(key.mark, "could not find expected ':'"));
        }
        key.possible = false;
        Ok(())
    }

    /// Opens a block collection if the column is indented more.
    fn roll_indent(
        &mut self,
        column: isize,
        token_number: Option<usize>,
        kind: TokenKind<'a>,
        mark: Mark,
    ) {
        if self.flow_level() > 0 || self.indent >= column {
            return;
        }
        self.indents.push(self.indent);
        self.indent = column;
        let token = Token {
            kind,
            start: mark,
            end: mark,
        };
        match token_number {
            Some(number) => self.tokens.insert(number - self.tokens_parsed, token),
            None => self.tokens.push_back(token),
        }
    }

    /// Closes all block collections that are indented more than the column.
    fn unroll_indent(&mut self, column: isize) {
        if self.flow_level() > 0 {
            return;
        }
        while self.indent > column {
            self.push_token(TokenKind::BlockEnd, self.mark);
            self.indent = self.indents.pop().unwrap();
        }
    }

    // -- fetchers ---------------------------------------------------------

    fn fetch_stream_start(&mut self) -> Result<(), Error> {
        self.indent = -1;
        self.simple_key_allowed = true;
        self.simple_keys.push(SimpleKey::default());
        self.stream_start_produced = true;
        self.push_token(TokenKind::StreamStart, self.mark);
        Ok(())
    }

    fn fetch_stream_end(&mut self) -> Result<(), Error> {
        self.unroll_indent(-1);
        self.remove_simple_key()?;
        self.simple_key_allowed = false;
        self.stream_end_produced = true;
        self.push_token(TokenKind::StreamEnd, self.mark);
        Ok(())
    }

    fn fetch_directive(&mut self) -> Result<(), Error> {
        self.unroll_indent(-1);
        self.remove_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_directive()?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_document_indicator(&mut self, kind: TokenKind<'a>) -> Result<(), Error> {
        if self.flow_level() > 0 {
            return self.error("document markers are not allowed in flow collections");
        }
        self.unroll_indent(-1);
        self.remove_simple_key()?;
        self.simple_key_allowed = false;
        let start = self.mark;
        for _ in 0..3 {
            self.skip();
        }
        let is_end = kind == TokenKind::DocumentEnd;
        self.push_token(kind, start);
        if is_end {
            // only a comment may follow the document end marker
            while is_blank(self.peek()) {
                self.skip();
            }
            if !is_break(self.peek()) && self.peek().is_some() && self.peek() != Some('#') {
                return self.error("unexpected content after document end marker");
            }
        }
        Ok(())
    }

    fn fetch_flow_collection_start(&mut self, kind: TokenKind<'a>) -> Result<(), Error> {
        self.save_simple_key()?;
        self.flows.push(if kind == TokenKind::FlowSequenceStart {
            FlowKind::Sequence
        } else {
            FlowKind::Mapping
        });
        self.simple_keys.push(SimpleKey::default());
        self.simple_key_allowed = true;
        let start = self.mark;
        self.skip();
        self.push_token(kind, start);
        Ok(())
    }

    fn fetch_flow_collection_end(&mut self, kind: TokenKind<'a>) -> Result<(), Error> {
        if self.flow_level() == 0 {
            return self.error("unexpected end of flow collection");
        }
        self.remove_simple_key()?;
        self.simple_keys.pop();
        self.flows.pop();
        let level = self.simple_keys.len();
        for keys in [&mut self.possible_keys, &mut self.stale_keys] {
            while keys.back().is_some_and(|&(l, _)| l >= level) {
                keys.pop_back();
            }
        }
        self.simple_key_allowed = false;
        self.adjacent_value_allowed = true;
        let start = self.mark;
        self.skip();
        self.push_token(kind, start);
        Ok(())
    }

    fn fetch_flow_entry(&mut self) -> Result<(), Error> {
        if self.flow_level() == 0 {
            return self.error("unexpected ',' outside of flow collection");
        }
        self.remove_simple_key()?;
        self.simple_key_allowed = true;
        let start = self.mark;
        self.skip();
        self.push_token(TokenKind::FlowEntry, start);
        Ok(())
    }

    /// Fails if a tab is used where block indentation is expected.
    fn check_no_tab_indentation(&self, tab_before: bool, mark: Mark) -> Result<(), Error> {
        if tab_before {
            Err(syntax_error(
                mark,
                "tabs are not allowed as indentation of block collections",
            ))
        } else {
            Ok(())
        }
    }

    fn fetch_block_entry(&mut self) -> Result<(), Error> {
        if self.flow_level() > 0 {
            return self.error("block sequence entries are not allowed in flow collections");
        }
        if !self.simple_key_allowed {
            return self.error("block sequence entries are not allowed in this context");
        }
        self.check_no_tab_indentation(self.ws_has_tab, self.mark)?;
        self.roll_indent(
            self.mark.column as isize,
            None,
            TokenKind::BlockSequenceStart,
            self.mark,
        );
        self.remove_simple_key()?;
        self.simple_key_allowed = true;
        let start = self.mark;
        self.skip();
        self.push_token(TokenKind::BlockEntry, start);
        Ok(())
    }

    fn fetch_key(&mut self) -> Result<(), Error> {
        if self.flow_level() == 0 {
            if !self.simple_key_allowed {
                return self.error("mapping keys are not allowed in this context");
            }
            self.check_no_tab_indentation(self.ws_has_tab, self.mark)?;
            self.roll_indent(
                self.mark.column as isize,
                None,
                TokenKind::BlockMappingStart,
                self.mark,
            );
        }
        self.remove_simple_key()?;
        self.simple_key_allowed = self.flow_level() == 0;
        let start = self.mark;
        self.skip();
        self.push_token(TokenKind::Key, start);
        Ok(())
    }

    fn fetch_value(&mut self) -> Result<(), Error> {
        let key = *self.simple_keys.last().unwrap();
        if key.possible {
            if self.flow_level() == 0 {
                self.check_no_tab_indentation(key.tab_before, key.mark)?;
            }
            let token = Token {
                kind: TokenKind::Key,
                start: key.mark,
                end: key.mark,
            };
            self.tokens
                .insert(key.token_number - self.tokens_parsed, token);
            self.roll_indent(
                key.mark.column as isize,
                Some(key.token_number),
                TokenKind::BlockMappingStart,
                key.mark,
            );
            self.simple_keys.last_mut().unwrap().possible = false;
            self.simple_key_allowed = false;
        } else {
            if self.flow_level() == 0 {
                if !self.simple_key_allowed {
                    return self.error("mapping values are not allowed in this context");
                }
                self.check_no_tab_indentation(self.ws_has_tab, self.mark)?;
                self.roll_indent(
                    self.mark.column as isize,
                    None,
                    TokenKind::BlockMappingStart,
                    self.mark,
                );
            }
            self.simple_key_allowed = self.flow_level() == 0;
        }
        let start = self.mark;
        self.skip();
        self.push_token(TokenKind::Value, start);
        Ok(())
    }

    fn fetch_anchor(&mut self, alias: bool) -> Result<(), Error> {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let start = self.mark;
        self.skip();
        let name_start = self.mark.offset;
        while !is_blankz(self.peek()) && !is_flow_indicator(self.peek()) {
            self.skip();
        }
        if self.mark.offset == name_start {
            return self.error("anchor and alias names must not be empty");
        }
        let name = Cow::Borrowed(&self.input[name_start..self.mark.offset]);
        self.push_token(
            if alias {
                TokenKind::Alias(name)
            } else {
                TokenKind::Anchor(name)
            },
            start,
        );
        Ok(())
    }

    fn fetch_tag(&mut self) -> Result<(), Error> {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let start = self.mark;
        let (handle, suffix) = self.scan_tag()?;
        if !(is_blankz(self.peek()) || (self.flow_level() > 0 && is_flow_indicator(self.peek()))) {
            return self.error("expected whitespace after tag");
        }
        self.push_token(TokenKind::Tag(handle, suffix), start);
        Ok(())
    }

    fn fetch_block_scalar(&mut self, folded: bool) -> Result<(), Error> {
        self.remove_simple_key()?;
        self.simple_key_allowed = true;
        let token = self.scan_block_scalar(folded)?;
        self.tokens.push_back(token);
        Ok(())
    }

    fn fetch_flow_scalar(&mut self, single: bool) -> Result<(), Error> {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_flow_scalar(single)?;
        self.tokens.push_back(token);
        self.adjacent_value_allowed = true;
        Ok(())
    }

    fn fetch_plain_scalar(&mut self) -> Result<(), Error> {
        self.save_simple_key()?;
        self.simple_key_allowed = false;
        let token = self.scan_plain_scalar()?;
        self.tokens.push_back(token);
        Ok(())
    }

    // -- scanners ---------------------------------------------------------

    /// Skips whitespace and an optional comment up to the end of the line.
    fn skip_line_trailer(&mut self, what: &str) -> Result<(), Error> {
        let mut had_blank = is_blank(self.peek_before());
        while is_blank(self.peek()) {
            self.skip();
            had_blank = true;
        }
        if self.peek() == Some('#') {
            if !had_blank {
                return self.error("comments must be separated by whitespace");
            }
            while !is_break(self.peek()) && self.peek().is_some() {
                self.skip();
            }
        }
        if !is_break(self.peek()) && self.peek().is_some() {
            return self.error(&format!("unexpected content after {}", what));
        }
        Ok(())
    }

    fn scan_directive(&mut self) -> Result<Token<'a>, Error> {
        let start = self.mark;
        self.skip();
        let name_start = self.mark.offset;
        while !is_blankz(self.peek()) {
            self.skip();
        }
        let name = &self.input[name_start..self.mark.offset];
        let kind = match name {
            "YAML" => {
                self.skip_separator("expected whitespace after %YAML")?;
                let major = self.scan_version_number()?;
                if self.peek() != Some('.') {
                    return self.error("expected '.' in %YAML directive");
                }
                self.skip();
                let minor = self.scan_version_number()?;
                TokenKind::VersionDirective(major, minor)
            }
            "TAG" => {
                self.skip_separator("expected whitespace after %TAG")?;
                let handle = self.scan_tag_handle()?;
                if !handle.ends_with('!') {
                    return self.error("invalid tag handle in %TAG directive");
                }
                self.skip_separator("expected whitespace after tag handle")?;
                let prefix = self.scan_tag_prefix()?;
                TokenKind::TagDirective(handle, prefix)
            }
            "" => return self.error("expected directive name"),
            _ => {
                // reserved directive, parameters are ignored
                while !is_break(self.peek()) && self.peek().is_some() {
                    if self.peek() == Some('#') && is_blank(self.peek_before()) {
                        break;
                    }
                    self.skip();
                }
                TokenKind::ReservedDirective
            }
        };
        let end = self.mark;
        self.skip_line_trailer("directive")?;
        Ok(Token { kind, start, end })
    }

    fn peek_before(&self) -> Option<char> {
        self.input[..self.mark.offset].chars().next_back()
    }

    fn skip_separator(&mut self, msg: &str) -> Result<(), Error> {
        if !is_blank(self.peek()) {
            return self.error(msg);
        }
        while is_blank(self.peek()) {
            self.skip();
        }
        Ok(())
    }

    fn scan_version_number(&mut self) -> Result<u32, Error> {
        let start = self.mark.offset;
        while matches!(self.peek(), Some('0'..='9')) {
            self.skip();
        }
        self.input[start..self.mark.offset]
            .parse()
            .or_else(|_| self.error("invalid version number in %YAML directive"))
    }

    /// Scans `!`, `!!` or `!word!`.  For primary tags this returns just `!`
    /// and leaves the rest for the suffix.
    fn scan_tag_handle(&mut self) -> Result<Cow<'a, str>, Error> {
        let start = self.mark.offset;
        if self.peek() != Some('!') {
            return self.error("expected '!' in tag handle");
        }
        self.skip();
        let word_start = self.mark;
        while is_word_char(self.peek()) {
            self.skip();
        }
        if self.peek() == Some('!') {
            self.skip();
        } else if word_start.offset != self.mark.offset {
            // this was the primary handle followed by a suffix, rewind
            self.mark = word_start;
        }
        Ok(Cow::Borrowed(&self.input[start..self.mark.offset]))
    }

    fn scan_tag_prefix(&mut self) -> Result<Cow<'a, str>, Error> {
        // a prefix is a local tag prefix (starting with `!`) or a global one
        let start = self.mark.offset;
        let first = self.peek();
        if first != Some('!') && !is_tag_char(first) {
            return self.error("invalid tag prefix");
        }
        let mut rv = String::new();
        let mut decoded = false;
        if first == Some('!') {
            self.read(&mut rv);
        }
        while is_uri_char(self.peek()) {
            if self.peek() == Some('%') {
                decoded = true;
                self.scan_uri_escape(&mut rv)?;
            } else {
                self.read(&mut rv);
            }
        }
        Ok(if decoded {
            Cow::Owned(rv)
        } else {
            Cow::Borrowed(&self.input[start..self.mark.offset])
        })
    }

    fn scan_uri_escape(&mut self, out: &mut String) -> Result<(), Error> {
        let mut bytes = Vec::new();
        while self.peek() == Some('%') {
            let hex = self.rest().get(1..3).unwrap_or("");
            let byte = match u8::from_str_radix(hex, 16) {
                Ok(byte) if hex.len() == 2 => byte,
                _ => return self.error("invalid URI escape"),
            };
            bytes.push(byte);
            for _ in 0..3 {
                self.skip();
            }
        }
        match String::from_utf8(bytes) {
            Ok(s) => {
                out.push_str(&s);
                Ok(())
            }
            Err(_) => self.error("invalid UTF-8 in URI escape"),
        }
    }

    fn scan_tag(&mut self) -> Result<(Cow<'a, str>, Cow<'a, str>), Error> {
        if self.peek_at(1) == Some('<') {
            // verbatim tag
            self.skip();
            self.skip();
            let mut suffix = String::new();
            while is_uri_char(self.peek()) {
                if self.peek() == Some('%') {
                    self.scan_uri_escape(&mut suffix)?;
                } else {
                    self.read(&mut suffix);
                }
            }
            if self.peek() != Some('>') || suffix.is_empty() {
                return self.error("invalid verbatim tag");
            }
            self.skip();
            return Ok((Cow::Borrowed(""), Cow::Owned(suffix)));
        }

        let handle = self.scan_tag_handle()?;
        let start = self.mark.offset;
        let mut suffix = String::new();
        let mut decoded = false;
        while is_tag_char(self.peek()) {
            if self.peek() == Some('%') {
                decoded = true;
                self.scan_uri_escape(&mut suffix)?;
            } else {
                self.read(&mut suffix);
            }
        }
        if suffix.is_empty() && handle != "!" {
            return self.error("tag suffix must not be empty");
        }
        let suffix = if decoded {
            Cow::Owned(suffix)
        } else {
            Cow::Borrowed(&self.input[start..self.mark.offset])
        };
        Ok((handle, suffix))
    }

    fn scan_block_scalar(&mut self, folded: bool) -> Result<Token<'a>, Error> {
        let start = self.mark;
        self.skip();

        // header: chomping and indentation indicators in any order
        let mut chomping = 0i8;
        let mut increment = 0usize;
        for _ in 0..2 {
            match self.peek() {
                Some('+') if chomping == 0 => {
                    chomping = 1;
                    self.skip();
                }
                Some('-') if chomping == 0 => {
                    chomping = -1;
                    self.skip();
                }
                Some(c @ '1'..='9') if increment == 0 => {
                    increment = c as usize - '0' as usize;
                    self.skip();
                }
                Some('0') => return self.error("block scalar indentation indicator must not be 0"),
                _ => break,
            }
        }
        self.skip_line_trailer("block scalar header")?;

        let min_indent = (self.indent + 1).max(0) as usize;
        let mut indent = if increment > 0 {
            (self.indent.max(0) as usize) + increment
        } else {
            0
        };

        let mut string = String::new();
        let mut leading_break = String::new();
        let mut trailing_breaks = String::new();

        if is_break(self.peek()) {
            self.skip_break();
        }

        // determine the indentation from the first non empty line
        if indent == 0 {
            let mut max_leading = 0;
            loop {
                while self.peek() == Some(' ') {
                    self.skip();
                }
                if is_break(self.peek()) {
                    max_leading = max_leading.max(self.mark.column);
                    self.read_break(&mut trailing_breaks);
                    continue;
                }
                break;
            }
            let at_end = self.peek().is_none();
            indent = self.mark.column;
            if at_end {
                indent = indent.max(max_leading);
                // like the reference parser, treat a final line that is
                // not terminated by a line break as if it was.
                if !self.line_has_content && self.mark.column > 0 {
                    trailing_breaks.push('\n');
                }
            }
            if indent < min_indent {
                // the scalar is empty (the next line belongs to the parent)
                if self.peek() == Some('\t') {
                    return self.error("tabs are not allowed as indentation");
                }
                indent = min_indent;
            } else if max_leading > indent && !at_end {
                return self.error(
                    "leading empty lines of a block scalar must not be \
                     indented more than the first line",
                );
            }
            // start of the first line is consumed at this point, the
            // regular loop expects to be at the indentation.
        } else {
            self.scan_block_scalar_breaks(indent, &mut trailing_breaks)?;
        }

        let mut leading_blank = false;
        while self.mark.column == indent && self.peek().is_some() && !self.is_document_indicator() {
            let trailing_blank = is_blank(self.peek());
            if folded && leading_break == "\n" && !leading_blank && !trailing_blank {
                if trailing_breaks.is_empty() {
                    string.push(' ');
                }
                leading_break.clear();
            } else {
                string.push_str(&leading_break);
                leading_break.clear();
            }
            string.push_str(&trailing_breaks);
            trailing_breaks.clear();
            leading_blank = is_blank(self.peek());

            while !is_break(self.peek()) && self.peek().is_some() {
                self.read(&mut string);
            }
            if self.peek().is_none() {
                // an unterminated final line counts as terminated
                leading_break.push('\n');
                break;
            }
            self.read_break(&mut leading_break);
            self.scan_block_scalar_breaks(indent, &mut trailing_breaks)?;
        }

        if chomping != -1 {
            string.push_str(&leading_break);
        }
        if chomping == 1 {
            string.push_str(&trailing_breaks);
        }

        Ok(Token {
            kind: TokenKind::Scalar(
                if folded {
                    ScalarStyle::Folded
                } else {
                    ScalarStyle::Literal
                },
                Cow::Owned(string),
            ),
            start,
            end: self.mark,
        })
    }

    /// Consumes empty lines and the indentation of the next content line.
    fn scan_block_scalar_breaks(
        &mut self,
        indent: usize,
        breaks: &mut String,
    ) -> Result<(), Error> {
        loop {
            while self.mark.column < indent && self.peek() == Some(' ') {
                self.skip();
            }
            if self.mark.column < indent && self.peek() == Some('\t') {
                // a less indented line that is not empty ends the scalar,
                // unless the rest of the line is whitespace only.
                let rest = self.rest();
                let line = &rest[..rest.find(['\n', '\r']).unwrap_or(rest.len())];
                if line.trim_matches([' ', '\t']).is_empty() {
                    while is_blank(self.peek()) {
                        self.skip();
                    }
                } else {
                    return self.error("tabs are not allowed as indentation");
                }
            }
            if !is_break(self.peek()) {
                return Ok(());
            }
            self.read_break(breaks);
        }
    }

    /// Finds the end of a run of characters of a quoted scalar without
    /// blanks, line breaks, quotes and (in double quoted scalars) escapes.
    ///
    /// Returns the offset of the end and the number of characters.
    #[inline]
    fn quoted_scalar_run(&self, single: bool) -> (usize, usize) {
        let bytes = self.input.as_bytes();
        let quote = if single { b'\'' } else { b'"' };
        let mut pos = self.mark.offset;
        let mut columns = 0;
        while let Some(&b) = bytes.get(pos) {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') || b == quote || (b == b'\\' && !single) {
                break;
            }
            // count characters, not UTF-8 continuation bytes
            columns += usize::from(b & 0xc0 != 0x80);
            pos += 1;
        }
        (pos, columns)
    }

    fn scan_flow_scalar(&mut self, single: bool) -> Result<Token<'a>, Error> {
        let start = self.mark;
        self.skip();
        let content_start = self.mark.offset;
        // The value is a slice of the input as long as it has no escapes and
        // no line breaks.  Afterwards it's copied into `owned`.
        let mut owned: Option<String> = None;
        let input = self.input;
        let to_owned = |owned: &mut Option<String>, end: usize| {
            owned
                .take()
                .unwrap_or_else(|| input[content_start..end].to_string())
        };

        loop {
            if self.is_document_indicator() {
                return self.error("unexpected document marker in quoted scalar");
            }
            if self.peek().is_none() {
                return self.error("unexpected end of stream in quoted scalar");
            }

            let mut leading_blanks = false;
            // non blank characters
            loop {
                let run_start = self.mark.offset;
                let (run_end, run_columns) = self.quoted_scalar_run(single);
                if run_end != run_start {
                    if let Some(ref mut string) = owned {
                        string.push_str(&self.input[run_start..run_end]);
                    }
                    // the run has no blanks or breaks, see `skip`
                    self.mark.offset = run_end;
                    self.mark.column += run_columns;
                    self.line_has_content = true;
                    self.ws_has_tab = false;
                }
                match self.peek() {
                    Some('\'') if single && self.peek_at(1) == Some('\'') => {
                        let mut string = to_owned(&mut owned, self.mark.offset);
                        string.push('\'');
                        owned = Some(string);
                        self.skip();
                        self.skip();
                    }
                    Some('\\') if !single && is_break(self.peek_at(1)) => {
                        owned = Some(to_owned(&mut owned, self.mark.offset));
                        self.skip();
                        self.skip_break();
                        leading_blanks = true;
                        break;
                    }
                    Some('\\') if !single => {
                        let mut string = to_owned(&mut owned, self.mark.offset);
                        self.scan_escape(&mut string)?;
                        owned = Some(string);
                    }
                    // quotes, blanks, breaks and the end
                    _ => break,
                }
            }

            match self.peek() {
                Some('\'') if single => break,
                Some('"') if !single => break,
                _ => {}
            }

            // whitespace and line breaks
            let blanks_start = self.mark.offset;
            // `true` if there is a line break that is not escaped
            let mut leading_break = false;
            let mut trailing_breaks = 0;
            while is_blank_or_break(self.peek()) {
                if is_blank(self.peek()) {
                    self.skip();
                } else {
                    // blanks before the break are not part of the value
                    owned = Some(to_owned(&mut owned, blanks_start));
                    self.skip_break();
                    if leading_blanks {
                        trailing_breaks += 1;
                    } else {
                        leading_break = true;
                        leading_blanks = true;
                    }
                }
            }

            if leading_blanks {
                // continuation lines must be indented
                if self.flow_level() == 0
                    && self.peek().is_some()
                    && (self.line_indent as isize) <= self.indent
                {
                    return self.error("invalid indentation of quoted scalar continuation");
                }
                // a line break makes the value owned
                let string = owned.as_mut().unwrap();
                if leading_break && trailing_breaks == 0 {
                    string.push(' ');
                } else {
                    string.extend(std::iter::repeat_n('\n', trailing_breaks));
                }
            } else if let Some(ref mut string) = owned {
                // blanks on the same line are copied as is
                string.push_str(&self.input[blanks_start..self.mark.offset]);
            }
        }

        let value = match owned {
            Some(string) => Cow::Owned(string),
            None => Cow::Borrowed(&self.input[content_start..self.mark.offset]),
        };
        self.skip();
        Ok(Token {
            kind: TokenKind::Scalar(
                if single {
                    ScalarStyle::SingleQuoted
                } else {
                    ScalarStyle::DoubleQuoted
                },
                value,
            ),
            start,
            end: self.mark,
        })
    }

    fn scan_escape(&mut self, out: &mut String) -> Result<(), Error> {
        let escape_mark = self.mark;
        self.skip();
        let c = match self.peek() {
            Some(c) => c,
            None => return self.error("unexpected end of stream in escape sequence"),
        };
        self.skip();
        let len = match c {
            '0' => {
                out.push('\0');
                0
            }
            'a' => {
                out.push('\x07');
                0
            }
            'b' => {
                out.push('\x08');
                0
            }
            't' | '\t' => {
                out.push('\t');
                0
            }
            'n' => {
                out.push('\n');
                0
            }
            'v' => {
                out.push('\x0b');
                0
            }
            'f' => {
                out.push('\x0c');
                0
            }
            'r' => {
                out.push('\r');
                0
            }
            'e' => {
                out.push('\x1b');
                0
            }
            ' ' => {
                out.push(' ');
                0
            }
            '"' => {
                out.push('"');
                0
            }
            '/' => {
                out.push('/');
                0
            }
            '\\' => {
                out.push('\\');
                0
            }
            'N' => {
                out.push('\u{85}');
                0
            }
            '_' => {
                out.push('\u{a0}');
                0
            }
            'L' => {
                out.push('\u{2028}');
                0
            }
            'P' => {
                out.push('\u{2029}');
                0
            }
            'x' => 2,
            'u' => 4,
            'U' => 8,
            _ => return Err(syntax_error(escape_mark, "invalid escape sequence")),
        };
        if len > 0 {
            let hex = self.rest().get(..len).unwrap_or("");
            let code = match u32::from_str_radix(hex, 16) {
                Ok(code) if hex.len() == len && hex.bytes().all(|b| b.is_ascii_hexdigit()) => code,
                _ => return Err(syntax_error(escape_mark, "invalid escape sequence")),
            };
            match char::from_u32(code) {
                Some(c) => out.push(c),
                None => return Err(syntax_error(escape_mark, "invalid unicode escape")),
            }
            for _ in 0..len {
                self.skip();
            }
        }
        Ok(())
    }

    /// Finds the end of a run of characters of a plain scalar (up to
    /// whitespace, a line break or an indicator that ends the scalar).
    ///
    /// Returns the offset of the end and the number of characters.
    #[inline]
    fn plain_scalar_run(&self, in_flow: bool) -> (usize, usize) {
        let bytes = self.input.as_bytes();
        let mut pos = self.mark.offset;
        let mut columns = 0;
        while let Some(&b) = bytes.get(pos) {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' => break,
                // the character after the colon is ASCII if it ends the
                // scalar
                b':' if matches!(
                    bytes.get(pos + 1),
                    None | Some(b' ' | b'\t' | b'\n' | b'\r')
                ) || (in_flow
                    && matches!(bytes.get(pos + 1), Some(b',' | b'[' | b']' | b'{' | b'}'))) =>
                {
                    break;
                }
                b',' | b'[' | b']' | b'{' | b'}' if in_flow => break,
                _ => {}
            }
            // count characters, not UTF-8 continuation bytes
            columns += usize::from(b & 0xc0 != 0x80);
            pos += 1;
        }
        (pos, columns)
    }

    fn scan_plain_scalar(&mut self) -> Result<Token<'a>, Error> {
        let start = self.mark;
        let mut end = self.mark;
        let min_indent = self.indent + 1;
        let in_flow = self.flow_level() > 0;
        // The value is a slice of the input unless lines are folded.  Only
        // then it's copied into `folded`.
        let mut folded: Option<String> = None;
        // the number of line breaks after the first one of a fold
        let mut trailing_breaks = 0;
        let mut leading_blanks = false;

        loop {
            if self.is_document_indicator() || self.peek() == Some('#') {
                break;
            }

            let run_start = self.mark;
            let (run_end, run_columns) = self.plain_scalar_run(in_flow);
            if run_end != run_start.offset {
                // joining the whitespace before the run.  On the same line
                // it's copied as is (the input already contains it).
                if leading_blanks {
                    let string = folded
                        .get_or_insert_with(|| self.input[start.offset..end.offset].to_string());
                    if trailing_breaks == 0 {
                        string.push(' ');
                    } else {
                        string.extend(std::iter::repeat_n('\n', trailing_breaks));
                    }
                    trailing_breaks = 0;
                    leading_blanks = false;
                } else if let Some(ref mut string) = folded {
                    string.push_str(&self.input[end.offset..run_start.offset]);
                }
                if let Some(ref mut string) = folded {
                    string.push_str(&self.input[run_start.offset..run_end]);
                }
                // the run has no blanks or breaks, see `skip`
                self.mark.offset = run_end;
                self.mark.column += run_columns;
                self.line_has_content = true;
                self.ws_has_tab = false;
                end = self.mark;
            }

            if !is_blank_or_break(self.peek()) {
                break;
            }

            while is_blank_or_break(self.peek()) {
                if is_blank(self.peek()) {
                    self.skip();
                } else {
                    self.skip_break();
                    if leading_blanks {
                        trailing_breaks += 1;
                    } else {
                        leading_blanks = true;
                    }
                }
            }

            // continuation lines in block context must be indented more
            // than the parent block (tabs do not count as indentation)
            if leading_blanks && !in_flow && (self.line_indent as isize) < min_indent {
                break;
            }
        }

        if leading_blanks {
            self.simple_key_allowed = true;
        }

        let value = match folded {
            Some(string) => Cow::Owned(string),
            None => Cow::Borrowed(&self.input[start.offset..end.offset]),
        };
        Ok(Token {
            kind: TokenKind::Scalar(ScalarStyle::Plain, value),
            start,
            end,
        })
    }
}
