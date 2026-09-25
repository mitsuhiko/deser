use std::borrow::Cow;
use std::collections::HashMap;
use std::marker::PhantomData;

use deser::de::{Deserialize, DeserializeDriver};
use deser::{Atom, Error, ErrorKind, Event};

use crate::event::{Event as YamlEvent, EventKind, Mark, ScalarStyle};
use crate::parser::{error_at, Parser};
use crate::resolve::{classify_tag, is_collection_tag, resolve_plain, resolve_standard};
use crate::resolve::{ScalarTag, Version};
use crate::tag::CurrentTag;

/// The default for [`Deserializer::alias_limit`].
const DEFAULT_ALIAS_LIMIT: usize = 1_000_000;

/// Deserializes YAML.
///
/// A YAML stream can contain multiple documents.  Every call to
/// [`deserialize`](Self::deserialize) reads the next document:
///
/// ```
/// use deser_yaml::Deserializer;
///
/// let mut de = Deserializer::new("--- 1\n--- two\n");
/// assert_eq!(de.deserialize::<u32>().unwrap(), 1);
/// assert_eq!(de.deserialize::<String>().unwrap(), "two");
/// assert!(de.is_end());
/// ```
///
/// # Aliases
///
/// Aliases (`*name`) are expanded: the events of the anchored node are
/// replayed.  To protect against inputs that expand exponentially (the
/// "billion laughs" attack) the number of events produced by aliases is
/// limited, see [`alias_limit`](Self::alias_limit).
pub struct Deserializer<'a> {
    #[cfg_attr(not(feature = "locations"), allow(dead_code))]
    input: &'a str,
    parser: Parser<'a>,
    peeked: Option<YamlEvent<'a>>,
    started: bool,
    /// An error that is reported by the next call.
    pending_error: Option<Error>,
    /// The parser failed, no further documents can be read.
    failed: bool,
    version: Version,
    max_depth: Option<usize>,
    alias_limit: usize,
    #[cfg(feature = "locations")]
    track_locations: bool,
    doc: Document<'a>,
}

/// A node event as it is recorded for anchors.
#[derive(Clone)]
enum Node<'a> {
    Scalar {
        tag: Option<Cow<'a, str>>,
        style: ScalarStyle,
        value: Cow<'a, str>,
        start: Mark,
        end: usize,
    },
    Start {
        tag: Option<Cow<'a, str>>,
        is_map: bool,
        start: Mark,
        end: usize,
    },
    End {
        is_map: bool,
        start: Mark,
        end: usize,
    },
    /// An alias, resolved to the range of the anchored node in the
    /// recorded nodes.
    Alias { range: (usize, usize) },
}

/// The per document state.
#[derive(Default)]
struct Document<'a> {
    version: Version,
    /// The recorded nodes.  Nodes are only recorded while an anchored
    /// collection is open.  Nested anchors share the recording.
    nodes: Vec<Node<'a>>,
    /// The anchors and the range of their nodes in `nodes`.
    anchors: HashMap<Cow<'a, str>, (usize, usize)>,
    /// The anchored collections that are currently recorded: the name,
    /// the start of the recording and the depth of the collection.
    open: Vec<(Cow<'a, str>, usize, usize)>,
    /// The depth of the collections from the parser (without aliases).
    depth: usize,
}

impl<'a> Document<'a> {
    fn reset(&mut self, version: Version) {
        self.version = version;
        self.nodes.clear();
        self.anchors.clear();
        self.open.clear();
        self.depth = 0;
    }

