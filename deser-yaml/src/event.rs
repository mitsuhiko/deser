//! The YAML event model.
//!
//! This is the output of the parser and corresponds to the serialization
//! tree of the YAML specification (and the event stream of the YAML test
//! suite).  It is internal: users only ever see deser events.
use std::borrow::Cow;

/// A position in the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mark {
    /// Byte offset.
    pub offset: usize,
    /// Zero based line.
    pub line: usize,
    /// Zero based column in characters.
    pub column: usize,
}

/// The presentation style of a scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarStyle {
    Plain,
    SingleQuoted,
    DoubleQuoted,
    Literal,
    Folded,
}

/// The properties of a node.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Props<'a> {
    pub anchor: Option<Cow<'a, str>>,
    /// The resolved tag (tag handles are already expanded).  The
    /// non-specific tag `!` is represented as `"!"`.
    pub tag: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind<'a> {
    StreamStart,
    StreamEnd,
    DocumentStart {
        /// `---` was present.
        explicit: bool,
        /// The version declared with the `%YAML` directive.
        version: Option<(u32, u32)>,
    },
    DocumentEnd {
        /// `...` was present.
        explicit: bool,
    },
    SequenceStart {
        props: Props<'a>,
        flow: bool,
    },
    SequenceEnd,
    MappingStart {
        props: Props<'a>,
        flow: bool,
    },
    MappingEnd,
    Scalar {
        props: Props<'a>,
        style: ScalarStyle,
        value: Cow<'a, str>,
    },
    Alias {
        anchor: Cow<'a, str>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event<'a> {
    pub kind: EventKind<'a>,
    pub start: Mark,
    pub end: Mark,
}
