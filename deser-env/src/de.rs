use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;

use deser_core::Text;
use deser_core::de::{
    self, Deserialize, DeserializeDriver, DeserializeOwned, DuplicateKeys, LexicalRules,
};
use deser_core::{Atom, Bytes, ContainerShape, Error, ErrorKind, Event};

use crate::{Case, EnvVar};

/// Configures how environment variables are deserialized.
///
/// The configuration is independent of the environment so it can be
/// created once (even as a constant) and used many times.  The methods
/// [`from_env`](Self::from_env) and [`from_vars`](Self::from_vars) work like
/// the functions of the same name.
///
/// ```
/// use std::collections::BTreeMap;
/// use deser_env::DeserializerConfig;
///
/// const CONFIG: DeserializerConfig =
///     DeserializerConfig::new().separator("_");
/// let value: BTreeMap<String, BTreeMap<String, u32>> =
///     CONFIG.from_vars("APP_", [("APP_SERVER_PORT", "80")]).unwrap();
/// assert_eq!(value["server"]["port"], 80);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeserializerConfig {
    separator: &'static str,
    case: Case,
    max_depth: usize,
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
            separator: "__",
            case: Case::Upper,
            max_depth: 16,
        }
    }

    /// Sets the separator of nested keys.
    ///
    /// The default is `__` (`APP_SERVER__PORT` is `server.port`).  A single
    /// `_` cannot be told apart from the underscores in names, so
    /// `APP_MAX_CONNECTIONS` would be `max.connections`.  The empty string
    /// disables nesting, names are keys as they are.
    pub const fn separator(mut self, separator: &'static str) -> DeserializerConfig {
        self.separator = separator;
        self
    }

    /// Sets how the case of names maps onto keys.
    ///
    /// The default is [`Case::Upper`] which lowercases the names.
    pub const fn case(mut self, case: Case) -> DeserializerConfig {
        self.case = case;
        self
    }

    /// Sets how deeply keys can be nested.
    ///
    /// This is the number of separators in a name (after the prefix).
    /// Names that are nested deeper are an error.  The default is 16.
    pub const fn max_depth(mut self, depth: usize) -> DeserializerConfig {
        self.max_depth = depth;
        self
    }

    /// Deserializes a value from the environment variables with a prefix.
    ///
    /// See [`from_env`](crate::from_env).
    pub fn from_env<T: DeserializeOwned>(&self, prefix: &str) -> Result<T, Error> {
        Deserializer::from_env_with_config(prefix, self).deserialize()
    }

    /// Deserializes a value from the given variables with a prefix.
    ///
    /// See [`from_vars`](crate::from_vars).
    pub fn from_vars<'a, T, I, K, V>(&self, prefix: &str, vars: I) -> Result<T, Error>
    where
        T: Deserialize<'a>,
        I: IntoIterator<Item = (K, V)>,
        K: Into<Cow<'a, str>>,
        V: Into<Cow<'a, str>>,
    {
        Deserializer::from_vars_with_config(prefix, vars, self).deserialize()
    }
}

/// The value of a variable.
enum Value<'a> {
    Text(Cow<'a, str>),
    /// A value that is not valid unicode.
    Bytes(Vec<u8>),
}

/// A variable with a name that starts with the prefix.
struct Var<'a> {
    /// The full name (for errors).
    name: Arc<str>,
    /// The length of the prefix in the name.
    prefix_len: usize,
    value: Value<'a>,
}

/// Deserializes environment variables.
///
/// The variables are read when the deserializer is created.  Most of the
/// time the [`from_env`](crate::from_env) and
/// [`from_vars`](crate::from_vars) functions (or the methods of the same
/// name on [`DeserializerConfig`]) are all that is needed.  The deserializer
/// is useful to configure the driver, for instance to add layers, or to
/// update a value (see [`update`](deser_core::de::Deserializer::update)):
///
/// ```
/// use deser::de::Deserializer as _;
/// use deser_env::Deserializer;
///
/// #[derive(Debug, deser::Deserialize)]
/// struct Config {
///     host: String,
///     port: u16,
/// }
///
/// let mut config = Config { host: "localhost".into(), port: 80 };
/// Deserializer::from_vars("APP_", [("APP_PORT", "8080")])
///     .update(&mut config)
///     .unwrap();
/// assert_eq!(config.host, "localhost");
/// assert_eq!(config.port, 8080);
/// ```
pub struct Deserializer<'a> {
    vars: Vec<Var<'a>>,
    /// An error that is reported instead of deserializing.
    error: Option<Error>,
    config: DeserializerConfig,
}

