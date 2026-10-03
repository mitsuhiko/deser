//! Emits the events of the graph built by the [machine](crate::vm).
//!
//! The graph is walked from its root with an explicit stack.  What a node
//! is emitted as is its shape (see [`shape`]): containers are sequences
//! and maps, objects are emitted as their items, state or arguments.
//!
//! Values that are reached more than once are emitted at every place with
//! their id as event data.  Values that contain themselves are emitted
//! once, where they are reached again from within themselves a
//! [`Reference`] is emitted instead.
use alloc::borrow::Cow;
use alloc::vec;
use alloc::vec::Vec;
use core::slice;
use core::str;

use deser_core::de::DeserializeDriver;
use deser_core::ext::ExtValue;
use deser_core::{Atom, Bytes, ContainerShape, Error, ErrorKind, Event, Text};

use crate::types::{ClassData, Form, FormData, Global, Kind, KindData, Reference, SharedIdData};
use crate::vm::{BytesKind, Graph, Id, Node, Object};

/// What a node is emitted as.
#[derive(Clone, Copy)]
enum Shape<'g> {
    /// An atom: the node (which can be another node than the one emitted,
    /// for objects).
    Atom(Id),
    Seq(Option<Kind>, &'g [Id]),
    /// A map, the entries of the second slice follow the first (for the
    /// state of objects with slots).
    Map(&'g [(Id, Id)], &'g [(Id, Id)]),
}

impl<'g> Shape<'g> {
    fn len(&self) -> usize {
        match *self {
            Shape::Atom(_) => 0,
            Shape::Seq(_, items) => items.len(),
            Shape::Map(a, b) => a.len() + b.len(),
        }
    }

    fn for_each_child<F: FnMut(Id)>(&self, mut f: F) {
        match *self {
            Shape::Atom(_) => {}
            Shape::Seq(_, items) => items.iter().for_each(|&x| f(x)),
            Shape::Map(a, b) => a.iter().chain(b).for_each(|&(k, v)| {
                f(k);
                f(v)
            }),
        }
    }
}

/// Returns `true` if a node is reported as shared and can be referred to.
///
/// Atoms do not have an identity and neither do empty tuples and
/// frozensets (Python has only one of each).
fn has_identity(node: &Node<'_>) -> bool {
    match node {
        Node::List(_) | Node::Dict(_) | Node::Set(_) | Node::Object(_) => true,
        Node::Tuple(items) | Node::FrozenSet(items) => !items.is_empty(),
        _ => false,
    }
}

/// The shape of a node that is not an object.
fn plain_shape<'g>(graph: &'g Graph<'_>, id: Id) -> Shape<'g> {
    match graph.node(id) {
        Node::List(items) => Shape::Seq(None, items),
        Node::Tuple(items) => Shape::Seq(Some(Kind::Tuple), items),
        Node::Set(items) => Shape::Seq(Some(Kind::Set), items),
        Node::FrozenSet(items) => Shape::Seq(Some(Kind::FrozenSet), items),
        Node::Dict(entries) => Shape::Map(entries, &[]),
        _ => Shape::Atom(id),
    }
}

/// The shape of the state or argument that an object is emitted as.
///
/// An object is a tuple with the object, its class could not be emitted
/// otherwise.
fn inline_shape<'g>(graph: &'g Graph<'_>, id: &'g Id) -> Shape<'g> {
    match graph.node(*id) {
        Node::Object(_) => Shape::Seq(Some(Kind::Tuple), slice::from_ref(id)),
        _ => plain_shape(graph, *id),
    }
}

