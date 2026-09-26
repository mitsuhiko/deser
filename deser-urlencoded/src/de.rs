use std::borrow::Cow;
use std::collections::HashMap;

use deser::adapters::bytes::BytesFormat;
use deser::de::{self, Deserialize, DeserializeDriver, DuplicateKeys};
use deser::{Atom, Bytes, ContainerShape, Error, ErrorKind, Event};

use crate::Nesting;
use crate::encoding::{Decoded, decode};

/// Configures how query strings and form data are deserialized.
///
/// The configuration is independent of the input so it can be created once
/// (even as a constant) and used for many inputs.  The methods
/// [`from_str`](Self::from_str) and [`from_slice`](Self::from_slice) work
/// like the functions of the same name.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_urlencoded::{DeserializerConfig, Nesting};
///
/// const CONFIG: DeserializerConfig = DeserializerConfig::new().nesting(Nesting::Dots);
/// let value: BTreeMap<String, BTreeMap<String, u32>> = CONFIG.from_str("a.b=1").unwrap();
/// assert_eq!(value["a"]["b"], 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializerConfig {
    nesting: Nesting,
    max_depth: usize,
    max_params: usize,
    duplicate_keys: DuplicateKeys,
    bytes: BytesFormat,
    track_locations: bool,
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
            nesting: Nesting::Brackets,
            max_depth: 16,
            max_params: 4096,
            duplicate_keys: DuplicateKeys::Last,
            bytes: BytesFormat::BASE64,
            track_locations: false,
        }
    }

    /// Sets how nested keys are written.
    ///
    /// The default is [`Nesting::Brackets`] (`a[b][0]=1`).  With
    /// [`Nesting::Flat`] keys are taken as they are, see [`Nesting`] for
    /// more information.
    pub const fn nesting(mut self, nesting: Nesting) -> DeserializerConfig {
        self.nesting = nesting;
        self
    }

    /// Sets how deeply keys can be nested.
    ///
    /// This is the number of nested keys after the first one (`a[b][c]` has
    /// a depth of 2).  Keys that are nested deeper are an error.  The
    /// default is 16.
    pub const fn max_depth(mut self, depth: usize) -> DeserializerConfig {
        self.max_depth = depth;
        self
    }

    /// Sets the maximum number of parameters.
    ///
    /// Inputs with more parameters (`key=value` pairs) are an error.  The
    /// default is 4096.
    pub const fn max_params(mut self, max: usize) -> DeserializerConfig {
        self.max_params = max;
        self
    }

    /// Sets what happens if a key that stands for a single value is given
    /// more than once.
    ///
    /// The values of a key that is given more than once are passed on as a
    /// sequence (see [`ContainerShape::with_repeated`]).  Types that accept
    /// sequences (like `Vec<T>`) receive all of them, for other types this
    /// decides which value is used.  The default is [`DuplicateKeys::Last`]
    /// which matches what many web frameworks do (and makes the pattern of a
    /// hidden input for unchecked checkboxes work).
    ///
    /// ```
    /// use deser::de::DuplicateKeys;
    /// use deser_urlencoded::DeserializerConfig;
    ///
    /// #[derive(deser::Deserialize)]
    /// struct Query {
    ///     page: u32,
    /// }
    ///
    /// let query: Query = deser_urlencoded::from_str("page=1&page=2").unwrap();
    /// assert_eq!(query.page, 2);
    ///
    /// const STRICT: DeserializerConfig =
    ///     DeserializerConfig::new().duplicate_keys(DuplicateKeys::Error);
    /// assert!(STRICT.from_str::<Query>("page=1&page=2").is_err());
    /// ```
    pub const fn duplicate_keys(mut self, policy: DuplicateKeys) -> DeserializerConfig {
        self.duplicate_keys = policy;
        self
    }

    /// Sets how strings are decoded into bytes.
    ///
    /// Types that expect bytes (like `Vec<u8>`) decode values as base64
    /// by default, both with the standard and the URL-safe alphabet and
    /// with or without padding.  Values which are not UTF-8 after
    /// percent-decoding are passed on as bytes (see
    /// [`deser::adapters::bytes`]).
    pub const fn bytes(mut self, format: BytesFormat) -> DeserializerConfig {
        self.bytes = format;
        self
    }

    /// Enables or disables location tracking.
    ///
    /// The byte range of every event is always published into the state
    /// (see [`State::input_range`](deser::State::input_range)).  When
    /// enabled additionally the input is set as source (see
    /// [`State::source`](deser::State::source)).  This copies the input.
    pub const fn track_locations(mut self, yes: bool) -> DeserializerConfig {
        self.track_locations = yes;
        self
    }

    /// Deserializes a value from a query string.
    ///
    /// See [`from_str`](crate::from_str).
    pub fn from_str<'de, T: Deserialize<'de>>(&self, s: &'de str) -> Result<T, Error> {
        Deserializer::from_str_with_config(s, self).deserialize()
    }

    /// Deserializes a value from a query string in a byte slice.
    ///
    /// See [`from_slice`](crate::from_slice).
    pub fn from_slice<'de, T: Deserialize<'de>>(&self, bytes: &'de [u8]) -> Result<T, Error> {
        Deserializer::from_slice_with_config(bytes, self).deserialize()
    }
}