impl Deserializer<'static> {
    /// Creates a deserializer for the environment variables with a prefix.
    pub fn from_env(prefix: &str) -> Deserializer<'static> {
        Deserializer::from_env_with_config(prefix, &DeserializerConfig::new())
    }

    /// Creates a deserializer for the environment variables with a prefix
    /// and the given configuration.
    ///
    /// Variables with names that are not valid unicode are skipped unless
    /// they start with the prefix, which is an error.  Values that are not
    /// valid unicode are passed on as bytes on Unix and are an error on
    /// other platforms.
    pub fn from_env_with_config(
        prefix: &str,
        config: &DeserializerConfig,
    ) -> Deserializer<'static> {
        let mut rv = Deserializer {
            vars: Vec::new(),
            error: None,
            config: config.clone(),
        };
        for (name, value) in std::env::vars_os() {
            let name = match name.into_string() {
                Ok(name) => name,
                Err(name) => {
                    if strip_prefix(&name.to_string_lossy(), prefix).is_some() {
                        rv.fail(Error::new(
                            ErrorKind::Syntax,
                            format!(
                                "the name of the environment variable {:?} is not valid unicode",
                                name
                            ),
                        ));
                    }
                    continue;
                }
            };
            if strip_prefix(&name, prefix).is_none() {
                continue;
            }
            let value = match value.into_string() {
                Ok(value) => Value::Text(Cow::Owned(value)),
                Err(value) => match os_bytes(value) {
                    Some(bytes) => Value::Bytes(bytes),
                    None => {
                        rv.fail(
                            Error::new(ErrorKind::Syntax, "value is not valid unicode")
                                .with_attachment(EnvVar::new(name.as_str().into())),
                        );
                        continue;
                    }
                },
            };
            rv.push(name.into(), prefix.len(), value);
        }
        rv.finish_vars();
        rv
    }
}

impl<'a> Deserializer<'a> {
    /// Creates a deserializer for the given variables with a prefix.
    ///
    /// The variables are name-value pairs, for instance from
    /// [`std::env::vars`] or a file.  Values that are given borrowed are
    /// passed on borrowed.
    pub fn from_vars<I, K, V>(prefix: &str, vars: I) -> Deserializer<'a>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<Cow<'a, str>>,
        V: Into<Cow<'a, str>>,
    {
        Deserializer::from_vars_with_config(prefix, vars, &DeserializerConfig::new())
    }

    /// Creates a deserializer for the given variables with a prefix and
    /// the given configuration.
    pub fn from_vars_with_config<I, K, V>(
        prefix: &str,
        vars: I,
        config: &DeserializerConfig,
    ) -> Deserializer<'a>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<Cow<'a, str>>,
        V: Into<Cow<'a, str>>,
    {
        let mut rv = Deserializer {
            vars: Vec::new(),
            error: None,
            config: config.clone(),
        };
        for (name, value) in vars {
            let name = name.into();
            if strip_prefix(&name, prefix).is_some() {
                rv.push(Arc::from(&*name), prefix.len(), Value::Text(value.into()));
            }
        }
        rv.finish_vars();
        rv
    }

    fn push(&mut self, name: Arc<str>, prefix_len: usize, value: Value<'a>) {
        // the name is only the prefix
        if name.len() > prefix_len {
            self.vars.push(Var {
                name,
                prefix_len,
                value,
            });
        }
    }

    fn fail(&mut self, err: Error) {
        if self.error.is_none() {
            self.error = Some(err);
        }
    }

    /// Sorts the variables, the order of the environment is arbitrary.
    fn finish_vars(&mut self) {
        self.vars.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Returns the configuration.
    pub fn config(&self) -> &DeserializerConfig {
        &self.config
    }

    /// Deserializes the variables.
    ///
    /// To configure the deserialization (for instance to add layers) use
    /// [`deserialize_with`](Self::deserialize_with).
    pub fn deserialize<T: Deserialize<'a>>(&mut self) -> Result<T, Error> {
        de::Deserializer::deserialize(self)
    }

