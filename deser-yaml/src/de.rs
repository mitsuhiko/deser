use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::Arc;

use deser::adapters::bytes::BytesFormat;
use deser::de::{self, Deserialize, DeserializeDriver, Limits};
use deser::hints::Layout;
use deser::{Atom, Error, ErrorKind, Event};

use crate::event::{Event as YamlEvent, EventKind, Mark, ScalarStyle};
use crate::parser::{Parser, error_at};
use crate::resolve::{ScalarTag, Version};
use crate::resolve::{classify_tag, is_collection_tag, resolve_plain, resolve_standard};
use crate::tag::NodeTag;

/// The default for [`DeserializerConfig::alias_limit`].
const DEFAULT_ALIAS_LIMIT: usize = 1_000_000;

/// Configures how YAML is deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The methods
/// [`from_str`](Self::from_str) and [`from_slice`](Self::from_slice) work
/// like the functions of the same name.  To read multiple documents with
/// the configuration create a [`Deserializer`] with
/// [`Deserializer::from_str_with_config`] or
/// [`Deserializer::from_slice_with_config`].
///
/// ```
/// use deser_yaml::{DeserializerConfig, Version};
///
/// const CONFIG: DeserializerConfig = DeserializerConfig::new()
///     .version(Version::V1_1)
///     .max_depth(64);
/// let (flag, mode): (bool, u32) = CONFIG.from_str("[yes, 0777]").unwrap();
/// assert_eq!((flag, mode), (true, 0o777));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializerConfig {
    version: Version,
    max_depth: Option<usize>,
    alias_limit: usize,
    merge_keys: bool,
    track_locations: bool,
    bytes: BytesFormat,
}

impl Default for DeserializerConfig {
    fn default() -> DeserializerConfig {
        DeserializerConfig::new()
    }
}

impl DeserializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> DeserializerConfig {
        DeserializerConfig {
            version: Version::V1_2,
            max_depth: None,
            alias_limit: DEFAULT_ALIAS_LIMIT,
            merge_keys: true,
            track_locations: false,
            bytes: BytesFormat::BASE64,
        }
    }

    /// Sets the YAML version for documents that do not declare one.
    ///
    /// The version determines how plain scalars are resolved.  Documents
    /// that start with a `%YAML` directive use the version they declare.  The
    /// default is [`Version::V1_2`].
    pub const fn version(mut self, version: Version) -> DeserializerConfig {
        self.version = version;
        self
    }

    /// Limits the nesting depth of sequences and mappings.
    ///
    /// Deser does not use the stack to process nested data so arbitrarily
    /// deep structures do not overflow the stack.  Still it can be useful to
    /// limit the depth of untrusted inputs.  By default the depth is not
    /// limited.  This adds a [`Limits`] layer to the driver, which can also
    /// limit other aspects of the input.
    pub const fn max_depth(mut self, depth: usize) -> DeserializerConfig {
        self.max_depth = Some(depth);
        self
    }

    /// Limits the number of events that aliases can expand to per document.
    ///
    /// Every scalar and every start and end of a collection that is replayed
    /// for an alias counts as one event.  The default is 1,000,000.
    pub const fn alias_limit(mut self, limit: usize) -> DeserializerConfig {
        self.alias_limit = limit;
        self
    }

    /// Enables or disables merge keys.
    ///
    /// Merge keys (`<<`) insert the entries of other mappings into a
    /// mapping.  They are defined for YAML 1.1 but widely used with all
    /// versions of YAML, which is why they are enabled by default:
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use deser::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct Service {
    ///     image: String,
    ///     replicas: u32,
    /// }
    ///
    /// let input = "
    /// base: &base {image: app, replicas: 1}
    /// web:
    ///   <<: *base
    ///   replicas: 3
    /// ";
    /// let value: BTreeMap<String, Service> = deser_yaml::from_str(input).unwrap();
    /// assert_eq!(value["web"].image, "app");
    /// assert_eq!(value["web"].replicas, 3);
    /// ```
    ///
    /// Only a plain `<<` is a merge key (`"<<"` is a regular key).  Its value
    /// must be a mapping or a sequence of mappings.  Keys of the mapping take
    /// precedence over merged keys, and with multiple mappings the earlier
    /// ones take precedence.  The merged entries are emitted after the
    /// entries of the mapping.
    ///
    /// When disabled, `<<` is a regular key.
    pub const fn merge_keys(mut self, yes: bool) -> DeserializerConfig {
        self.merge_keys = yes;
        self
    }

    /// Enables or disables location tracking.
    ///
    /// The byte range of every event is always published into the state
    /// (see [`State::input_range`](deser::State::input_range)).  When
    /// enabled additionally the input is set as source (see
    /// [`State::source`](deser::State::source)) which allows resolving the
    /// ranges into lines and columns, for instance with the `Spanned` type
    /// of [`deser-location`](https://docs.rs/deser-location).  This copies
    /// the input.
    ///
    /// Values produced by aliases report the location of the anchored node.
    pub const fn track_locations(mut self, yes: bool) -> DeserializerConfig {
        self.track_locations = yes;
        self
    }

    /// Sets how strings are decoded into bytes.
    ///
    /// Bytes are native in YAML (`!!binary`).  Strings that types which
    /// expect bytes (like `Vec<u8>`) receive are decoded as base64 by
    /// default.  This is only needed to read bytes written with
    /// [`SerializerConfig::binary`](crate::SerializerConfig::binary) off and
    /// another format.
    ///
    /// ```
    /// use deser::adapters::bytes::{BytesFormat, Hex};
    /// use deser_yaml::DeserializerConfig;
    ///
    /// const HEX: DeserializerConfig = DeserializerConfig::new().bytes(BytesFormat::encoded::<Hex>());
    /// let bytes: Vec<u8> = HEX.from_str("01ff").unwrap();
    /// assert_eq!(bytes, [1, 255]);
    /// ```
    ///
    /// The format is placed into the state (see [`deser::adapters::bytes`]).
    /// Values that use an adapter for bytes are not affected.
    pub const fn bytes(mut self, format: BytesFormat) -> DeserializerConfig {
        self.bytes = format;
        self
    }

    /// Deserializes a value from YAML.
    ///
    /// See [`from_str`](crate::from_str).
    pub fn from_str<'de, T: Deserialize<'de>>(&self, s: &'de str) -> Result<T, Error> {
        deserialize_single(Deserializer::from_str_with_config(s, self), |_| {})
    }

    /// Deserializes a value from YAML in a byte slice.
    ///
    /// See [`from_slice`](crate::from_slice).
    pub fn from_slice<'de, T: Deserialize<'de>>(&self, bytes: &'de [u8]) -> Result<T, Error> {
        deserialize_single(Deserializer::from_slice_with_config(bytes, self), |_| {})
    }
}