/// Deserializes query strings and form data.
///
/// Most of the time the [`from_str`](crate::from_str) and
/// [`from_slice`](crate::from_slice) functions (or the methods of the same
/// name on [`DeserializerConfig`]) are all that is needed.  The deserializer
/// is useful to configure the driver, for instance to add layers:
///
/// ```
/// use deser_path::{Path, PathLayer};
/// use deser_urlencoded::Deserializer;
///
/// #[derive(Debug, deser::Deserialize)]
/// struct Query {
///     filter: Filter,
/// }
///
/// #[derive(Debug, deser::Deserialize)]
/// struct Filter {
///     limit: u32,
/// }
///
/// let err = Deserializer::from_str("filter[limit]=ten")
///     .deserialize_with::<Query, _>(|driver| driver.push_layer(PathLayer::new()))
///     .unwrap_err();
/// assert_eq!(err.message(), "invalid value \"ten\", expected u32");
/// assert_eq!(err.attachment::<Path>().unwrap().to_string(), "filter.limit");
/// assert_eq!(err.offset(), Some(14));
/// ```
pub struct Deserializer<'a> {
    input: &'a str,
    /// An error that is reported instead of parsing (invalid UTF-8).
    error: Option<Error>,
    config: DeserializerConfig,
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
            error: None,
            config: config.clone(),
        }
    }

    /// Creates a new deserializer for a byte slice.
    ///
    /// The input must be UTF-8 (it's ASCII if it was percent-encoded),
    /// otherwise deserializing fails.
    pub fn from_slice(input: &'a [u8]) -> Deserializer<'a> {
        Deserializer::from_slice_with_config(input, &DeserializerConfig::new())
    }

    /// Creates a new deserializer for a byte slice with the given
    /// configuration.
    pub fn from_slice_with_config(
        input: &'a [u8],
        config: &DeserializerConfig,
    ) -> Deserializer<'a> {
        match std::str::from_utf8(input) {
            Ok(input) => Deserializer::from_str_with_config(input, config),
            Err(err) => Deserializer {
                input: "",
                error: Some(
                    Error::new(ErrorKind::Unexpected, "input is not valid UTF-8")
                        .with_offset(err.valid_up_to()),
                ),
                config: config.clone(),
            },
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Deserializes the input.
    ///
    /// To configure the deserialization (for instance to add layers) use
    /// [`deserialize_with`](Self::deserialize_with).
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        de::Deserializer::deserialize(self)
    }

    /// Deserializes the input with a configured driver.
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

    /// Parses the input and feeds the events into the given driver.
    ///
    /// The whole input is parsed before the first event is emitted, so
    /// malformed input is reported before any value is deserialized.  Keys
    /// and values that do not need to be decoded are passed on borrowed from
    /// the input (see [`emit_borrowed`](DeserializeDriver::emit_borrowed)).
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if let Some(err) = self.error.take() {
            return Err(err);
        }
        let tree = Tree::parse(self.input, &self.config)?;
        let state = driver.state_mut();
        if self.config.track_locations {
            state.set_source(self.input);
        }
        if self.config.bytes != BytesFormat::BASE64 {
            *state.get_mut::<BytesFormat>() = self.config.bytes;
        }
        if self.config.duplicate_keys != DuplicateKeys::Last {
            state.set_duplicate_keys(self.config.duplicate_keys);
        }
        tree.emit(driver)
            .map_err(|err| err.resolve_position(self.input.as_bytes()))
    }
}

impl<'a> de::Deserializer<'a> for Deserializer<'a> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        Deserializer::drive(self, driver)
    }
}

/// A byte range in the input.
type Range = (usize, usize);