    /// Deserializes the variables with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// deserialized, for instance to add [`Layer`](deser_core::de::Layer)s.
    pub fn deserialize_with<T, F>(&mut self, setup: F) -> Result<T, Error>
    where
        T: Deserialize<'a>,
        F: FnOnce(&mut DeserializeDriver<'_, 'a>),
    {
        de::Deserializer::deserialize_with(self, setup)
    }

    /// Deserializes the next value in a context.
    ///
    /// The values of the context are the defaults of the extension values
    /// of the state (see [`Context`](deser_core::Context)).
    pub fn deserialize_in<T: Deserialize<'a>>(
        &mut self,
        context: &deser_core::Context,
    ) -> Result<T, Error> {
        de::Deserializer::deserialize_in(self, context)
    }

    /// Feeds the events of the variables into the given driver.
    ///
    /// The variables are a map (see the [crate documentation](crate)).  All
    /// names are checked before the first event is emitted.  The name of
    /// the variable an event comes from is attached to it and to the errors
    /// it causes (see [`EnvVar`]).  Values that are given borrowed are passed
    /// on borrowed.
    pub fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        if let Some(err) = self.error.take() {
            return Err(err);
        }
        let tree = Tree::build(&self.vars, &self.config)?;
        let state = driver.state_mut();
        // the last value of repeated keys is used unless the context says
        // otherwise
        DuplicateKeys::Last.set_default(state);
        LexicalRules::LENIENT.set(state);
        state.add_error_context::<CurrentVar>();
        tree.emit(&self.vars, driver)
    }
}

impl<'a> de::Deserializer<'a> for Deserializer<'a> {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'a>) -> Result<(), Error> {
        Deserializer::drive(self, driver)
    }
}

/// Returns the name without the prefix if it starts with the prefix.
///
/// On Windows names are not case sensitive, the prefix is matched ignoring
/// ASCII case.
fn strip_prefix<'n>(name: &'n str, prefix: &str) -> Option<&'n str> {
    if cfg!(windows) {
        let head = name.get(..prefix.len())?;
        head.eq_ignore_ascii_case(prefix)
            .then(|| &name[prefix.len()..])
    } else {
        name.strip_prefix(prefix)
    }
}

/// Returns the bytes of a value that is not valid unicode.
#[cfg(unix)]
pub(crate) fn os_bytes(value: OsString) -> Option<Vec<u8>> {
    use std::os::unix::ffi::OsStringExt;
    Some(value.into_vec())
}

#[cfg(not(unix))]
pub(crate) fn os_bytes(_value: OsString) -> Option<Vec<u8>> {
    None
}

/// The name of the variable of the current event.
///
/// This is event data which is attached to errors as [`EnvVar`].
#[derive(Debug, Default, Clone)]
struct CurrentVar(Option<Arc<str>>);

impl deser_core::ErrorContext for CurrentVar {
    fn add_context(err: Error, state: &deser_core::State) -> Error {
        if err.attachment::<EnvVar>().is_some() {
            return err;
        }
        match state.event::<CurrentVar>() {
            Some(CurrentVar(Some(name))) => err.with_attachment(EnvVar::new(name.clone())),
            _ => err,
        }
    }
}

/// The key of a node in its parent.
enum NodeKey {
    Root,
    Name(String),
    /// An index with its text.
    Index(usize, String),
}