/// The shape of an object and how it's created from it.
///
/// Objects are emitted as their items if items were added to them (like
/// list and dict subclasses), else as their state (dicts and the state of
/// objects with slots are maps) and else as their arguments: no arguments
/// are an empty map (or the keyword arguments), one is the argument and
/// more are a tuple.
fn object_shape<'g>(graph: &'g Graph<'_>, object: &'g Object) -> (Shape<'g>, Form) {
    if !object.dict_items.is_empty() {
        return (Shape::Map(&object.dict_items, &[]), Form::Items);
    }
    if !object.list_items.is_empty() {
        return (Shape::Seq(None, &object.list_items), Form::Items);
    }
    if let Some(ref state) = object.state {
        let mut form = Form::State;
        let shape = match graph.node(*state) {
            Node::None => None,
            Node::Dict(entries) => Some(Shape::Map(entries, &[])),
            Node::Tuple(items)
                if items.len() == 2
                    && items
                        .iter()
                        .all(|&x| matches!(graph.node(x), Node::None | Node::Dict(_))) =>
            {
                let entries = |id: Id| match graph.node(id) {
                    Node::Dict(entries) => &entries[..],
                    _ => &[][..],
                };
                form = Form::Slots;
                Some(Shape::Map(entries(items[0]), entries(items[1])))
            }
            _ => Some(inline_shape(graph, state)),
        };
        if let Some(shape) = shape {
            return (shape, form);
        }
    }
    match object.args[..] {
        [] => (Shape::Map(&object.kwargs, &[]), Form::Arguments),
        // an object is a tuple of the arguments
        [ref arg] if matches!(graph.node(*arg), Node::Object(_)) => {
            (inline_shape(graph, arg), Form::Arguments)
        }
        [ref arg] => (inline_shape(graph, arg), Form::Argument),
        _ => (Shape::Seq(Some(Kind::Tuple), &object.args), Form::Arguments),
    }
}

/// Returns the shape of a node and the class and form for objects.
fn shape<'g>(graph: &'g Graph<'_>, id: Id) -> (Shape<'g>, Option<(Id, Form)>) {
    match graph.node(id) {
        Node::Object(object) => {
            let (shape, form) = object_shape(graph, object);
            (shape, Some((object.class, form)))
        }
        _ => (plain_shape(graph, id), None),
    }
}

/// Counts how often the nodes with identity are reached (up to 2).
fn count_references(graph: &Graph<'_>) -> Vec<u8> {
    let mut counts = vec![0u8; graph.nodes.len()];
    let mut seen = vec![false; graph.nodes.len()];
    if has_identity(graph.node(graph.root)) {
        counts[graph.root as usize] = 1;
    }
    let mut stack = vec![graph.root];
    while let Some(id) = stack.pop() {
        if seen[id as usize] {
            continue;
        }
        seen[id as usize] = true;
        shape(graph, id).0.for_each_child(|child| {
            if has_identity(graph.node(child)) {
                let count = &mut counts[child as usize];
                *count = count.saturating_add(1).min(2);
                if !seen[child as usize] {
                    stack.push(child);
                }
            }
        });
    }
    counts
}

/// An open sequence or map.
struct Frame<'g> {
    node: Id,
    is_map: bool,
    shape: Shape<'g>,
    /// The next item or entry.
    pos: usize,
    /// For maps: the value that comes next.
    value: Option<Id>,
    /// `true` if the node was emitted before.
    copy: bool,
}

struct Emitter<'g, 'i, 'a, 'd> {
    graph: &'g Graph<'i>,
    driver: &'a mut DeserializeDriver<'d, 'i>,
    counts: Vec<u8>,
    open: Vec<bool>,
    emitted: Vec<bool>,
    frames: Vec<Frame<'g>>,
    /// The number of events of values emitted again that are left.
    budget: usize,
}

/// Emits the events of a graph.
///
/// `max_shared_events` limits the number of events of values that are
/// emitted more than once.
pub(crate) fn emit<'i>(
    graph: &Graph<'i>,
    driver: &mut DeserializeDriver<'_, 'i>,
    max_shared_events: usize,
) -> Result<(), Error> {
    let mut emitter = Emitter {
        graph,
        driver,
        counts: count_references(graph),
        open: vec![false; graph.nodes.len()],
        emitted: vec![false; graph.nodes.len()],
        frames: Vec::new(),
        budget: max_shared_events,
    };
    emitter.value(graph.root, false)?;
    while let Some(frame) = emitter.frames.last_mut() {
        let copy = frame.copy;
        let next = if let Some(value) = frame.value.take() {
            Some(value)
        } else if frame.pos < frame.shape.len() {
            let pos = frame.pos;
            frame.pos += 1;
            match frame.shape {
                Shape::Seq(_, items) => Some(items[pos]),
                Shape::Map(a, b) => {
                    let (key, value) = if pos < a.len() {
                        a[pos]
                    } else {
                        b[pos - a.len()]
                    };
                    frame.value = Some(value);
                    Some(key)
                }
                Shape::Atom(_) => unreachable!(),
            }
        } else {
            None
        };
        match next {
            Some(child) => emitter.value(child, copy)?,
            None => {
                let frame = emitter.frames.pop().unwrap();
                emitter.open[frame.node as usize] = false;
                emitter.count(frame.copy)?;
                let (start, end) = graph.ranges[frame.node as usize];
                emitter.driver.state_mut().set_input_range(start, end);
                emitter.driver.emit(match frame.is_map {
                    true => Event::MapEnd,
                    false => Event::SeqEnd,
                })?;
            }
        }
    }
    Ok(())
}

