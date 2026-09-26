//! This crate provides source locations (line and column) for deser.
//!
//! Formats publish the byte range in the input of every event they emit
//! into the [`State`] (see [`State::input_range`]) and, if location tracking
//! is requested, the source these ranges refer to (see [`State::source`]).
//! This crate resolves the ranges into lines and columns with a
//! [`SourceMap`] (see [`Locations`]) and types can pick them up while they
//! are deserialized.  The simplest way to do that is the [`Spanned`]
//! wrapper:
//!
//! ```
//! # use deser::Deserialize;
//! use deser_location::Spanned;
//!
//! #[derive(Deserialize)]
//! struct Config {
//!     name: Spanned<String>,
//! }
//! ```
//!
//! # Implementing Location Support in Formats
//!
//! Formats do not depend on this crate.  They publish the byte range of
//! every event with [`State::set_input_range`] before they emit it and set
//! the source with [`State::set_source`] if locations are requested.  The source map is built when a consumer asks for a location
//! for the first time:
//!
//! ```
//! use deser::de::DeserializeDriver;
//! use deser::Event;
//! use deser_location::Spanned;
//!
//! let input = "true";
//! let mut out = None::<Spanned<bool>>;
//! {
//!     let mut driver = DeserializeDriver::new(&mut out);
//!     driver.state_mut().set_source(input);
//!     driver.state_mut().set_input_range(0, 4);
//!     driver.emit(Event::from(true)).unwrap();
//! }
//! let span = out.unwrap().span.unwrap();
//! assert_eq!((span.start.line, span.start.column), (1, 1));
//! assert_eq!((span.end.line, span.end.column), (1, 5));
//! ```
//!
//! # Buffering
//!
//! Values that are internally buffered with a
//! [`Recording`](deser::de::Recording) (as some enum representations do)
//! retain their locations when they are replayed as recordings capture the
//! input range of every event.
use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use deser::State;
use deser::de::{Deserialize, OwnedSink, Sink, SinkHandle};
use deser::ser::{Chunk, Describe, Serialize};
use deser::{Atom, ContainerShape, Error};

/// A position in the input.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    /// The byte offset from the start of the input.
    pub offset: usize,
    /// The line number (1-based).
    pub line: usize,
    /// The column number in characters (1-based).
    pub column: usize,
}

impl Default for Position {
    fn default() -> Position {
        Position {
            offset: 0,
            line: 1,
            column: 1,
        }
    }
}

impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// A range in the input.
///
/// The end position is exclusive.
#[derive(Copy, Clone, Default, PartialEq, Eq, Hash)]
pub struct Span {
    /// The start of the span.
    pub start: Position,
    /// The end of the span (exclusive).
    pub end: Position,
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}-{:?}", self.start, self.end)
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.start, self.end)
    }
}

/// Maps byte offsets in a source to positions.
///
/// The line index is only built when a position is resolved for the first
/// time.
pub struct SourceMap {
    source: Arc<str>,
    line_starts: OnceLock<Vec<usize>>,
    // the line index (0-based) of the last lookup.  Lookups tend to be
    // monotonic so this avoids most binary searches.
    last_line: AtomicUsize,
}

impl fmt::Debug for SourceMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceMap")
            .field("len", &self.source.len())
            .finish()
    }
}

impl SourceMap {
    /// Creates a source map for the given source.
    pub fn new<S: Into<Arc<str>>>(source: S) -> SourceMap {
        SourceMap {
            source: source.into(),
            line_starts: OnceLock::new(),
            last_line: AtomicUsize::new(0),
        }
    }

    /// Returns the source.
    pub fn source(&self) -> &str {
        &self.source
    }

    fn line_starts(&self) -> &[usize] {
        self.line_starts.get_or_init(|| {
            let mut rv = vec![0];
            find_newlines(self.source.as_bytes(), |idx| rv.push(idx + 1));
            rv
        })
    }

    /// Returns the index (0-based) of the line containing the offset.
    fn line_index(&self, offset: usize) -> usize {
        let line_starts = self.line_starts();
        let contains = |idx: usize| {
            line_starts[idx] <= offset
                && offset < line_starts.get(idx + 1).copied().unwrap_or(usize::MAX)
        };
        let hint = self.last_line.load(Ordering::Relaxed);
        let idx = if hint < line_starts.len() && contains(hint) {
            hint
        } else if hint + 1 < line_starts.len() && contains(hint + 1) {
            hint + 1
        } else {
            line_starts.partition_point(|&start| start <= offset) - 1
        };
        self.last_line.store(idx, Ordering::Relaxed);
        idx
    }