/// A key and its values.
struct Node {
    key: NodeKey,
    /// The name up to the key, the name of the variable for keys with
    /// values.
    name: Option<Arc<str>>,
    /// The variables that give the key a value.
    values: Vec<usize>,
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
enum ChildId {
    Name(String),
    Index(usize),
}

/// The variables as a tree of keys.
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    fn build(vars: &[Var<'_>], config: &DeserializerConfig) -> Result<Tree, Error> {
        let mut tree = Tree {
            nodes: vec![Node {
                key: NodeKey::Root,
                name: None,
                values: Vec::new(),
                children: Vec::new(),
            }],
        };
        let mut lookup = HashMap::new();
        let mut segments = Vec::new();
        for (index, var) in vars.iter().enumerate() {
            segments.clear();
            split_name(&var.name[var.prefix_len..], config.separator, &mut segments);
            if segments.len() > config.max_depth + 1 {
                return Err(
                    Error::new(ErrorKind::LimitExceeded, "name is nested too deeply")
                        .with_attachment(EnvVar::new(var.name.clone())),
                );
            }
            let mut node = 0;
            for (depth, &(start, end)) in segments.iter().enumerate() {
                let text = &var.name[var.prefix_len + start..var.prefix_len + end];
                let text = match config.case {
                    Case::Upper => text.to_ascii_lowercase(),
                    Case::Preserve => text.to_string(),
                };
                let id = match text.parse() {
                    Ok(index) if depth > 0 && text.bytes().all(|b| b.is_ascii_digit()) => {
                        ChildId::Index(index)
                    }
                    _ => ChildId::Name(text),
                };
                let id = (node, id);
                node = match lookup.get(&id) {
                    Some(&child) => child,
                    None => {
                        let name = if depth + 1 == segments.len() {
                            var.name.clone()
                        } else {
                            Arc::from(&var.name[..var.prefix_len + end])
                        };
                        tree.child(&mut lookup, id, name)
                    }
                };
            }
            tree.nodes[node].values.push(index);
        }
        Ok(tree)
    }

    /// Creates the child of a node.
    fn child(
        &mut self,
        lookup: &mut HashMap<(usize, ChildId), usize>,
        id: (usize, ChildId),
        name: Arc<str>,
    ) -> usize {
        let parent = id.0;
        let key = match id.1 {
            ChildId::Name(ref text) => NodeKey::Name(text.clone()),
            ChildId::Index(index) => NodeKey::Index(index, index.to_string()),
        };
        let child = self.nodes.len();
        self.nodes.push(Node {
            key,
            name: Some(name),
            values: Vec::new(),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(child);
        lookup.insert(id, child);
        child
    }

    /// Decides how a node with nested keys is emitted.
    ///
    /// Indexes that start at 0 and have no gaps are a sequence, everything
    /// else is a map.
    fn container(&self, node: &Node) -> Container {
        let indexes = node
            .children
            .iter()
            .all(|&child| matches!(self.nodes[child].key, NodeKey::Index(..)));
        if indexes {
            let mut sorted = node.children.clone();
            sorted.sort_by_key(|&child| match self.nodes[child].key {
                NodeKey::Index(index, _) => index,
                _ => unreachable!(),
            });
            let dense = sorted.iter().enumerate().all(|(pos, &child)| {
                matches!(self.nodes[child].key, NodeKey::Index(index, _) if index == pos)
            });
            if dense {
                return Container::Seq(sorted);
            }
        }
        Container::Map(node.children.clone())
    }

    /// Returns the shape of a map with children.
    ///
    /// Maps are multimaps, the keys of the children are given once per
    /// variable.
    fn map_shape(&self, children: &[usize]) -> ContainerShape {
        let len = children
            .iter()
            .map(|&child| self.nodes[child].values.len().max(1))
            .sum();
        ContainerShape::new().with_len(len).with_multimap(true)
    }

    /// Emits the events of the tree.
    fn emit<'a>(
        &self,
        vars: &[Var<'a>],
        driver: &mut DeserializeDriver<'_, 'a>,
    ) -> Result<(), Error> {
        struct Frame {
            children: Vec<usize>,
            pos: usize,
            is_map: bool,
        }

        let root = &self.nodes[0];
        emit_as(
            driver,
            Event::MapStart(self.map_shape(&root.children)),
            None,
        )?;
        let mut stack = vec![Frame {
            children: root.children.clone(),
            pos: 0,
            is_map: true,
        }];

        while let Some(frame) = stack.last_mut() {
            let Some(&child) = frame.children.get(frame.pos) else {
                let event = if frame.is_map {
                    Event::MapEnd
                } else {
                    Event::SeqEnd
                };
                stack.pop();
                emit_as(driver, event, None)?;
                continue;
            };
            frame.pos += 1;
            let is_map = frame.is_map;
            let node = &self.nodes[child];
            if is_map {
                emit_key(driver, node)?;
            }

            match (&node.values[..], node.children.is_empty()) {
                (&[var], true) => emit_value(driver, &vars[var])?,
                // the key is emitted for every variable (maps are
                // multimaps)
                (&[first, ref rest @ ..], true) if is_map => {
                    emit_value(driver, &vars[first])?;
                    for &var in rest {
                        emit_key(driver, node)?;
                        emit_value(driver, &vars[var])?;
                    }
                }
                // an index of a sequence given more than once
                (values @ [_, _, ..], true) => {
                    let policy = DuplicateKeys::of(driver.state());
                    let var = match policy {
                        DuplicateKeys::First => values[0],
                        DuplicateKeys::Error => {
                            return Err(Error::new(
                                ErrorKind::Syntax,
                                "more than one variable for the same index",
                            )
                            .with_attachment(EnvVar::new(vars[values[1]].name.clone())));
                        }
                        _ => values[values.len() - 1],
                    };
                    emit_value(driver, &vars[var])?;
                }
                // nodes have values or nested variables
                ([], true) => unreachable!(),
                ([], false) => {
                    let (children, is_map) = match self.container(node) {
                        Container::Map(children) => (children, true),
                        Container::Seq(children) => (children, false),
                    };
                    let event = if is_map {
                        Event::MapStart(self.map_shape(&children))
                    } else {
                        Event::SeqStart(ContainerShape::new().with_len(children.len()))
                    };
                    emit_as(driver, event, node.name.as_ref())?;
                    stack.push(Frame {
                        children,
                        pos: 0,
                        is_map,
                    });
                }
                (&[var, ..], false) => {
                    return Err(Error::new(
                        ErrorKind::Syntax,
                        "variable has a value and nested variables",
                    )
                    .with_attachment(EnvVar::new(vars[var].name.clone())));
                }
            }
        }
        Ok(())
    }
}

/// Emits the key of a node.
fn emit_key(driver: &mut DeserializeDriver<'_, '_>, node: &Node) -> Result<(), Error> {
    let key = match node.key {
        NodeKey::Name(ref text) | NodeKey::Index(_, ref text) => text,
        NodeKey::Root => unreachable!(),
    };
    emit_as(
        driver,
        Atom::Lexical(Text::borrowed(key.as_str())),
        node.name.as_ref(),
    )
}

/// Emits an event with the name of the variable it comes from.
#[inline]
fn emit_as<'e, E: Into<Event<'e>>>(
    driver: &mut DeserializeDriver<'_, '_>,
    event: E,
    name: Option<&Arc<str>>,
) -> Result<(), Error> {
    if let Some(name) = name {
        driver.state_mut().event_mut::<CurrentVar>().0 = Some(name.clone());
    }
    driver.emit(event)
}

/// Emits the value of a variable, borrowed if the variable was given
/// borrowed.
fn emit_value<'a>(driver: &mut DeserializeDriver<'_, 'a>, var: &Var<'a>) -> Result<(), Error> {
    driver.state_mut().event_mut::<CurrentVar>().0 = Some(var.name.clone());
    match var.value {
        Value::Text(Cow::Borrowed(text)) => {
            driver.emit_borrowed(Atom::Lexical(Text::borrowed(text)))
        }
        Value::Text(Cow::Owned(ref text)) => {
            driver.emit(Atom::Lexical(Text::borrowed(text.as_str())))
        }
        Value::Bytes(ref bytes) => driver.emit(Atom::Bytes(Bytes::borrowed(bytes))),
    }
}

/// Splits a name (without the prefix) into the ranges of its segments.
///
/// Names that start or end with the separator or have two separators in a
/// row are not split, they are taken as they are.
fn split_name(name: &str, separator: &str, segments: &mut Vec<(usize, usize)>) {
    if !separator.is_empty() {
        let mut start = 0;
        for (pos, _) in name.match_indices(separator) {
            segments.push((start, pos));
            start = pos + separator.len();
        }
        segments.push((start, name.len()));
        if segments.iter().all(|&(start, end)| start < end) {
            return;
        }
        segments.clear();
    }
    segments.push((0, name.len()));
}

#[cfg(test)]
fn split(name: &str, separator: &str) -> Vec<String> {
    let mut segments = Vec::new();
    split_name(name, separator, &mut segments);
    segments
        .into_iter()
        .map(|(start, end)| name[start..end].to_string())
        .collect()
}

#[test]
fn test_split_name() {
    assert_eq!(split("PORT", "__"), ["PORT"]);
    assert_eq!(
        split("SERVER__MAX_CONNECTIONS", "__"),
        ["SERVER", "MAX_CONNECTIONS"]
    );
    assert_eq!(split("A__0__B", "__"), ["A", "0", "B"]);
    assert_eq!(split("A_B", "_"), ["A", "B"]);
    assert_eq!(split("A__B", ""), ["A__B"]);
    // three underscores are a separator followed by an underscore
    assert_eq!(split("A___B", "__"), ["A", "_B"]);
    // malformed names are taken as they are
    for name in ["__A", "A__", "A____B", "__"] {
        assert_eq!(split(name, "__"), [name], "{}", name);
    }
}