/// Deserializes YAML.
///
/// A YAML stream can contain multiple documents.  Every call to
/// [`deserialize`](Self::deserialize) reads the next document:
///
/// ```
/// use deser_yaml::Deserializer;
///
/// let mut de = Deserializer::from_str("--- 1\n--- two\n");
/// assert_eq!(de.deserialize::<u32>().unwrap(), 1);
/// assert_eq!(de.deserialize::<String>().unwrap(), "two");
/// assert!(de.is_end());
/// ```
///
/// To deserialize a single document, use [`from_str`](crate::from_str) and
/// [`from_slice`](crate::from_slice) (or the methods of the same name on
/// [`DeserializerConfig`]).
///
/// # Aliases
///
/// Aliases (`*name`) are expanded: the events of the anchored node are
/// replayed.  To protect against inputs that expand exponentially (the
/// "billion laughs" attack) the number of events produced by aliases is
/// limited, see [`DeserializerConfig::alias_limit`].
pub struct Deserializer<'a> {
    input: &'a str,
    parser: Parser<'a>,
    peeked: Option<YamlEvent<'a>>,
    started: bool,
    /// An error that is reported by the next call.
    pending_error: Option<Error>,
    /// The parser failed, no further documents can be read.
    failed: bool,
    config: DeserializerConfig,
    /// Merge keys are enabled and the input contains `<<`.  Otherwise
    /// there cannot be merge keys.
    track_merges: bool,
    /// The input as source for location tracking, shared by all documents.
    source: Option<Arc<str>>,
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
        /// `true` for flow collections (`[a]` and `{a: b}`).
        flow: bool,
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
    /// The collections that are currently recorded.
    open: Vec<OpenNode<'a>>,
    /// The range of the last collection that was captured (recorded
    /// without anchor, for merge keys).
    captured: Option<(usize, usize)>,
    /// The depth of the collections from the parser (without aliases).
    depth: usize,
}