/// The key of a node in its parent.
enum NodeKey<'a> {
    Root,
    /// A name (`a` or `[a]`).
    Name(Cow<'a, str>),
    /// An index (`[0]`) with its text.
    Index(usize, Cow<'a, str>),
    /// The next element of a sequence (`[]`).
    Push,
}

/// A key and its values.
struct Node<'a> {
    key: NodeKey<'a>,
    /// The range of the key that created the node.
    key_range: Range,
    /// The values given to the key itself.
    values: Vec<(Decoded<'a>, Range)>,
    /// The nested keys in the order in which they appear.
    children: Vec<usize>,
}

/// How a node with nested keys is emitted.
enum Container {
    Map(Vec<usize>),
    Seq(Vec<usize>),
}

/// Identifies the child of a node.
#[derive(PartialEq, Eq, Hash)]
enum ChildId<'a> {
    Name(Cow<'a, str>),
    Index(usize),
}

/// A segment of a key.
#[derive(Clone, Copy)]
enum Segment {
    Name(usize, usize),
    Index(usize, usize, usize),
    Push,
}

/// The parsed input: the keys as a tree with their values.
struct Tree<'a> {
    nodes: Vec<Node<'a>>,
    input_len: usize,
}

impl<'a> Tree<'a> {
    fn parse(input: &'a str, config: &DeserializerConfig) -> Result<Tree<'a>, Error> {
        let mut tree = Tree {
            nodes: vec![Node {
                key: NodeKey::Root,
                key_range: (0, input.len()),
                values: Vec::new(),
                children: Vec::new(),
            }],
            input_len: input.len(),
        };
        let mut lookup = HashMap::new();
        let mut segments = Vec::new();
        let mut params = 0;
        let mut start = usize::from(input.starts_with('?'));
        while start <= input.len() {
            let end = input[start..]
                .find('&')
                .map_or(input.len(), |pos| start + pos);
            let pair = &input[start..end];
            let pair_start = start;
            start = end + 1;
            if pair.is_empty() {
                continue;
            }
            params += 1;
            if params > config.max_params {
                return Err(Error::new(ErrorKind::Unexpected, "too many parameters")
                    .with_offset(pair_start));
            }
            let (raw_key, raw_value, value_start) = match pair.find('=') {
                Some(pos) => (&pair[..pos], &pair[pos + 1..], pair_start + pos + 1),
                None => (pair, "", end),
            };
            let key_range = (pair_start, pair_start + raw_key.len());
            let key = match decode(raw_key) {
                Decoded::Text(key) => key,
                Decoded::Bytes(_) => {
                    return Err(Error::new(ErrorKind::Unexpected, "key is not valid UTF-8")
                        .with_offset(key_range.0));
                }
            };
            let value = decode(raw_value);

            segments.clear();
            let first = split_key(&key, config.nesting, &mut segments);
            if segments.len() > config.max_depth {
                return Err(
                    Error::new(ErrorKind::Unexpected, "key is nested too deeply")
                        .with_offset(key_range.0),
                );
            }
            let mut node = tree.child(&mut lookup, 0, Segment::Name(0, first), &key, key_range);
            for segment in segments.iter().copied() {
                node = tree.child(&mut lookup, node, segment, &key, key_range);
            }
            tree.nodes[node].values.push((value, (value_start, end)));
        }
        Ok(tree)
    }

    /// Returns the child of a node, creates it if needed.
    // the key is a `Cow` as borrowed keys borrow from the input for `'a`
    #[allow(clippy::ptr_arg)]
    fn child(
        &mut self,
        lookup: &mut HashMap<(usize, ChildId<'a>), usize>,
        parent: usize,
        segment: Segment,
        key: &Cow<'a, str>,
        key_range: Range,
    ) -> usize {
        let (id, node_key) = match segment {
            Segment::Name(start, end) => {
                let name = sub_cow(key, start, end);
                (Some(ChildId::Name(name.clone())), NodeKey::Name(name))
            }
            Segment::Index(index, start, end) => (
                Some(ChildId::Index(index)),
                NodeKey::Index(index, sub_cow(key, start, end)),
            ),
            Segment::Push => (None, NodeKey::Push),
        };
        let id = id.map(|id| (parent, id));
        if let Some(ref id) = id
            && let Some(&child) = lookup.get(id)
        {
            return child;
        }
        let child = self.nodes.len();
        self.nodes.push(Node {
            key: node_key,
            key_range,
            values: Vec::new(),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(child);
        if let Some(id) = id {
            lookup.insert(id, child);
        }
        child
    }

    /// Decides how a node with nested keys is emitted.
    fn container(&self, node: &Node<'a>) -> Result<Container, Error> {
        let (mut names, mut indexes, mut pushes) = (false, false, false);
        for &child in &node.children {
            match self.nodes[child].key {
                NodeKey::Name(_) | NodeKey::Root => names = true,
                NodeKey::Index(..) => indexes = true,
                NodeKey::Push => pushes = true,
            }
        }
        if pushes && (names || indexes) {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "`[]` cannot be combined with other nested keys",
            )
            .with_offset(node.key_range.0));
        }
        if pushes {
            return Ok(Container::Seq(node.children.clone()));
        }
        if indexes && !names {
            // indexes from 0 without gaps are a sequence, others a map
            let mut sorted = node.children.clone();
            sorted.sort_by_key(|&child| match self.nodes[child].key {
                NodeKey::Index(index, _) => index,
                _ => unreachable!(),
            });
            let dense = sorted.iter().enumerate().all(|(pos, &child)| {
                matches!(self.nodes[child].key, NodeKey::Index(index, _) if index == pos)
            });
            if dense {
                return Ok(Container::Seq(sorted));
            }
        }
        Ok(Container::Map(node.children.clone()))
    }

    /// Emits the events of the tree.
    fn emit(&self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        struct Frame {
            children: Vec<usize>,
            pos: usize,
            is_map: bool,
            range: Range,
        }

        let root = &self.nodes[0];
        let end = (self.input_len, self.input_len);
        emit_at(
            driver,
            Event::MapStart(ContainerShape::new().with_len(root.children.len())),
            root.key_range,
        )?;
        let mut stack = vec![Frame {
            children: root.children.clone(),
            pos: 0,
            is_map: true,
            range: end,
        }];

        while let Some(frame) = stack.last_mut() {
            let Some(&child) = frame.children.get(frame.pos) else {
                let event = if frame.is_map {
                    Event::MapEnd
                } else {
                    Event::SeqEnd
                };
                let range = frame.range;
                stack.pop();
                emit_at(driver, event, range)?;
                continue;
            };
            frame.pos += 1;
            let is_map = frame.is_map;
            let node = &self.nodes[child];
            if is_map {
                driver
                    .state_mut()
                    .set_input_range(node.key_range.0, node.key_range.1);
                match node.key {
                    NodeKey::Name(ref name) | NodeKey::Index(_, ref name) => {
                        emit_lexical(driver, name)?
                    }
                    NodeKey::Root | NodeKey::Push => unreachable!(),
                }
            }

            match (&node.values[..], node.children.is_empty()) {
                ([(value, range)], true) => emit_value(driver, value, *range)?,
                (values, true) => {
                    let shape = ContainerShape::new()
                        .with_len(values.len())
                        .with_repeated(true);
                    emit_at(driver, Event::SeqStart(shape), node.key_range)?;
                    for (value, range) in values {
                        emit_value(driver, value, *range)?;
                    }
                    emit_at(driver, Event::SeqEnd, node.key_range)?;
                }
                ([], false) => {
                    let (children, is_map) = match self.container(node)? {
                        Container::Map(children) => (children, true),
                        Container::Seq(children) => (children, false),
                    };
                    let shape = ContainerShape::new().with_len(children.len());
                    let event = if is_map {
                        Event::MapStart(shape)
                    } else {
                        Event::SeqStart(shape)
                    };
                    emit_at(driver, event, node.key_range)?;
                    stack.push(Frame {
                        children,
                        pos: 0,
                        is_map,
                        range: node.key_range,
                    });
                }
                (_, false) => {
                    return Err(Error::new(
                        ErrorKind::Unexpected,
                        "key has a value and nested keys",
                    )
                    .with_offset(node.key_range.0));
                }
            }
        }
        Ok(())
    }
}

/// Emits an event with a byte range.
#[inline]
fn emit_at<'e, E: Into<Event<'e>>>(
    driver: &mut DeserializeDriver<'_, '_>,
    event: E,
    range: Range,
) -> Result<(), Error> {
    driver.state_mut().set_input_range(range.0, range.1);
    driver.emit(event)
}

/// Emits text as lexical atom, borrowed if it's a slice of the input.
// the text is a `Cow` as borrowed text is passed on for `'a`
#[allow(clippy::ptr_arg)]
#[inline]
fn emit_lexical<'a>(
    driver: &mut DeserializeDriver<'_, 'a>,
    text: &Cow<'a, str>,
) -> Result<(), Error> {
    match *text {
        Cow::Borrowed(text) => driver.emit_borrowed(Atom::Lexical(Cow::Borrowed(text))),
        Cow::Owned(ref text) => driver.emit(Atom::Lexical(Cow::Borrowed(text.as_str()))),
    }
}

/// Emits a value.
fn emit_value<'a>(
    driver: &mut DeserializeDriver<'_, 'a>,
    value: &Decoded<'a>,
    range: Range,
) -> Result<(), Error> {
    driver.state_mut().set_input_range(range.0, range.1);
    match *value {
        Decoded::Text(ref text) => emit_lexical(driver, text),
        Decoded::Bytes(ref bytes) => driver.emit(Atom::Bytes(Bytes::borrowed(bytes))),
    }
}

