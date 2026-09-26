//! This library takes a [`Serialize`] and formats it with [`std::fmt`] like
//! the [`Debug`](std::fmt::Debug) implementation of Rust types would.
//!
//! The Rust shape of the values (struct and variant names, `Option`,
//! tuples, ...) is taken from their description (see
//! [`Describe`]).  For types which describe
//! themselves the output matches `#[derive(Debug)]`:
//!
//! ```
//! use deser::Serialize;
//! use deser_debug::ToDebug;
//!
//! #[derive(Serialize, Debug)]
//! struct Point {
//!     x: i32,
//!     y: Option<i32>,
//! }
//!
//! let point = Point { x: 1, y: Some(2) };
//! assert_eq!(ToDebug::new(&point).to_string(), format!("{:?}", point));
//! assert_eq!(ToDebug::new(&point).to_string(), "Point { x: 1, y: Some(2) }");
//! ```
//!
//! As all floats are `f64` in the data model, `f32` values are formatted
//! as the `f64` they widen to.
use std::fmt;

use deser::ser::{Describe, Serialize, SerializeDriver, Variant, VariantKind, VariantRepr};
use deser::{Atom, Event};

/// Serializes a serializable value to `Debug` format.
pub struct ToDebug {
    root: Node,
}

impl fmt::Display for ToDebug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl fmt::Debug for ToDebug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&Render(&self.root, &self.root.desc), f)
    }
}

impl ToDebug {
    /// Creates a new [`ToDebug`] object from a serializable value.
    ///
    /// # Panics
    ///
    /// Panics if the value fails to serialize.
    pub fn new(value: &dyn Serialize) -> ToDebug {
        let mut builder = Builder::default();
        SerializeDriver::new(value)
            .drive_described(|event, value, state| {
                builder.event(event, value, state.is_map_key());
                Ok(())
            })
            .unwrap();
        ToDebug {
            root: builder.root.expect("no value was serialized"),
        }
    }
}

/// A part of the description of a value.
#[derive(Debug)]
enum Desc {
    Structure(String),
    Newtype(String),
    Variant {
        name: String,
        kind: VariantKind,
        repr: Repr,
    },
    Some,
    None,
    Tuple,
    Set,
}

/// An owned [`VariantRepr`].
#[derive(Debug)]
enum Repr {
    External,
    Internal { tag: String },
    Adjacent { content: String },
    Untagged,
}

/// Collects the description of a value.
#[derive(Default)]
struct Collector(Vec<Desc>);

impl Describe for Collector {
    fn structure(&mut self, name: &str) {
        self.0.push(Desc::Structure(name.into()));
    }

    fn newtype(&mut self, name: &str) {
        self.0.push(Desc::Newtype(name.into()));
    }

    fn variant(&mut self, variant: &Variant<'_>) {
        let repr = match variant.repr {
            VariantRepr::Internal { tag } => Repr::Internal { tag: tag.into() },
            VariantRepr::Adjacent { content, .. } => Repr::Adjacent {
                content: content.into(),
            },
            VariantRepr::Untagged => Repr::Untagged,
            _ => Repr::External,
        };
        self.0.push(Desc::Variant {
            name: variant.name.into(),
            kind: variant.kind,
            repr,
        });
    }

    fn some(&mut self) {
        self.0.push(Desc::Some);
    }

    fn none(&mut self) {
        self.0.push(Desc::None);
    }

    fn tuple(&mut self) {
        self.0.push(Desc::Tuple);
    }

    fn set(&mut self) {
        self.0.push(Desc::Set);
    }
}

/// A serialized value with its description.
#[derive(Debug)]
struct Node {
    desc: Vec<Desc>,
    kind: NodeKind,
}