/// A collection that is being recorded.
struct OpenNode<'a> {
    anchor: Option<Cow<'a, str>>,
    /// The index of the start node in the recording.
    start: usize,
    /// The depth of the collection.
    depth: usize,
    /// The range should be stored in `captured` when the collection ends.
    capture: bool,
}

impl<'a> Document<'a> {
    fn reset(&mut self, version: Version) {
        self.version = version;
        self.nodes.clear();
        self.anchors.clear();
        self.open.clear();
        self.captured = None;
        self.depth = 0;
    }

    /// Turns a parser event into a node and records anchors.
    ///
    /// If `capture` is set, a collection is recorded even without anchor
    /// and its range is stored in `captured` once it ends.
    fn record(&mut self, event: YamlEvent<'a>, capture: bool) -> Result<Node<'a>, Error> {
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
            EventKind::SequenceStart { props, flow } | EventKind::MappingStart { props, flow } => {
                self.depth += 1;
                (
                    Node::Start {
                        tag: props.tag,
                        is_map,
                        flow,
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
                    if let Some(open) = self.open.pop_if(|x| x.depth == depth) {
                        let range = (open.start, self.nodes.len());
                        if let Some(name) = open.anchor {
                            self.anchors.insert(name, range);
                        }
                        if open.capture {
                            self.captured = Some(range);
                        }
                    }
                }
                return Ok(node);
            }
            EventKind::Alias { anchor } => {
                if self.open.iter().any(|x| x.anchor.as_ref() == Some(&anchor)) {
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

        let is_start = matches!(node, Node::Start { .. });
        if anchor.is_some() || (capture && is_start) || !self.open.is_empty() {
            let index = self.nodes.len();
            self.nodes.push(node.clone());
            if is_start && (anchor.is_some() || capture) {
                self.open.push(OpenNode {
                    anchor,
                    start: index,
                    depth: self.depth,
                    capture,
                });
            } else if let Some(name) = anchor {
                self.anchors.insert(name, (index, index + 1));
            }
        }
        Ok(node)
    }

    /// Returns the index after the recorded node that starts at `index`.
    fn node_end(&self, index: usize) -> usize {
        if !matches!(self.nodes[index], Node::Start { .. }) {
            return index + 1;
        }
        let mut depth = 0;
        for (offset, node) in self.nodes[index..].iter().enumerate() {
            match node {
                Node::Start { .. } => depth += 1,
                Node::End { .. } => {
                    depth -= 1;
                    if depth == 0 {
                        return index + offset + 1;
                    }
                }
                _ => {}
            }
        }
        unreachable!("unbalanced recording");
    }

    /// Follows a recorded alias to the range of the node it refers to.
    fn resolve_range(&self, mut range: (usize, usize)) -> (usize, usize) {
        while let Node::Alias { range: target } = self.nodes[range.0] {
            range = target;
        }
        range
    }

    /// Returns the ranges of the items of a recorded collection.
    fn items(&self, range: (usize, usize)) -> impl Iterator<Item = (usize, usize)> + '_ {
        let mut index = range.0 + 1;
        std::iter::from_fn(move || {
            if index + 1 >= range.1 {
                return None;
            }
            let item = (index, self.node_end(index));
            index = item.1;
            Some(item)
        })
    }
}

/// The identity of a map key for the purpose of merge keys.
///
/// Only scalar keys are compared.  Two keys are the same if they resolve to
/// the same value (so `1` and `0x1` are the same key).
#[derive(PartialEq, Eq, Hash)]
enum KeyId<'a> {
    Null,
    Bool(bool),
    /// An integer as sign and magnitude.
    Int(bool, u128),
    Float(u64),
    Str(Cow<'a, str>),
    Bytes(Vec<u8>),
    Tagged(Cow<'a, str>, Cow<'a, str>),
}

impl<'a> KeyId<'a> {
    fn of_scalar(
        tag: &Option<Cow<'a, str>>,
        style: ScalarStyle,
        value: &Cow<'a, str>,
        version: Version,
    ) -> Option<KeyId<'a>> {
        let atom = match tag {
            None if style == ScalarStyle::Plain => resolve_plain(value.clone(), version),
            None => return Some(KeyId::Str(value.clone())),
            Some(tag) => match classify_tag(tag) {
                ScalarTag::Str => return Some(KeyId::Str(value.clone())),
                ScalarTag::Standard(name) => resolve_standard(name, value.clone(), version).ok()?,
                ScalarTag::Custom => return Some(KeyId::Tagged(tag.clone(), value.clone())),
            },
        };
        Some(match atom {
            Atom::Null => KeyId::Null,
            Atom::Bool(value) => KeyId::Bool(value),
            Atom::U64(value) => KeyId::Int(false, value.into()),
            Atom::I64(value) => KeyId::Int(value < 0, value.unsigned_abs().into()),
            Atom::F64(value) => KeyId::Float(value.to_bits()),
            Atom::Str(value) => KeyId::Str(value),
            Atom::Bytes(value) => KeyId::Bytes(value.into_owned()),
            Atom::Ext(ref ext) => {
                if let Some(&value) = ext.downcast_ref::<u128>() {
                    KeyId::Int(false, value)
                } else if let Some(&value) = ext.downcast_ref::<i128>() {
                    KeyId::Int(value < 0, value.unsigned_abs())
                } else {
                    return None;
                }
            }
            _ => return None,
        })
    }
}

fn is_merge_key(tag: &Option<Cow<'_, str>>, style: ScalarStyle, value: &str) -> bool {
    tag.is_none() && style == ScalarStyle::Plain && value == "<<"
}

/// Something that is emitted before the next event from the parser.
enum Pending<'a> {
    /// A range of recorded nodes (for aliases and merged entries).
    Range(usize, usize),
    /// A single node.
    Node(Node<'a>),
}

/// An open collection while merge keys are tracked.
struct Frame<'a> {
    is_map: bool,
    /// For maps: the next node is a key.
    expect_key: bool,
    /// The keys that were emitted so far.
    keys: HashSet<KeyId<'a>>,
    /// The ranges of the merge key values.
    sources: Vec<(usize, usize)>,
}

impl<'a> Deserializer<'a> {
    /// Creates a new deserializer for a string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(input: &'a str) -> Deserializer<'a> {
        Deserializer::from_str_with_config(input, &DeserializerConfig::new())
    }