    /// Turns a parser event into a node and records anchors.
    fn record(&mut self, event: YamlEvent<'a>) -> Result<Node<'a>, Error> {
        let start = event.start;
        let end = event.end.offset;
        let is_map = matches!(event.kind, EventKind::MappingStart { .. });
        let (node, anchor) = match event.kind {
            EventKind::Scalar {
                props,
                style,
                value,
            } => (
                Node::Scalar {
                    tag: props.tag,
                    style,
                    value,
                    start,
                    end,
                },
                props.anchor,
            ),
            EventKind::SequenceStart { props, .. } | EventKind::MappingStart { props, .. } => {
                self.depth += 1;
                (
                    Node::Start {
                        tag: props.tag,
                        is_map,
                        start,
                        end,
                    },
                    props.anchor,
                )
            }
            EventKind::SequenceEnd | EventKind::MappingEnd => {
                let is_map = event.kind == EventKind::MappingEnd;
                self.depth -= 1;
                let node = Node::End { is_map, start, end };
                if !self.open.is_empty() {
                    self.nodes.push(node.clone());
                    let depth = self.depth + 1;
                    if let Some((name, start, _)) = self.open.pop_if(|x| x.2 == depth) {
                        self.anchors.insert(name, (start, self.nodes.len()));
                    }
                }
                return Ok(node);
            }
            EventKind::Alias { anchor } => {
                if self.open.iter().any(|x| x.0 == anchor) {
                    return Err(error_at(start, "recursive alias"));
                }
                let range = match self.anchors.get(&anchor) {
                    Some(&range) => range,
                    None => return Err(error_at(start, &format!("unknown anchor '{}'", anchor))),
                };
                let node = Node::Alias { range };
                if !self.open.is_empty() {
                    self.nodes.push(node.clone());
                }
                return Ok(node);
            }
            _ => unreachable!("unexpected event in document"),
        };

        match anchor {
            Some(name) => {
                let index = self.nodes.len();
                self.nodes.push(node.clone());
                if let Node::Start { .. } = node {
                    self.open.push((name, index, self.depth));
                } else {
                    self.anchors.insert(name, (index, index + 1));
                }
            }
            None if !self.open.is_empty() => self.nodes.push(node.clone()),
            None => {}
        }
        Ok(node)
    }
}

impl<'a> Deserializer<'a> {
    /// Creates a new deserializer.
    pub fn new(input: &'a str) -> Deserializer<'a> {
        Deserializer {
            input,
            parser: Parser::new(input),
            peeked: None,
            started: false,
            pending_error: None,
            failed: false,
            version: Version::default(),
            max_depth: None,
            alias_limit: DEFAULT_ALIAS_LIMIT,
            #[cfg(feature = "locations")]
            track_locations: false,
            doc: Document::default(),
        }
    }

    /// Creates a new deserializer for a byte slice.
    ///
    /// The input must be UTF-8, otherwise deserializing fails.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        match str_from_utf8(input) {
            Ok(input) => Deserializer::new(input),
            Err(err) => {
                let mut rv = Deserializer::new("");
                rv.pending_error = Some(err);
                rv.failed = true;
                rv
            }
        }
    }