    /// Resolves a byte offset into a position.
    ///
    /// Offsets beyond the end of the source are clamped.
    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.source.len());
        let idx = self.line_index(offset);
        let line_start = self.line_starts()[idx];
        Position {
            offset,
            line: idx + 1,
            column: 1 + count_chars(&self.source.as_bytes()[line_start..offset]),
        }
    }

    /// Resolves a range of byte offsets into a span.
    pub fn span(&self, start: usize, end: usize) -> Span {
        let start = self.position(start);
        let end = end.min(self.source.len()).max(start.offset);
        // most spans are on a single line, resolve the end relative to the
        // start in that case.
        let bytes = &self.source.as_bytes()[start.offset..end];
        let end = if bytes.contains(&b'\n') {
            self.position(end)
        } else {
            Position {
                offset: end,
                line: start.line,
                column: start.column + count_chars(bytes),
            }
        };
        Span { start, end }
    }
}

/// Location information in the [`State`].
///
/// This resolves the input ranges of the events (see
/// [`State::input_range`]) into lines and columns with a [`SourceMap`] of the
/// source (see [`State::source`]).  The source map is built on first use and
/// cached in the state.  Consumers retrieve the resolved span of the current
/// event with [`current_span`](Self::current_span).
#[derive(Debug, Default, Clone)]
pub struct Locations {
    source_map: Option<Arc<SourceMap>>,
}

impl Locations {
    /// Returns the source map for the source in the state.
    ///
    /// Returns `None` if the format does not provide the source.
    pub fn source_map(state: &mut State) -> Option<Arc<SourceMap>> {
        Locations::cached_source_map(state).cloned()
    }

    /// Returns the span of the current event if the format provides it.
    pub fn current_span(state: &mut State) -> Option<Span> {
        let range = state.input_range()?;
        let source_map = Locations::cached_source_map(state)?;
        Some(source_map.span(range.start, range.end))
    }

    fn cached_source_map(state: &mut State) -> Option<&Arc<SourceMap>> {
        let source = state.source()?;
        let is_cached = matches!(
            state.get::<Locations>(),
            Some(Locations { source_map: Some(source_map) })
                if Arc::ptr_eq(&source_map.source, source)
        );
        if !is_cached {
            let source_map = SourceMap::new(source.clone());
            state.get_mut::<Locations>().source_map = Some(Arc::new(source_map));
        }
        state.get::<Locations>()?.source_map.as_ref()
    }
}

// The helpers below process the input a word at a time as the tracker is
// invoked for every event.

const LO7: u64 = 0x7f7f_7f7f_7f7f_7f7f;
const HI: u64 = 0x8080_8080_8080_8080;
const NEWLINES: u64 = 0x0a0a_0a0a_0a0a_0a0a;

/// Sets the high bit of every byte that is zero (exact, no false positives).
fn zero_bytes(x: u64) -> u64 {
    !(((x & LO7).wrapping_add(LO7)) | x | LO7)
}

/// Invokes the callback with the index of every newline.
fn find_newlines<F: FnMut(usize)>(bytes: &[u8], mut f: F) {
    let (chunks, rest) = bytes.as_chunks::<8>();
    for (idx, &chunk) in chunks.iter().enumerate() {
        let mut mask = zero_bytes(u64::from_le_bytes(chunk) ^ NEWLINES);
        while mask != 0 {
            f(idx * 8 + mask.trailing_zeros() as usize / 8);
            mask &= mask - 1;
        }
    }
    let offset = bytes.len() - rest.len();
    for (idx, &byte) in rest.iter().enumerate() {
        if byte == b'\n' {
            f(offset + idx);
        }
    }
}

/// Counts the characters (bytes that are not utf-8 continuation bytes).
fn count_chars(bytes: &[u8]) -> usize {
    let (chunks, rest) = bytes.as_chunks::<8>();
    let mut continuation = 0;
    for &chunk in chunks {
        let w = u64::from_le_bytes(chunk);
        // high bit set and the bit below it cleared
        continuation += (w & !(w << 1) & HI).count_ones() as usize;
    }
    continuation += rest.iter().filter(|&&b| b & 0xc0 == 0x80).count();
    bytes.len() - continuation
}

/// A value together with its location in the input.
///
/// For primitive values the span covers the value, for maps and sequences
/// it covers everything from the opening to the closing token.  The span is
/// `None` if the format does not provide locations.
///
/// When serialized, only the value is serialized.  The debug representation
/// is the value followed by its span, e.g. `42 (@ 3:5-3:7)`.
#[derive(Clone, PartialEq)]
pub struct Spanned<T> {
    /// The value.
    pub value: T,
    /// The location of the value in the input.
    pub span: Option<Span>,
}

impl<T: fmt::Debug> fmt::Debug for Spanned<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.value, f)?;
        match self.span {
            Some(span) => write!(f, " (@ {:?})", span),
            None => Ok(()),
        }
    }
}

impl<T> Spanned<T> {
    /// Creates a new spanned value.
    pub fn new(value: T, span: Option<Span>) -> Spanned<T> {
        Spanned { value, span }
    }