    /// Creates a new deserializer for a string with the given configuration.
    pub fn from_str_with_config(input: &'a str, config: &DeserializerConfig) -> Deserializer<'a> {
        Deserializer {
            input,
            parser: Parser::new(input),
            peeked: None,
            started: false,
            pending_error: None,
            failed: false,
            config: config.clone(),
            track_merges: config.merge_keys && input.contains("<<"),
            source: None,
            doc: Document::default(),
        }
    }

    /// Creates a new deserializer for a byte slice.
    ///
    /// The input must be UTF-8, otherwise deserializing fails.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input, &DeserializerConfig::new())
    }

    /// Creates a new deserializer for a byte slice with the given
    /// configuration.
    ///
    /// The input must be UTF-8, otherwise deserializing fails.
    pub fn from_slice_with_config(
        input: &'a [u8],
        config: &DeserializerConfig,
    ) -> Deserializer<'a> {
        match str_from_utf8(input) {
            Ok(input) => Deserializer::from_str_with_config(input, config),
            Err(err) => {
                let mut rv = Deserializer::from_str_with_config("", config);
                rv.pending_error = Some(err);
                rv.failed = true;
                rv
            }
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
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
    ///
    /// To configure the deserialization (for instance to add layers) use
    /// [`deserialize_with`](Self::deserialize_with).
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        de::Deserializer::deserialize(self)
    }

    /// Deserializes the next value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// deserialized, for instance to add [`Layer`](deser::de::Layer)s.
    pub fn deserialize_with<T, F>(&mut self, setup: F) -> Result<T, Error>
    where
        T: Deserialize<'a>,
        F: FnOnce(&mut DeserializeDriver<'_, 'a>),
    {
        de::Deserializer::deserialize_with(self, setup)
    }