    /// Sets the YAML version for documents that do not declare one.
    ///
    /// The version determines how plain scalars are resolved.  Documents
    /// that start with a `%YAML` directive use the version they declare.  The
    /// default is [`Version::V1_2`].
    ///
    /// ```
    /// use deser_yaml::{Deserializer, Version};
    ///
    /// let mut de = Deserializer::new("[yes, 0777]").version(Version::V1_1);
    /// let (flag, mode): (bool, u32) = de.deserialize().unwrap();
    /// assert_eq!((flag, mode), (true, 0o777));
    /// ```
    pub fn version(mut self, version: Version) -> Deserializer<'a> {
        self.version = version;
        self
    }

    /// Limits the nesting depth of sequences and mappings.
    ///
    /// Deser does not use the stack to process nested data so arbitrarily
    /// deep structures do not overflow the stack.  Still it can be useful to
    /// limit the depth of untrusted inputs.  By default the depth is not
    /// limited.
    pub fn max_depth(mut self, depth: Option<usize>) -> Deserializer<'a> {
        self.max_depth = depth;
        self
    }

    /// Limits the number of events that aliases can expand to per document.
    ///
    /// Every scalar and every start and end of a collection that is replayed
    /// for an alias counts as one event.  The default is 1,000,000.
    pub fn alias_limit(mut self, limit: usize) -> Deserializer<'a> {
        self.alias_limit = limit;
        self
    }

    /// Enables or disables location tracking.
    ///
    /// When enabled the byte offsets of every event and a source map are
    /// published into the deserializer state as
    /// [`Locations`](deser_location::Locations).  Types like
    /// [`Spanned`](deser_location::Spanned) can then pick them up.  Values
    /// produced by aliases report the location of the anchored node.
    #[cfg(feature = "locations")]
    pub fn track_locations(mut self, yes: bool) -> Deserializer<'a> {
        self.track_locations = yes;
        self
    }

    /// Returns `true` if there are no more documents.
    ///
    /// If the input is malformed this returns `false` and the next call to
    /// [`deserialize`](Self::deserialize) reports the error.
    pub fn is_end(&mut self) -> bool {
        if self.failed {
            return self.pending_error.is_none();
        }
        match self.peek_event() {
            Ok(event) => event.kind == EventKind::StreamEnd,
            Err(err) => {
                self.pending_error = Some(err);
                false
            }
        }
    }

    /// Fails if there are more documents.
    pub fn end(&mut self) -> Result<(), Error> {
        if self.is_end() {
            Ok(())
        } else if let Some(err) = self.pending_error.take() {
            Err(err)
        } else {
            Err(Error::new(
                ErrorKind::Unexpected,
                "expected a single document, found more",
            ))
        }
    }

    /// Deserializes the next document.
    ///
    /// Fails with [`ErrorKind::EndOfFile`] if there are no more documents.
    /// If a document fails to deserialize (for instance because it does not
    /// match the type), the rest of the document is skipped and the next
    /// call continues with the next document.  Syntax errors end the stream.
    pub fn deserialize<T: Deserialize>(&mut self) -> Result<T, Error> {
        let mut out = None;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            self.drive(&mut driver)?;
        }
        out.take()
            .ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty document"))
    }

    /// Returns an iterator over the remaining documents.
    ///
    /// The iterator stops after the first error.
    ///
    /// ```
    /// let mut de = deser_yaml::Deserializer::new("--- 1\n--- 2\n--- 3\n");
    /// let docs = de.iter::<u32>().collect::<Result<Vec<_>, _>>().unwrap();
    /// assert_eq!(docs, [1, 2, 3]);
    /// ```
    pub fn iter<T: Deserialize>(&mut self) -> Iter<'_, 'a, T> {
        Iter {
            de: self,
            failed: false,
            _marker: PhantomData,
        }
    }

    /// Parses the next document and feeds the events into the given driver.
    ///
    /// This is useful to deserialize into a custom
    /// [`Sink`](deser::de::Sink) or to wrap the sink of a value.
    pub fn drive(&mut self, driver: &mut DeserializeDriver) -> Result<(), Error> {
        if let Some(err) = self.pending_error.take() {
            return Err(err);
        }
        if self.failed {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "cannot continue after a syntax error",
            ));
        }

        let event = self.next_event()?;
        let version = match event.kind {
            EventKind::DocumentStart { version, .. } => match version {
                Some((1, minor)) if minor < 2 => Version::V1_1,
                Some(_) => Version::V1_2,
                None => self.version,
            },
            EventKind::StreamEnd => {
                // stay at the end of the stream
                self.peeked = Some(event);
                return Err(Error::new(ErrorKind::EndOfFile, "no more documents"));
            }
            _ => unreachable!("expected document start"),
        };
        self.doc.reset(version);

        #[cfg(feature = "locations")]
        let rv = if self.track_locations {
            deser_location::Locations::set_source_map(
                driver.state_mut(),
                std::sync::Arc::new(deser_location::SourceMap::new(self.input)),
            );
            self.drive_document::<true>(driver)
        } else {
            self.drive_document::<false>(driver)
        };
        #[cfg(not(feature = "locations"))]
        let rv = self.drive_document::<false>(driver);

        if rv.is_err() && !self.failed {
            // skip the rest of the document so that the next one can be read
            if let Err(err) = self.skip_document() {
                self.pending_error = Some(err);
            }
        }
        rv
    }

    fn ensure_started(&mut self) -> Result<(), Error> {
        if !self.started {
            self.started = true;
            let event = self.parser_event()?;
            debug_assert_eq!(event.kind, EventKind::StreamStart);
        }
        Ok(())
    }

    fn parser_event(&mut self) -> Result<YamlEvent<'a>, Error> {
        self.parser.next_event().inspect_err(|_| {
            self.failed = true;
        })
    }

    fn next_event(&mut self) -> Result<YamlEvent<'a>, Error> {
        self.ensure_started()?;
        match self.peeked.take() {
            Some(event) => Ok(event),
            None => self.parser_event(),
        }
    }

    fn peek_event(&mut self) -> Result<&YamlEvent<'a>, Error> {
        if self.peeked.is_none() {
            let event = self.next_event()?;
            self.peeked = Some(event);
        }
        Ok(self.peeked.as_ref().unwrap())
    }

    fn skip_document(&mut self) -> Result<(), Error> {
        loop {
            if let EventKind::DocumentEnd { .. } = self.next_event()?.kind {
                return Ok(());
            }
        }
    }

    fn drive_document<const LOCATIONS: bool>(
        &mut self,
        driver: &mut DeserializeDriver,
    ) -> Result<(), Error> {
        // the ranges of recorded nodes that are replayed for aliases
        let mut replay: Vec<(usize, usize)> = Vec::new();
        let mut replayed = 0;
        let mut depth = 0;

        loop {
            let node = if let Some(frame) = replay.last_mut() {
                if frame.0 == frame.1 {
                    replay.pop();
                    continue;
                }
                let node = self.doc.nodes[frame.0].clone();
                frame.0 += 1;
                replayed += 1;
                if replayed > self.alias_limit {
                    return Err(Error::new(
                        ErrorKind::Unexpected,
                        "aliases expand to too many events",
                    ));
                }
                node
            } else {
                let event = self.next_event()?;
                if let EventKind::DocumentEnd { .. } = event.kind {
                    return Ok(());
                }
                self.doc.record(event)?
            };

            match node {
                Node::Scalar {
                    tag,
                    style,
                    value,
                    start,
                    end,
                } => {
                    let version = self.doc.version;
                    emit_scalar::<LOCATIONS>(driver, tag, style, value, version, start, end)?;
                }
                Node::Start {
                    tag,
                    is_map,
                    start,
                    end,
                } => {
                    if self.max_depth.is_some_and(|max| depth >= max) {
                        return Err(error_at(start, "recursion limit exceeded"));
                    }
                    depth += 1;
                    let event = if is_map {
                        Event::MapStart
                    } else {
                        Event::SeqStart
                    };
                    publish_location::<LOCATIONS>(driver, start.offset, end);
                    match tag {
                        Some(tag) => match is_collection_tag(&tag, is_map) {
                            Ok(true) => driver.emit(event)?,
                            Ok(false) => emit_tagged(driver, &tag, event)?,
                            Err(msg) => return Err(error_at(start, msg)),
                        },
                        None => driver.emit(event)?,
                    }
                }
                Node::End { is_map, start, end } => {
                    depth -= 1;
                    publish_location::<LOCATIONS>(driver, start.offset, end);
                    driver.emit(if is_map { Event::MapEnd } else { Event::SeqEnd })?;
                }
                Node::Alias { range } => replay.push(range),
            }
        }
    }
}

