//! The YAML parser: turns tokens into [`Event`]s.
//!
//! This is a state machine with an explicit stack so that deeply nested
//! documents do not consume native stack space.
use std::borrow::Cow;

use deser::{Error, ErrorKind};

use crate::event::{Event, EventKind, Mark, Props, ScalarStyle};
use crate::scanner::{Scanner, Token, TokenKind, TokenType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    StreamStart,
    ImplicitDocumentStart,
    DocumentStart,
    DocumentContent,
    DocumentEnd,
    BlockNode,
    BlockSequenceFirstEntry,
    BlockSequenceEntry,
    IndentlessSequenceEntry,
    BlockMappingFirstKey,
    BlockMappingKey,
    BlockMappingValue,
    FlowSequenceFirstEntry,
    FlowSequenceEntry,
    FlowSequenceEntryMappingKey,
    FlowSequenceEntryMappingValue,
    FlowSequenceEntryMappingEnd,
    FlowMappingFirstKey,
    FlowMappingKey,
    FlowMappingValue,
    FlowMappingEmptyValue,
    End,
}

pub struct Parser<'a> {
    scanner: Scanner<'a>,
    state: State,
    states: Vec<State>,
    /// The tag handles of the current document.
    tag_handles: Vec<(Cow<'a, str>, Cow<'a, str>)>,
    /// The end of the last event.  Block collections end there.
    last_end: Mark,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Parser<'a> {
        Parser {
            scanner: Scanner::new(input),
            state: State::StreamStart,
            states: Vec::new(),
            tag_handles: Vec::new(),
            last_end: Mark::default(),
        }
    }

    /// Returns the next event.
    ///
    /// After [`EventKind::StreamEnd`] or an error, no more events must be
    /// requested.
    pub fn next_event(&mut self) -> Result<Event<'a>, Error> {
        let event = self.parse_event()?;
        self.last_end = event.end;
        Ok(event)
    }

    fn parse_event(&mut self) -> Result<Event<'a>, Error> {
        match self.state {
            State::StreamStart => self.parse_stream_start(),
            State::ImplicitDocumentStart => self.parse_document_start(true),
            State::DocumentStart => self.parse_document_start(false),
            State::DocumentContent => self.parse_document_content(),
            State::DocumentEnd => self.parse_document_end(),
            State::BlockNode => self.parse_node(true, false),
            State::BlockSequenceFirstEntry => self.parse_block_sequence_entry(true),
            State::BlockSequenceEntry => self.parse_block_sequence_entry(false),
            State::IndentlessSequenceEntry => self.parse_indentless_sequence_entry(),
            State::BlockMappingFirstKey => self.parse_block_mapping_key(true),
            State::BlockMappingKey => self.parse_block_mapping_key(false),
            State::BlockMappingValue => self.parse_block_mapping_value(),
            State::FlowSequenceFirstEntry => self.parse_flow_sequence_entry(true),
            State::FlowSequenceEntry => self.parse_flow_sequence_entry(false),
            State::FlowSequenceEntryMappingKey => self.parse_flow_sequence_entry_mapping_key(),
            State::FlowSequenceEntryMappingValue => self.parse_flow_sequence_entry_mapping_value(),
            State::FlowSequenceEntryMappingEnd => self.parse_flow_sequence_entry_mapping_end(),
            State::FlowMappingFirstKey => self.parse_flow_mapping_key(true),
            State::FlowMappingKey => self.parse_flow_mapping_key(false),
            State::FlowMappingValue => self.parse_flow_mapping_value(false),
            State::FlowMappingEmptyValue => self.parse_flow_mapping_value(true),
            State::End => panic!("next_event called after the end of the stream"),
        }
    }

    // -- helpers ----------------------------------------------------------

    fn peek(&mut self) -> Result<TokenType, Error> {
        self.scanner.peek_type()
    }

    fn peek_mark(&mut self) -> Result<Mark, Error> {
        Ok(self.scanner.peek_token()?.start)
    }

    fn next(&mut self) -> Token<'a> {
        self.scanner.next_token()
    }

    fn pop_state(&mut self) {
        self.state = self.states.pop().unwrap();
    }

    fn event(kind: EventKind<'a>, start: Mark, end: Mark) -> Result<Event<'a>, Error> {
        Ok(Event { kind, start, end })
    }

    fn empty_scalar(mark: Mark, props: Props<'a>) -> Result<Event<'a>, Error> {
        Self::event(
            EventKind::Scalar {
                props,
                style: ScalarStyle::Plain,
                value: Cow::Borrowed(""),
            },
            mark,
            mark,
        )
    }

    fn error<T>(&mut self, msg: &str) -> Result<T, Error> {
        let mark = self.peek_mark()?;
        Err(syntax_error(mark, msg))
    }

    // -- stream and documents ---------------------------------------------

    fn parse_stream_start(&mut self) -> Result<Event<'a>, Error> {
        self.peek()?;
        let token = self.next();
        debug_assert_eq!(token.kind, TokenKind::StreamStart);
        self.state = State::ImplicitDocumentStart;
        Self::event(EventKind::StreamStart, token.start, token.end)
    }

    /// Parses the start of a document.  `implicit` is `true` if a bare
    /// document (without `---`) is allowed here: at the start of the stream
    /// and after a document end marker.
    fn parse_document_start(&mut self, implicit: bool) -> Result<Event<'a>, Error> {
        let mut implicit = implicit;
        // extra document end markers
        while self.peek()? == TokenType::DocumentEnd {
            self.next();
            implicit = true;
        }

        match self.peek()? {
            TokenType::StreamEnd => {
                let token = self.next();
                self.state = State::End;
                Self::event(EventKind::StreamEnd, token.start, token.end)
            }
            TokenType::VersionDirective
            | TokenType::TagDirective
            | TokenType::ReservedDirective
            | TokenType::DocumentStart => {
                let start = self.peek_mark()?;
                if self.peek()? != TokenType::DocumentStart && !implicit {
                    return self.error("missing document end marker before directive");
                }
                let version = self.parse_directives()?;
                if self.peek()? != TokenType::DocumentStart {
                    return self.error("did not find expected <document start>");
                }
                let token = self.next();
                self.states.push(State::DocumentEnd);
                self.state = State::DocumentContent;
                Self::event(
                    EventKind::DocumentStart {
                        explicit: true,
                        version,
                    },
                    start,
                    token.end,
                )
            }
            _ if implicit => {
                let mark = self.peek_mark()?;
                self.reset_tag_handles();
                self.states.push(State::DocumentEnd);
                self.state = State::BlockNode;
                Self::event(
                    EventKind::DocumentStart {
                        explicit: false,
                        version: None,
                    },
                    mark,
                    mark,
                )
            }
            _ => self.error("did not find expected <document start>"),
        }
    }

    fn reset_tag_handles(&mut self) {
        self.tag_handles.clear();
        self.tag_handles
            .push((Cow::Borrowed("!"), Cow::Borrowed("!")));
        self.tag_handles
            .push((Cow::Borrowed("!!"), Cow::Borrowed("tag:yaml.org,2002:")));
    }

    /// Parses the directives of a document and returns the version.
    fn parse_directives(&mut self) -> Result<Option<(u32, u32)>, Error> {
        self.reset_tag_handles();
        let mut version = None;
        let mut custom_handles = Vec::new();
        loop {
            match self.peek()? {
                TokenType::VersionDirective => {
                    let token = self.next();
                    if version.is_some() {
                        return Err(syntax_error(token.start, "duplicate %YAML directive"));
                    }
                    if let TokenKind::VersionDirective(major, minor) = token.kind {
                        if major != 1 {
                            return Err(syntax_error(
                                token.start,
                                "incompatible YAML document version",
                            ));
                        }
                        version = Some((major, minor));
                    }
                }
                TokenType::TagDirective => {
                    let token = self.next();
                    if let TokenKind::TagDirective(handle, prefix) = token.kind {
                        if custom_handles.contains(&handle) {
                            return Err(syntax_error(token.start, "duplicate %TAG directive"));
                        }
                        custom_handles.push(handle.clone());
                        self.tag_handles.retain(|(h, _)| *h != handle);
                        self.tag_handles.push((handle, prefix));
                    }
                }
                TokenType::ReservedDirective => {
                    self.next();
                }
                _ => return Ok(version),
            }
        }
    }

    fn parse_document_content(&mut self) -> Result<Event<'a>, Error> {
        match self.peek()? {
            TokenType::VersionDirective
            | TokenType::TagDirective
            | TokenType::ReservedDirective
            | TokenType::DocumentStart
            | TokenType::DocumentEnd
            | TokenType::StreamEnd => {
                let mark = self.peek_mark()?;
                self.pop_state();
                Self::empty_scalar(mark, Props::default())
            }
            _ => self.parse_node(true, false),
        }
    }

    fn parse_document_end(&mut self) -> Result<Event<'a>, Error> {
        let start = self.peek_mark()?;
        let mut end = start;
        let explicit = self.peek()? == TokenType::DocumentEnd;
        if explicit {
            end = self.next().end;
            self.state = State::ImplicitDocumentStart;
        } else {
            // without a document end marker, only an explicit document
            // (or the end of the stream) may follow
            match self.peek()? {
                TokenType::DocumentStart | TokenType::StreamEnd => {}
                TokenType::VersionDirective
                | TokenType::TagDirective
                | TokenType::ReservedDirective => {
                    return self.error("missing document end marker before directive");
                }
                _ => return self.error("did not find expected <document start>"),
            }
            self.state = State::DocumentStart;
        }
        Self::event(EventKind::DocumentEnd { explicit }, start, end)
    }

    // -- nodes ------------------------------------------------------------

    fn resolve_tag(
        &self,
        handle: Cow<'a, str>,
        suffix: Cow<'a, str>,
        mark: Mark,
    ) -> Result<Cow<'a, str>, Error> {
        if handle.is_empty() {
            // verbatim
            return Ok(suffix);
        }
        if handle == "!" && suffix.is_empty() {
            return Ok(Cow::Borrowed("!"));
        }
        for (h, prefix) in &self.tag_handles {
            if *h == handle {
                return Ok(Cow::Owned(format!("{}{}", prefix, suffix)));
            }
        }
        Err(syntax_error(mark, "found undefined tag handle"))
    }

    fn parse_node(&mut self, block: bool, indentless_sequence: bool) -> Result<Event<'a>, Error> {
        if self.peek()? == TokenType::Alias {
            let token = self.next();
            self.pop_state();
            if let TokenKind::Alias(anchor) = token.kind {
                return Self::event(EventKind::Alias { anchor }, token.start, token.end);
            }
            unreachable!();
        }

        let start = self.peek_mark()?;
        let mut props = Props::default();
        let mut has_props = false;
        loop {
            match self.peek()? {
                TokenType::Anchor => {
                    if props.anchor.is_some() {
                        return self.error("a node can only have one anchor");
                    }
                    if let TokenKind::Anchor(name) = self.next().kind {
                        props.anchor = Some(name);
                    }
                }
                TokenType::Tag => {
                    if props.tag.is_some() {
                        return self.error("a node can only have one tag");
                    }
                    let token = self.next();
                    if let TokenKind::Tag(handle, suffix) = token.kind {
                        props.tag = Some(self.resolve_tag(handle, suffix, token.start)?);
                    }
                }
                _ => break,
            }
            has_props = true;
        }

        let mark = self.peek_mark()?;
        match self.peek()? {
            TokenType::BlockEntry if indentless_sequence => {
                self.state = State::IndentlessSequenceEntry;
                Self::event(EventKind::SequenceStart { props, flow: false }, start, mark)
            }
            TokenType::Scalar => {
                let token = self.next();
                self.pop_state();
                if let TokenKind::Scalar(style, value) = token.kind {
                    return Self::event(
                        EventKind::Scalar {
                            props,
                            style,
                            value,
                        },
                        start,
                        token.end,
                    );
                }
                unreachable!();
            }
            TokenType::FlowSequenceStart => {
                self.state = State::FlowSequenceFirstEntry;
                Self::event(EventKind::SequenceStart { props, flow: true }, start, mark)
            }
            TokenType::FlowMappingStart => {
                self.state = State::FlowMappingFirstKey;
                Self::event(EventKind::MappingStart { props, flow: true }, start, mark)
            }
            TokenType::BlockSequenceStart if block => {
                self.state = State::BlockSequenceFirstEntry;
                Self::event(EventKind::SequenceStart { props, flow: false }, start, mark)
            }
            TokenType::BlockMappingStart if block => {
                self.state = State::BlockMappingFirstKey;
                Self::event(EventKind::MappingStart { props, flow: false }, start, mark)
            }
            TokenType::Alias if has_props => self.error("an alias cannot have properties"),
            _ if has_props => {
                self.pop_state();
                Self::empty_scalar(mark, props)
            }
            _ => self.error("did not find expected node content"),
        }
    }

    // -- block collections ------------------------------------------------

    fn parse_block_sequence_entry(&mut self, first: bool) -> Result<Event<'a>, Error> {
        if first {
            let token = self.next();
            debug_assert_eq!(token.kind, TokenKind::BlockSequenceStart);
        }
        match self.peek()? {
            TokenType::BlockEntry => {
                let token = self.next();
                match self.peek()? {
                    TokenType::BlockEntry | TokenType::BlockEnd => {
                        self.state = State::BlockSequenceEntry;
                        Self::empty_scalar(token.end, Props::default())
                    }
                    _ => {
                        self.states.push(State::BlockSequenceEntry);
                        self.parse_node(true, false)
                    }
                }
            }
            TokenType::BlockEnd => {
                self.next();
                self.pop_state();
                Self::event(EventKind::SequenceEnd, self.last_end, self.last_end)
            }
            _ => self.error("did not find expected '-' indicator"),
        }
    }

    fn parse_indentless_sequence_entry(&mut self) -> Result<Event<'a>, Error> {
        if self.peek()? == TokenType::BlockEntry {
            let token = self.next();
            match self.peek()? {
                TokenType::BlockEntry | TokenType::Key | TokenType::Value | TokenType::BlockEnd => {
                    self.state = State::IndentlessSequenceEntry;
                    Self::empty_scalar(token.end, Props::default())
                }
                _ => {
                    self.states.push(State::IndentlessSequenceEntry);
                    self.parse_node(true, false)
                }
            }
        } else {
            self.pop_state();
            Self::event(EventKind::SequenceEnd, self.last_end, self.last_end)
        }
    }

    fn parse_block_mapping_key(&mut self, first: bool) -> Result<Event<'a>, Error> {
        if first {
            let token = self.next();
            debug_assert_eq!(token.kind, TokenKind::BlockMappingStart);
        }
        match self.peek()? {
            TokenType::Key => {
                let token = self.next();
                match self.peek()? {
                    TokenType::Key | TokenType::Value | TokenType::BlockEnd => {
                        self.state = State::BlockMappingValue;
                        Self::empty_scalar(token.end, Props::default())
                    }
                    _ => {
                        self.states.push(State::BlockMappingValue);
                        self.parse_node(true, true)
                    }
                }
            }
            TokenType::Value => {
                let mark = self.peek_mark()?;
                self.state = State::BlockMappingValue;
                Self::empty_scalar(mark, Props::default())
            }
            TokenType::BlockEnd => {
                self.next();
                self.pop_state();
                Self::event(EventKind::MappingEnd, self.last_end, self.last_end)
            }
            _ => self.error("did not find expected key"),
        }
    }

    fn parse_block_mapping_value(&mut self) -> Result<Event<'a>, Error> {
        if self.peek()? == TokenType::Value {
            let token = self.next();
            match self.peek()? {
                TokenType::Key | TokenType::Value | TokenType::BlockEnd => {
                    self.state = State::BlockMappingKey;
                    Self::empty_scalar(token.end, Props::default())
                }
                _ => {
                    self.states.push(State::BlockMappingKey);
                    self.parse_node(true, true)
                }
            }
        } else {
            let mark = self.peek_mark()?;
            self.state = State::BlockMappingKey;
            Self::empty_scalar(mark, Props::default())
        }
    }

    // -- flow collections -------------------------------------------------

    fn parse_flow_sequence_entry(&mut self, first: bool) -> Result<Event<'a>, Error> {
        if first {
            self.next();
        }
        if self.peek()? != TokenType::FlowSequenceEnd {
            if !first {
                if self.peek()? == TokenType::FlowEntry {
                    self.next();
                } else {
                    return self.error("did not find expected ',' or ']'");
                }
            }
            match self.peek()? {
                TokenType::Key => {
                    let token = self.next();
                    self.state = State::FlowSequenceEntryMappingKey;
                    return Self::event(
                        EventKind::MappingStart {
                            props: Props::default(),
                            flow: true,
                        },
                        token.start,
                        token.end,
                    );
                }
                TokenType::Value => {
                    // a single pair with an empty key
                    let mark = self.peek_mark()?;
                    self.state = State::FlowSequenceEntryMappingKey;
                    return Self::event(
                        EventKind::MappingStart {
                            props: Props::default(),
                            flow: true,
                        },
                        mark,
                        mark,
                    );
                }
                TokenType::FlowSequenceEnd => {}
                _ => {
                    self.states.push(State::FlowSequenceEntry);
                    return self.parse_node(false, false);
                }
            }
        }
        let token = self.next();
        self.pop_state();
        Self::event(EventKind::SequenceEnd, token.start, token.end)
    }

    fn parse_flow_sequence_entry_mapping_key(&mut self) -> Result<Event<'a>, Error> {
        match self.peek()? {
            TokenType::Value | TokenType::FlowEntry | TokenType::FlowSequenceEnd => {
                let mark = self.peek_mark()?;
                self.state = State::FlowSequenceEntryMappingValue;
                Self::empty_scalar(mark, Props::default())
            }
            _ => {
                self.states.push(State::FlowSequenceEntryMappingValue);
                self.parse_node(false, false)
            }
        }
    }

    fn parse_flow_sequence_entry_mapping_value(&mut self) -> Result<Event<'a>, Error> {
        if self.peek()? == TokenType::Value {
            let token = self.next();
            match self.peek()? {
                TokenType::FlowEntry | TokenType::FlowSequenceEnd => {
                    self.state = State::FlowSequenceEntryMappingEnd;
                    Self::empty_scalar(token.end, Props::default())
                }
                _ => {
                    self.states.push(State::FlowSequenceEntryMappingEnd);
                    self.parse_node(false, false)
                }
            }
        } else {
            let mark = self.peek_mark()?;
            self.state = State::FlowSequenceEntryMappingEnd;
            Self::empty_scalar(mark, Props::default())
        }
    }

    fn parse_flow_sequence_entry_mapping_end(&mut self) -> Result<Event<'a>, Error> {
        let mark = self.peek_mark()?;
        self.state = State::FlowSequenceEntry;
        Self::event(EventKind::MappingEnd, mark, mark)
    }

    fn parse_flow_mapping_key(&mut self, first: bool) -> Result<Event<'a>, Error> {
        if first {
            self.next();
        }
        if self.peek()? != TokenType::FlowMappingEnd {
            if !first {
                if self.peek()? == TokenType::FlowEntry {
                    self.next();
                } else {
                    return self.error("did not find expected ',' or '}'");
                }
            }
            match self.peek()? {
                TokenType::Key => {
                    let token = self.next();
                    return match self.peek()? {
                        TokenType::Value | TokenType::FlowEntry | TokenType::FlowMappingEnd => {
                            self.state = State::FlowMappingValue;
                            Self::empty_scalar(token.end, Props::default())
                        }
                        _ => {
                            self.states.push(State::FlowMappingValue);
                            self.parse_node(false, false)
                        }
                    };
                }
                TokenType::Value => {
                    let mark = self.peek_mark()?;
                    self.state = State::FlowMappingValue;
                    return Self::empty_scalar(mark, Props::default());
                }
                TokenType::FlowMappingEnd => {}
                _ => {
                    self.states.push(State::FlowMappingEmptyValue);
                    return self.parse_node(false, false);
                }
            }
        }
        let token = self.next();
        self.pop_state();
        Self::event(EventKind::MappingEnd, token.start, token.end)
    }

    fn parse_flow_mapping_value(&mut self, empty: bool) -> Result<Event<'a>, Error> {
        let mark = self.peek_mark()?;
        if empty {
            self.state = State::FlowMappingKey;
            return Self::empty_scalar(mark, Props::default());
        }
        if self.peek()? == TokenType::Value {
            let token = self.next();
            match self.peek()? {
                TokenType::FlowEntry | TokenType::FlowMappingEnd => {
                    self.state = State::FlowMappingKey;
                    Self::empty_scalar(token.end, Props::default())
                }
                _ => {
                    self.states.push(State::FlowMappingKey);
                    self.parse_node(false, false)
                }
            }
        } else {
            self.state = State::FlowMappingKey;
            Self::empty_scalar(mark, Props::default())
        }
    }
}

/// Creates an error for a problem at a position that is not a syntax
/// error (for instance an invalid value).
#[cold]
pub fn error_at(mark: Mark, msg: &str) -> Error {
    Error::new(ErrorKind::Unexpected, msg.to_string()).with_position(
        mark.offset,
        mark.line + 1,
        mark.column + 1,
    )
}

#[cold]
pub fn syntax_error(mark: Mark, msg: &str) -> Error {
    Error::new(ErrorKind::Unexpected, format!("syntax error: {}", msg)).with_position(
        mark.offset,
        mark.line + 1,
        mark.column + 1,
    )
}