    /// Returns an iterator over the remaining documents.
    ///
    /// The iterator stops after the first error.
    ///
    /// ```
    /// let mut de = deser_yaml::Deserializer::from_str("--- 1\n--- 2\n--- 3\n");
    /// let docs = de.iter::<u32>().collect::<Result<Vec<_>, _>>().unwrap();
    /// assert_eq!(docs, [1, 2, 3]);
    /// ```
    pub fn iter<T: Deserialize<'a>>(&mut self) -> Iter<'_, 'a, T> {
        Iter {
            de: self,
            failed: false,
            _marker: PhantomData,
        }
    }

    /// Parses the next document and feeds the events into the given driver.
    ///
    /// This is useful to deserialize into a custom
    /// [`Sink`](deser::de::Sink).  See also [`deserialize_with`](Self::deserialize_with).
    /// Errors carry the location in the input (see [`Error::line`]).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
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
                None => self.config.version,
            },
            EventKind::StreamEnd => {
                // stay at the end of the stream
                self.peeked = Some(event);
                return Err(Error::new(ErrorKind::EndOfFile, "no more documents"));
            }
            _ => unreachable!("expected document start"),
        };
        self.doc.reset(version);

        if self.config.track_locations {
            let source = self.source.get_or_insert_with(|| self.input.into());
            driver.state_mut().set_source(source.clone());
        }
        if let Some(max_depth) = self.config.max_depth {
            driver.push_layer(Limits::new().max_depth(max_depth));
        }
        if self.config.bytes != BytesFormat::BASE64 {
            *driver.state_mut().get_mut::<BytesFormat>() = self.config.bytes;
        }
        let input = self.input;
        let rv = self
            .drive_document(driver)
            .map_err(|err| err.resolve_position(input.as_bytes()));

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

    fn drive_document(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        // nodes that are emitted before the next event from the parser
        let mut pending: Vec<Pending<'a>> = Vec::new();
        // the number of nodes produced by aliases and merges
        let mut replayed = 0;
        // merge keys need to know the keys of all open maps.  This is only
        // done if the input can contain merge keys.
        let track_merges = self.track_merges;
        let mut frames: Vec<Frame<'a>> = Vec::new();
        let mut merge_value_follows = false;

        loop {
            let (node, index) =
                match self.next_node(&mut pending, &mut replayed, merge_value_follows)? {
                    Some(rv) => rv,
                    None => return Ok(()),
                };

            if merge_value_follows {
                merge_value_follows = false;
                let range = self.merge_source(node, index, &mut pending)?;
                let frame = frames.last_mut().unwrap();
                frame.sources.push(range);
                frame.expect_key = true;
                continue;
            }

            if track_merges
                && let Some(frame) = frames.last_mut()
                && frame.is_map
            {
                if frame.expect_key
                    && let Node::Scalar {
                        ref tag,
                        style,
                        ref value,
                        ..
                    } = node
                {
                    if is_merge_key(tag, style, value) {
                        frame.expect_key = false;
                        merge_value_follows = true;
                        continue;
                    }
                    if let Some(id) = KeyId::of_scalar(tag, style, value, self.doc.version) {
                        frame.keys.insert(id);
                    }
                }
                // aliases are replayed, their nodes are counted
                if !matches!(node, Node::Alias { .. } | Node::End { .. }) {
                    frame.expect_key = !frame.expect_key;
                }
            }

            match node {
                Node::Scalar {
                    tag,
                    style,
                    value,
                    start,
                    end,
                } => {
                    let version = self.doc.version;
                    emit_scalar(driver, tag, style, value, version, start, end)?;
                }
                Node::Start {
                    tag,
                    is_map,
                    flow,
                    start,
                    end,
                } => {
                    // flow collections are compact so that they stay flow
                    // collections when they are serialized again
                    if flow {
                        Layout::Compact.set(driver.state_mut());
                    }
                    if track_merges {
                        frames.push(Frame {
                            is_map,
                            expect_key: true,
                            keys: HashSet::new(),
                            sources: Vec::new(),
                        });
                    }
                    let event = if is_map {
                        Event::map_start()
                    } else {
                        Event::seq_start()
                    };
                    driver.state_mut().set_input_range(start.offset, end);
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
                    if track_merges {
                        let frame = frames.last_mut().unwrap();
                        if !frame.sources.is_empty() {
                            // emit the merged entries before the end of the
                            // map, the end is emitted again afterwards
                            let sources = std::mem::take(&mut frame.sources);
                            let entries =
                                self.merged_entries(&sources, &mut frame.keys, &mut replayed)?;
                            pending.push(Pending::Node(Node::End { is_map, start, end }));
                            for (key, value) in entries.into_iter().rev() {
                                pending.push(Pending::Range(value.0, value.1));
                                pending.push(Pending::Range(key.0, key.1));
                            }
                            continue;
                        }
                        frames.pop();
                    }
                    let event = if is_map { Event::MapEnd } else { Event::SeqEnd };
                    driver.state_mut().set_input_range(start.offset, end);
                    driver.emit(event)?;
                }
                Node::Alias { range } => pending.push(Pending::Range(range.0, range.1)),
            }
        }
    }

    /// Returns the next node and its index if it was recorded.  Returns
    /// `None` at the end of the document.
    fn next_node(
        &mut self,
        pending: &mut Vec<Pending<'a>>,
        replayed: &mut usize,
        capture: bool,
    ) -> Result<Option<(Node<'a>, Option<usize>)>, Error> {
        loop {
            let (node, index) = match pending.last_mut() {
                Some(Pending::Range(start, end)) => {
                    if start == end {
                        pending.pop();
                        continue;
                    }
                    let index = *start;
                    *start += 1;
                    (self.doc.nodes[index].clone(), Some(index))
                }
                Some(Pending::Node(_)) => match pending.pop() {
                    Some(Pending::Node(node)) => return Ok(Some((node, None))),
                    _ => unreachable!(),
                },
                None => {
                    let event = self.next_event()?;
                    if let EventKind::DocumentEnd { .. } = event.kind {
                        return Ok(None);
                    }
                    return Ok(Some((self.doc.record(event, capture)?, None)));
                }
            };
            self.count_replayed(replayed, 1)?;
            return Ok(Some((node, index)));
        }
    }

    fn count_replayed(&self, replayed: &mut usize, count: usize) -> Result<(), Error> {
        *replayed += count;
        if *replayed > self.config.alias_limit {
            Err(Error::new(
                ErrorKind::Unexpected,
                "aliases expand to too many events",
            ))
        } else {
            Ok(())
        }
    }

    /// Consumes the value of a merge key and returns the range of its
    /// recorded nodes.
    fn merge_source(
        &mut self,
        node: Node<'a>,
        index: Option<usize>,
        pending: &mut [Pending<'a>],
    ) -> Result<(usize, usize), Error> {
        match node {
            Node::Alias { range } => Ok(range),
            Node::Start { .. } => match index {
                // the value is replayed, skip it
                Some(index) => {
                    let end = self.doc.node_end(index);
                    if let Some(Pending::Range(start, _)) = pending.last_mut() {
                        *start = end;
                    }
                    Ok((index, end))
                }
                // the value comes from the parser, record it without
                // emitting it
                None => {
                    while self.doc.captured.is_none() {
                        let event = self.next_event()?;
                        self.doc.record(event, false)?;
                    }
                    Ok(self.doc.captured.take().unwrap())
                }
            },
            Node::Scalar { start, .. } | Node::End { start, .. } => Err(error_at(
                start,
                "the value of a merge key must be a mapping or a sequence of mappings",
            )),
        }
    }

    /// Returns the entries that the merge keys of a map add.
    ///
    /// `keys` holds the keys the map already has, the merged keys are added.
    /// Earlier sources take precedence over later ones, the merge keys of the
    /// sources are applied recursively.
    #[allow(clippy::type_complexity)]
    fn merged_entries(
        &self,
        sources: &[(usize, usize)],
        keys: &mut HashSet<KeyId<'a>>,
        replayed: &mut usize,
    ) -> Result<Vec<((usize, usize), (usize, usize))>, Error> {
        let mut entries = Vec::new();
        // the mappings to merge in reverse order
        let mut stack = Vec::new();
        self.push_merge_sources(sources, &mut stack);

        while let Some(range) = stack.pop() {
            match self.doc.nodes[range.0] {
                Node::Start { is_map: true, .. } => {}
                Node::Start { start, .. } | Node::Scalar { start, .. } => {
                    return Err(error_at(
                        start,
                        "merge keys can only merge mappings or sequences of mappings",
                    ));
                }
                _ => unreachable!(),
            }
            self.count_replayed(replayed, range.1 - range.0)?;

            let mut nested = Vec::new();
            let mut items = self.doc.items(range);
            while let (Some(key), Some(value)) = (items.next(), items.next()) {
                if let Node::Scalar {
                    tag: ref key_tag,
                    style,
                    value: ref key_value,
                    ..
                } = self.doc.nodes[self.doc.resolve_range(key).0]
                {
                    if is_merge_key(key_tag, style, key_value) {
                        nested.push(value);
                        continue;
                    }
                    let version = self.doc.version;
                    if let Some(id) = KeyId::of_scalar(key_tag, style, key_value, version)
                        && !keys.insert(id)
                    {
                        // the map (or an earlier source) has the key
                        continue;
                    }
                }
                entries.push((key, value));
            }
            self.push_merge_sources(&nested, &mut stack);
        }
        Ok(entries)
    }

    /// Adds the mappings of merge key values to the stack (in reverse order,
    /// so that the first mapping is processed first).
    fn push_merge_sources(&self, sources: &[(usize, usize)], stack: &mut Vec<(usize, usize)>) {
        for &source in sources.iter().rev() {
            let source = self.doc.resolve_range(source);
            match self.doc.nodes[source.0] {
                Node::Start { is_map: false, .. } => {
                    let items: Vec<_> = self
                        .doc
                        .items(source)
                        .map(|item| self.doc.resolve_range(item))
                        .collect();
                    stack.extend(items.into_iter().rev());
                }
                _ => stack.push(source),
            }
        }
    }
}