#[inline(always)]
fn publish_location<const LOCATIONS: bool>(
    driver: &mut DeserializeDriver,
    start: usize,
    end: usize,
) {
    #[cfg(feature = "locations")]
    if LOCATIONS {
        deser_location::Locations::set_current(driver.state_mut(), start, end);
    }
    #[cfg(not(feature = "locations"))]
    let _ = (driver, start, end);
}

#[inline]
fn emit_scalar<const LOCATIONS: bool>(
    driver: &mut DeserializeDriver,
    tag: Option<Cow<'_, str>>,
    style: ScalarStyle,
    value: Cow<'_, str>,
    version: Version,
    start: Mark,
    end: usize,
) -> Result<(), Error> {
    publish_location::<LOCATIONS>(driver, start.offset, end);
    let tag = match tag {
        None if style == ScalarStyle::Plain => {
            return driver.emit(resolve_plain(value, version));
        }
        None => return driver.emit(Atom::Str(value)),
        Some(tag) => tag,
    };
    match classify_tag(&tag) {
        ScalarTag::Str => driver.emit(Atom::Str(value)),
        ScalarTag::Standard(name) => match resolve_standard(name, value, version) {
            Ok(atom) => driver.emit(atom),
            Err(msg) => Err(error_at(start, msg)),
        },
        ScalarTag::Custom => emit_tagged(driver, &tag, Atom::Str(value)),
    }
}

/// Emits an event with a tag in the state.
#[cold]
fn emit_tagged<'e, E: Into<Event<'e>>>(
    driver: &mut DeserializeDriver,
    tag: &str,
    event: E,
) -> Result<(), Error> {
    let state = driver.state_mut();
    state.set_replayable::<CurrentTag>();
    state.get_mut::<CurrentTag>().0 = Some(tag.to_string());
    let rv = driver.emit(event);
    driver.state_mut().get_mut::<CurrentTag>().0 = None;
    rv
}

fn str_from_utf8(bytes: &[u8]) -> Result<&str, Error> {
    #[cfg(feature = "simdutf8")]
    {
        if simdutf8::basic::from_utf8(bytes).is_ok() {
            // SAFETY: validated above
            return Ok(unsafe { std::str::from_utf8_unchecked(bytes) });
        }
    }
    std::str::from_utf8(bytes).map_err(|err| {
        Error::new(
            ErrorKind::Unexpected,
            format!("invalid UTF-8 at offset {}", err.valid_up_to()),
        )
    })
}

/// An iterator over the documents of a YAML stream.
///
/// See [`Deserializer::iter`].
pub struct Iter<'de, 'a, T> {
    de: &'de mut Deserializer<'a>,
    failed: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<'de, 'a, T: Deserialize> Iterator for Iter<'de, 'a, T> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.de.is_end() {
            return None;
        }
        let rv = self.de.deserialize();
        self.failed = rv.is_err();
        Some(rv)
    }
}

/// Deserializes a value from YAML.
///
/// The input must contain at most one document.  An empty stream (no
/// document at all, for instance an empty file) is deserialized as null.
pub fn from_str<T: Deserialize>(s: &str) -> Result<T, Error> {
    from_deserializer(Deserializer::new(s))
}

/// Deserializes a value from YAML in a byte slice.
///
/// The input must be UTF-8.  Otherwise this works like [`from_str`].
pub fn from_slice<T: Deserialize>(bytes: &[u8]) -> Result<T, Error> {
    from_deserializer(Deserializer::from_slice(bytes))
}

fn from_deserializer<T: Deserialize>(mut de: Deserializer) -> Result<T, Error> {
    if de.is_end() {
        let mut out = None;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            driver.emit(Atom::Null)?;
        }
        return out.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty document"));
    }
    let rv = de.deserialize()?;
    de.end()?;
    Ok(rv)
}