#[derive(Debug)]
enum NodeKind {
    Atom(Atom<'static>),
    Map(Vec<(Node, Node)>),
    Seq(Vec<Node>),
}

impl Node {
    /// Returns the value of an entry of a map node by string key.
    fn get(&self, key: &str) -> Option<&Node> {
        match self.kind {
            NodeKind::Map(ref entries) => entries
                .iter()
                .find(|(k, _)| k.as_str() == Some(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self.kind {
            NodeKind::Atom(Atom::Str(ref s)) => Some(s),
            _ => None,
        }
    }
}

/// Builds the tree of nodes from the events.
#[derive(Default)]
struct Builder {
    stack: Vec<Node>,
    // keys of maps waiting for their value
    keys: Vec<Option<Node>>,
    root: Option<Node>,
}

impl Builder {
    fn event(&mut self, event: Event<'_>, value: &dyn Serialize, is_key: bool) {
        let describe = |value: &dyn Serialize| {
            // keys are not described, the parent describes them
            let mut collector = Collector::default();
            if !is_key {
                value.describe(&mut collector);
            }
            collector.0
        };
        match event {
            Event::Atom(atom) => {
                let node = Node {
                    desc: describe(value),
                    kind: NodeKind::Atom(atom.to_static()),
                };
                self.push(node);
            }
            Event::MapStart(_) => {
                self.stack.push(Node {
                    desc: describe(value),
                    kind: NodeKind::Map(Vec::new()),
                });
                self.keys.push(None);
            }
            Event::SeqStart(_) => {
                self.stack.push(Node {
                    desc: describe(value),
                    kind: NodeKind::Seq(Vec::new()),
                });
                self.keys.push(None);
            }
            Event::MapEnd | Event::SeqEnd => {
                self.keys.pop();
                let node = self.stack.pop().expect("unbalanced events");
                self.push(node);
            }
        }
    }

    fn push(&mut self, node: Node) {
        let Some(parent) = self.stack.last_mut() else {
            self.root = Some(node);
            return;
        };
        match parent.kind {
            NodeKind::Seq(ref mut items) => items.push(node),
            NodeKind::Map(ref mut entries) => {
                let key = self.keys.last_mut().unwrap();
                match key.take() {
                    Some(key) => entries.push((key, node)),
                    None => *key = Some(node),
                }
            }
            NodeKind::Atom(_) => unreachable!(),
        }
    }
}

/// Renders a node with the given (remaining) description.
struct Render<'a>(&'a Node, &'a [Desc]);

impl<'a> Render<'a> {
    /// Renders the node without description.
    fn plain(node: &'a Node) -> Render<'a> {
        Render(node, &node.desc)
    }
}

impl fmt::Debug for Render<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Render(node, desc) = *self;
        let Some((first, rest)) = desc.split_first() else {
            return render_undescribed(node, f);
        };
        match *first {
            Desc::Some => f.debug_tuple("Some").field(&Render(node, rest)).finish(),
            Desc::None => f.write_str("None"),
            Desc::Newtype(ref name) => f.debug_tuple(name).field(&Render(node, rest)).finish(),
            Desc::Structure(ref name) => render_struct(name, node, None, f),
            Desc::Tuple => render_tuple("", node, f),
            Desc::Set => match node.kind {
                NodeKind::Seq(ref items) => f
                    .debug_set()
                    .entries(items.iter().map(Render::plain))
                    .finish(),
                _ => render_undescribed(node, f),
            },
            Desc::Variant {
                ref name,
                kind,
                ref repr,
            } => render_variant(name, kind, repr, node, rest, f),
        }
    }
}

fn render_undescribed(node: &Node, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match node.kind {
        NodeKind::Atom(ref atom) => render_atom(atom, f),
        NodeKind::Map(ref entries) => f
            .debug_map()
            .entries(
                entries
                    .iter()
                    .map(|(k, v)| (Render::plain(k), Render::plain(v))),
            )
            .finish(),
        NodeKind::Seq(ref items) => f
            .debug_list()
            .entries(items.iter().map(Render::plain))
            .finish(),
    }
}

fn render_atom(atom: &Atom, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *atom {
        Atom::Null => f.write_str("()"),
        Atom::Bool(v) => fmt::Debug::fmt(&v, f),
        Atom::Str(ref v) => fmt::Debug::fmt(v, f),
        Atom::Bytes(ref v) => fmt::Debug::fmt(&v.data[..], f),
        Atom::Char(v) => fmt::Debug::fmt(&v, f),
        Atom::U64(v) => fmt::Debug::fmt(&v, f),
        Atom::I64(v) => fmt::Debug::fmt(&v, f),
        Atom::F64(v) => fmt::Debug::fmt(&v, f),
        Atom::Ext(ref v) => fmt::Debug::fmt(v, f),
        _ => f.write_str("?"),
    }
}

/// Renders a map as struct, optionally skipping a key (the tag).
fn render_struct(
    name: &str,
    node: &Node,
    skip: Option<&str>,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    let NodeKind::Map(ref entries) = node.kind else {
        return render_undescribed(node, f);
    };
    let mut s = f.debug_struct(name);
    for (key, value) in entries {
        match key.as_str() {
            Some(key) if Some(key) == skip => {}
            Some(key) => {
                s.field(key, &Render::plain(value));
            }
            None => {
                s.field("?", &Render::plain(value));
            }
        }
    }
    s.finish()
}

/// Renders a map as struct without the tag.
struct StructWithout<'a>(&'a str, &'a Node, &'a str);

impl fmt::Debug for StructWithout<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        render_struct(self.0, self.1, Some(self.2), f)
    }
}