#[inline]
///
/// Scalars are emitted borrowed as they are slices of the input unless they
/// had to be unescaped or folded.
fn emit_scalar<'a>(
    driver: &mut DeserializeDriver<'_, 'a>,
    tag: Option<Cow<'_, str>>,
    style: ScalarStyle,
    value: Cow<'a, str>,
    version: Version,
    start: Mark,
    end: usize,
) -> Result<(), Error> {
    driver.state_mut().set_input_range(start.offset, end);
    let tag = match tag {
        None if style == ScalarStyle::Plain => {
            return driver.emit_borrowed(resolve_plain(value, version));
        }
        None => return driver.emit_borrowed(Atom::Str(value)),
        Some(tag) => tag,
    };
    match classify_tag(&tag) {
        ScalarTag::Str => driver.emit_borrowed(Atom::Str(value)),
        ScalarTag::Standard(name) => match resolve_standard(name, value, version) {
            Ok(atom) => driver.emit_borrowed(atom),
            Err(msg) => Err(error_at(start, msg)),
        },
        ScalarTag::Custom => emit_tagged(driver, &tag, Atom::Str(value)),
    }
}

/// Emits an event with a tag attached to it.
#[cold]
fn emit_tagged<'a, E: Into<Event<'a>>>(
    driver: &mut DeserializeDriver<'_, 'a>,
    tag: &str,
    event: E,
) -> Result<(), Error> {
    driver.state_mut().event_mut::<NodeTag>().0 = Some(tag.to_string());
    driver.emit_borrowed(event)
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
        Error::new(ErrorKind::Unexpected, "invalid UTF-8").with_offset(err.valid_up_to())
    })
}