/// Returns a part of a key, borrowed from the input if the key is.
fn sub_cow<'a>(key: &Cow<'a, str>, start: usize, end: usize) -> Cow<'a, str> {
    match *key {
        Cow::Borrowed(key) => Cow::Borrowed(&key[start..end]),
        Cow::Owned(ref key) => Cow::Owned(key[start..end].to_string()),
    }
}

/// Splits a key into its first name and the nested segments.
///
/// Returns the end of the first name.  Keys that do not follow the syntax
/// of the nesting (like `a[b` or `[a]`) are taken as they are.
fn split_key(key: &str, nesting: Nesting, segments: &mut Vec<Segment>) -> usize {
    let bytes = key.as_bytes();
    let (open, first_end) = match nesting {
        Nesting::Flat => return key.len(),
        Nesting::Brackets => (b'[', bytes.iter().position(|&b| b == b'[')),
        Nesting::Dots => (b'.', bytes.iter().position(|&b| b == b'.')),
    };
    let first_end = match first_end {
        Some(0) | None => return key.len(),
        Some(end) => end,
    };
    let mut pos = first_end;
    while pos < bytes.len() {
        debug_assert_eq!(bytes[pos], open);
        let (start, end, next) = if open == b'[' {
            let close = match bytes[pos + 1..].iter().position(|&b| b == b']') {
                Some(close) => pos + 1 + close,
                None => break,
            };
            (pos + 1, close, close + 1)
        } else {
            let end = bytes[pos + 1..]
                .iter()
                .position(|&b| b == b'.')
                .map_or(bytes.len(), |end| pos + 1 + end);
            (pos + 1, end, end)
        };
        let text = &key[start..end];
        let valid_next = next == bytes.len() || bytes[next] == open;
        if !valid_next || (open == b'[' && text.contains('[')) || (open == b'.' && text.is_empty())
        {
            break;
        }
        segments.push(if text.is_empty() {
            Segment::Push
        } else if text.bytes().all(|b| b.is_ascii_digit()) {
            match text.parse() {
                Ok(index) => Segment::Index(index, start, end),
                Err(_) => Segment::Name(start, end),
            }
        } else {
            Segment::Name(start, end)
        });
        pos = next;
    }
    if pos < bytes.len() {
        // malformed, the key is taken as it is
        segments.clear();
        return key.len();
    }
    first_end
}