/// Renders a sequence as tuple.
fn render_tuple(name: &str, node: &Node, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let NodeKind::Seq(ref items) = node.kind else {
        return render_undescribed(node, f);
    };
    let mut t = f.debug_tuple(name);
    for item in items {
        t.field(&Render::plain(item));
    }
    t.finish()
}

/// Renders the content of a variant.
fn render_content(
    name: &str,
    kind: VariantKind,
    content: &Node,
    rest: &[Desc],
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    match kind {
        VariantKind::Unit => f.write_str(name),
        VariantKind::Newtype => f.debug_tuple(name).field(&Render(content, rest)).finish(),
        VariantKind::Tuple => render_tuple(name, content, f),
        _ => render_struct(name, content, None, f),
    }
}

fn render_variant(
    name: &str,
    kind: VariantKind,
    repr: &Repr,
    node: &Node,
    rest: &[Desc],
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    if kind == VariantKind::Unit {
        return f.write_str(name);
    }
    match *repr {
        Repr::External => match node.kind {
            NodeKind::Map(ref entries) if entries.len() == 1 => {
                let content = &entries[0].1;
                render_content(name, kind, content, &content.desc, f)
            }
            _ => render_undescribed(node, f),
        },
        // the fields of the content are merged with the tag
        Repr::Internal { ref tag } => match (kind, rest) {
            (VariantKind::Newtype, [Desc::Structure(inner), ..]) => f
                .debug_tuple(name)
                .field(&StructWithout(inner, node, tag))
                .finish(),
            (VariantKind::Newtype | VariantKind::Struct, _) => {
                render_struct(name, node, Some(tag), f)
            }
            _ => render_undescribed(node, f),
        },
        Repr::Adjacent { ref content } => match node.get(content) {
            Some(content) => render_content(name, kind, content, &content.desc, f),
            None => render_undescribed(node, f),
        },
        // the content is serialized in place of the enum
        Repr::Untagged => render_content(name, kind, node, rest, f),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_format() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(true, vec![vec![&b"x"[..], b"yyy"], vec![b"zzzz\x00\x01"]]);
        m.insert(false, vec![]);
        assert_eq!(ToDebug::new(&m).to_string(), format!("{:?}", m));
    }

    #[test]
    fn test_debug_format_ext() {
        assert_eq!(
            ToDebug::new(&vec![u128::MAX, 1]).to_string(),
            format!("[{}, 1]", u128::MAX)
        );
    }

    #[test]
    fn test_std_types() {
        fn check<T: Serialize + fmt::Debug>(value: T) {
            assert_eq!(ToDebug::new(&value).to_string(), format!("{:?}", value));
            assert_eq!(
                format!("{:#?}", ToDebug::new(&value)),
                format!("{:#?}", value)
            );
        }
        check(42u32);
        check("hello");
        check('x');
        check(Some(Some(1i64)));
        check(None::<u32>);
        check((1, "two", Some(3.5)));
        check(vec![Some(1), None]);
        check(std::collections::BTreeSet::from([1, 2, 3]));
        check(std::collections::BTreeMap::from([("a", (1, 2))]));
        check(Box::new(Some(vec![true])));
        check(());
    }
}