impl<'g, 'i> Emitter<'g, 'i, '_, '_> {
    /// Counts an event of a value that was emitted before.
    fn count(&mut self, copy: bool) -> Result<(), Error> {
        if copy {
            if self.budget == 0 {
                return Err(Error::new(
                    ErrorKind::LimitExceeded,
                    "shared values are emitted too often",
                ));
            }
            self.budget -= 1;
        }
        Ok(())
    }

    /// Emits the first event of a value.
    fn value(&mut self, id: Id, copy: bool) -> Result<(), Error> {
        let graph = self.graph;
        let identity = has_identity(graph.node(id));
        let (start, end) = graph.ranges[id as usize];
        self.driver.state_mut().set_input_range(start, end);
        if identity && self.open[id as usize] {
            self.count(copy)?;
            return self
                .driver
                .emit(Atom::Ext(ExtValue::owned(Reference::new(id.into()))));
        }
        let copy = copy || identity && self.emitted[id as usize];
        self.emitted[id as usize] = true;
        self.count(copy)?;
        let (shape, class) = shape(graph, id);
        let state = self.driver.state_mut();
        if let Some((class, form)) = class
            && let Node::Global { module, name, .. } = graph.node(class)
        {
            state.event_mut::<ClassData>().0 = Some(Global::new(module, name));
            state.event_mut::<FormData>().0 = Some(form);
        }
        if self.counts[id as usize] > 1 {
            state.event_mut::<SharedIdData>().0 = Some(id.into());
        }
        let is_map = match shape {
            Shape::Atom(atom) => return self.atom(atom),
            Shape::Seq(kind, _) => {
                if kind.is_some() {
                    state.event_mut::<KindData>().0 = kind;
                }
                false
            }
            Shape::Map(..) => true,
        };
        let len = shape.len();
        let mut container = ContainerShape::with_len(len);
        // empty maps and sequences can be both
        container.set_ambiguous_empty(len == 0);
        self.frames.push(Frame {
            node: id,
            is_map,
            shape,
            pos: 0,
            value: None,
            copy,
        });
        if identity {
            self.open[id as usize] = true;
        }
        self.driver.emit(match is_map {
            true => Event::MapStart(container),
            false => Event::SeqStart(container),
        })
    }

    fn atom(&mut self, id: Id) -> Result<(), Error> {
        let driver = &mut *self.driver;
        match self.graph.node(id) {
            Node::None => driver.emit(Atom::Null),
            Node::Bool(value) => driver.emit(Atom::Bool(*value)),
            Node::Int(value) => driver.emit(match u64::try_from(*value) {
                Ok(value) => Atom::U64(value),
                Err(_) => Atom::I64(*value),
            }),
            Node::BigInt(value) => driver.emit(value.clone().into_atom()),
            Node::Float(value) => driver.emit(Atom::F64(*value)),
            Node::Str(text) => match *text {
                Cow::Borrowed(text) => {
                    driver.emit_borrowed(Event::Atom(Atom::Str(Text::borrowed(text))))
                }
                Cow::Owned(ref text) => driver.emit(Atom::Str(Text::borrowed(text.as_str()))),
            },
            Node::Bytes(bytes, kind) => {
                if *kind == BytesKind::ByteArray {
                    driver.state_mut().event_mut::<KindData>().0 = Some(Kind::ByteArray);
                }
                // the strings of Python 2 are text if they are UTF-8
                let as_text = *kind == BytesKind::Py2Str && str::from_utf8(bytes).is_ok();
                match *bytes {
                    Cow::Borrowed(bytes) => driver.emit_borrowed(Event::Atom(match as_text {
                        true => Atom::Str(Text::borrowed(str::from_utf8(bytes).unwrap())),
                        false => Atom::Bytes(Bytes::borrowed(bytes)),
                    })),
                    Cow::Owned(ref bytes) => driver.emit(match as_text {
                        true => Atom::Str(Text::borrowed(str::from_utf8(bytes).unwrap())),
                        false => Atom::Bytes(Bytes::borrowed(bytes)),
                    }),
                }
            }
            Node::Global { module, name, .. } => {
                driver.emit(Atom::Ext(ExtValue::owned(Global::new(module, name))))
            }
            // containers and objects have other shapes
            _ => unreachable!(),
        }
    }
}