#[cfg(test)]
fn split(key: &str, nesting: Nesting) -> Vec<String> {
    let mut segments = Vec::new();
    let first = split_key(key, nesting, &mut segments);
    let mut rv = vec![key[..first].to_string()];
    for segment in segments {
        rv.push(match segment {
            Segment::Name(start, end) => key[start..end].to_string(),
            Segment::Index(index, _, _) => format!("#{}", index),
            Segment::Push => "[]".to_string(),
        });
    }
    rv
}

#[test]
fn test_split_key() {
    let b = Nesting::Brackets;
    assert_eq!(split("a", b), ["a"]);
    assert_eq!(split("a[b][0][]", b), ["a", "b", "#0", "[]"]);
    assert_eq!(split("a[b.c]", b), ["a", "b.c"]);
    assert_eq!(split("a[]", b), ["a", "[]"]);
    assert_eq!(split("a[007]", b), ["a", "#7"]);
    assert_eq!(
        split("a[99999999999999999999999]", b),
        ["a", "99999999999999999999999"]
    );
    // malformed keys are taken as they are
    for key in ["a[b", "[a]", "a[b]c", "a[b[c]]", "a]", "a[b]]"] {
        assert_eq!(split(key, b), [key], "{}", key);
    }

    let d = Nesting::Dots;
    assert_eq!(split("a.b.0", d), ["a", "b", "#0"]);
    assert_eq!(split("a[b]", d), ["a[b]"]);
    for key in ["a.", "a..b", ".a"] {
        assert_eq!(split(key, d), [key], "{}", key);
    }

    assert_eq!(split("a[b].c", Nesting::Flat), ["a[b].c"]);
}