/// An iterator over the documents of a YAML stream.
///
/// See [`Deserializer::iter`].
pub struct Iter<'b, 'a, T> {
    de: &'b mut Deserializer<'a>,
    failed: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<'b, 'a, T: Deserialize<'a>> Iterator for Iter<'b, 'a, T> {
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

impl<'a> de::Deserializer<'a> for Deserializer<'a> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        Deserializer::drive(self, driver)
    }
}

/// Deserializes a value from YAML.
///
/// The input must contain at most one document.  An empty stream (no
/// document at all, for instance an empty file) is deserialized as null.
/// This uses the default [`DeserializerConfig`].
pub fn from_str<'de, T: Deserialize<'de>>(s: &'de str) -> Result<T, Error> {
    deserialize_single(Deserializer::from_str(s), |_| {})
}

/// Deserializes a value from YAML in a byte slice.
///
/// The input must be UTF-8.  Otherwise this works like [`from_str`].  This
/// uses the default [`DeserializerConfig`].
pub fn from_slice<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    deserialize_single(Deserializer::from_slice(bytes), |_| {})
}

/// Deserializes the only document (or null for an empty stream).
pub(crate) fn deserialize_single<'de, T, F>(mut de: Deserializer<'de>, setup: F) -> Result<T, Error>
where
    T: Deserialize<'de>,
    F: FnOnce(&mut DeserializeDriver<'_, 'de>),
{
    if de.is_end() {
        let mut out = None;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            setup(&mut driver);
            driver.emit(Atom::Null)?;
        }
        return out.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty document"));
    }
    let rv = de.deserialize_with(setup)?;
    de.end()?;
    Ok(rv)
}