    /// Returns the inner value.
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Spanned<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(SpannedSink {
            out,
            slot: None,
            compound: None,
            span: None,
        })
    }
}

struct SpannedSink<'a, 'de, T> {
    out: &'a mut Option<Spanned<T>>,
    // primitive values are deserialized directly into this slot, maps and
    // sequences need a sink that lives across calls
    slot: Option<T>,
    compound: Option<OwnedSink<'de, T>>,
    span: Option<Span>,
}

impl<'a, 'de, T: Deserialize<'de>> SpannedSink<'a, 'de, T> {
    fn compound(&mut self) -> &mut dyn Sink<'de> {
        self.compound
            .get_or_insert_with(OwnedSink::deserialize)
            .borrow_mut()
    }
}

impl<'a, 'de, T: Deserialize<'de>> Sink<'de> for SpannedSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.span = Locations::current_span(state);
        let mut sink = T::deserialize_into(&mut self.slot);
        sink.atom(atom, state)?;
        sink.finish(state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.span = Locations::current_span(state);
        let mut sink = T::deserialize_into(&mut self.slot);
        sink.borrowed_atom(atom, state)?;
        sink.finish(state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.span = Locations::current_span(state);
        self.compound().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.span = Locations::current_span(state);
        self.compound().seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.compound().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.compound().next_value(state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.compound().value_for_key(key, state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        let value = match self.compound {
            Some(ref mut compound) => {
                compound.borrow_mut().finish(state)?;
                // containers are finished on their closing token, extend the
                // span to it
                if let (Some(start), Some(end)) = (self.span, Locations::current_span(state)) {
                    self.span = Some(Span {
                        start: start.start,
                        end: end.end,
                    });
                }
                compound.take()
            }
            None => self.slot.take(),
        };
        let span = self.span;
        *self.out = value.map(|value| Spanned { value, span });
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        if let Some(ref compound) = self.compound {
            return compound.borrow().expecting();
        }
        let mut slot = None;
        Cow::Owned(T::deserialize_into(&mut slot).expecting().into_owned())
    }
}

impl<T: Serialize> Serialize for Spanned<T> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        self.value.serialize(state)
    }

    fn finish(&self, state: &mut State) -> Result<(), Error> {
        self.value.finish(state)
    }

    fn is_optional(&self) -> bool {
        self.value.is_optional()
    }

    fn container_shape(&self) -> ContainerShape {
        self.value.container_shape()
    }

    fn describe(&self, d: &mut dyn Describe) {
        self.value.describe(d)
    }
}

#[test]
fn test_helpers() {
    let mut state = 0x2545f4914f6cdd1du64;
    let alphabet = "ab\n\u{e4}\u{1f600}x\n".as_bytes();
    let rounds = if cfg!(miri) { 1 } else { 50 };
    for len in 0..64 {
        for _ in 0..rounds {
            let input: Vec<u8> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    alphabet[(state % alphabet.len() as u64) as usize]
                })
                .collect();
            let mut newlines = Vec::new();
            find_newlines(&input, |idx| newlines.push(idx));
            let expected: Vec<usize> = input
                .iter()
                .enumerate()
                .filter(|&(_, &b)| b == b'\n')
                .map(|(idx, _)| idx)
                .collect();
            assert_eq!(newlines, expected);
            assert_eq!(
                count_chars(&input),
                input.iter().filter(|&&b| b & 0xc0 != 0x80).count()
            );
        }
    }
}

#[test]
fn test_source_map() {
    let source_map = SourceMap::new("ab\ncäd\n\nx");
    let pos = |offset| format!("{:?}", source_map.position(offset));
    assert_eq!(pos(0), "1:1");
    assert_eq!(pos(2), "1:3");
    assert_eq!(pos(3), "2:1");
    // ä is two bytes
    assert_eq!(pos(6), "2:3");
    assert_eq!(pos(7), "2:4");
    assert_eq!(pos(8), "3:1");
    assert_eq!(pos(9), "4:1");
    assert_eq!(pos(100), "4:2");
    assert_eq!(format!("{:?}", source_map.span(3, 7)), "2:1-2:4");
}

#[test]
fn test_debug() {
    let span = SourceMap::new("[1, 23]").span(4, 6);
    assert_eq!(
        format!("{:?}", Spanned::new(23, Some(span))),
        "23 (@ 1:5-1:7)"
    );
    assert_eq!(format!("{:?}", Spanned::new("x", None)), "\"x\"");
    assert_eq!(
        format!("{:#?}", Spanned::new(vec![1], Some(span))),
        "[\n    1,\n] (@ 1:5-1:7)"
    );
}

#[test]
fn test_auto_traits() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SourceMap>();
    assert_send_sync::<Locations>();
    assert_send_sync::<Spanned<String>>();
}
