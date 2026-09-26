//! Operations on trees of values.
//!
//! All operations are implemented with explicit stacks rather than
//! recursion so that deeply nested values do not overflow the stack.
use std::fmt;
use std::hash::{Hash, Hasher};
use std::slice;

use deser::Atom;

use crate::map::{Entries, Map};
use crate::seq::Seq;
use crate::value::{Kind, Meta, Value};

/// Drops values without recursion.
pub(crate) fn drop_values(mut stack: Vec<Value>) {
    while let Some(mut value) = stack.pop() {
        match &mut value.kind {
            Kind::Seq(seq) => stack.append(&mut seq.items),
            Kind::Map(map) => {
                for (key, value) in map.inner.entries.drain(..) {
                    stack.push(key);
                    stack.push(value);
                }
            }
            _ => {}
        }
        // the value is dropped here, its containers are empty.
    }
}

/// Drops the entries of a map without recursion.
pub(crate) fn drop_entries(entries: Entries) {
    let mut stack = Vec::with_capacity(entries.len() * 2);
    for (key, value) in entries {
        stack.push(key);
        stack.push(value);
    }
    drop_values(stack);
}

/// Clones a value that has no children.
fn clone_leaf(kind: &Kind) -> Kind {
    match kind {
        Kind::Null => Kind::Null,
        Kind::Bool(value) => Kind::Bool(*value),
        Kind::U64(value) => Kind::U64(*value),
        Kind::I64(value) => Kind::I64(*value),
        Kind::F32(value) => Kind::F32(*value),
        Kind::F64(value) => Kind::F64(*value),
        Kind::Char(value) => Kind::Char(*value),
        Kind::Str(value) => Kind::Str(value.clone()),
        Kind::Lexical(value) => Kind::Lexical(value.clone()),
        Kind::Bytes(value) => Kind::Bytes(value.clone()),
        Kind::Ext(value) => Kind::Ext(value.clone()),
        Kind::Seq(seq) => {
            debug_assert!(seq.is_empty());
            Kind::Seq(seq.empty_like(0))
        }
        Kind::Map(map) => {
            debug_assert!(map.is_empty());
            Kind::Map(Map::new().with_order(map.order()))
        }
    }
}

/// A container that is being cloned.
enum CloneFrame<'a> {
    Seq {
        iter: slice::Iter<'a, Value>,
        out: Seq,
        meta: Option<Box<Meta>>,
    },
    Map {
        iter: indexmap::map::Iter<'a, Value, Value>,
        // the value of the entry whose key was produced last
        pending: Option<&'a Value>,
        // the cloned key of the entry that is being cloned
        key: Option<Value>,
        out: Map,
        meta: Option<Box<Meta>>,
    },
}

impl<'a> CloneFrame<'a> {
    fn seq(seq: &'a Seq, meta: Option<Box<Meta>>) -> CloneFrame<'a> {
        CloneFrame::Seq {
            iter: seq.items.iter(),
            out: seq.empty_like(seq.len()),
            meta,
        }
    }

    fn map(map: &'a Map, meta: Option<Box<Meta>>) -> CloneFrame<'a> {
        CloneFrame::Map {
            iter: map.inner.entries.iter(),
            pending: None,
            key: None,
            out: Map::with_capacity(map.len()).with_order(map.order()),
            meta,
        }
    }

    /// Returns the next child to clone.  For maps these are the keys and
    /// values alternating.
    fn next_child(&mut self) -> Option<&'a Value> {
        match self {
            CloneFrame::Seq { iter, .. } => iter.next(),
            CloneFrame::Map { iter, pending, .. } => pending.take().or_else(|| {
                let (key, value) = iter.next()?;
                *pending = Some(value);
                Some(key)
            }),
        }
    }

    /// Adds a cloned child.
    fn push(&mut self, value: Value) {
        match self {
            CloneFrame::Seq { out, .. } => out.items.push(value),
            CloneFrame::Map { out, key, .. } => match key.take() {
                Some(key) => {
                    out.inner.entries.insert(key, value);
                }
                None => *key = Some(value),
            },
        }
    }

    fn finish(self) -> (Kind, Option<Box<Meta>>) {
        match self {
            CloneFrame::Seq { out, meta, .. } => (Kind::Seq(out), meta),
            CloneFrame::Map { out, meta, .. } => (Kind::Map(out), meta),
        }
    }
}

fn clone_tree(root: CloneFrame<'_>) -> Kind {
    let mut stack = vec![root];
    loop {
        let child = stack.last_mut().expect("empty stack").next_child();
        match child {
            Some(child) => match child.kind {
                Kind::Seq(ref seq) if !seq.is_empty() => {
                    stack.push(CloneFrame::seq(seq, child.meta.clone()));
                }
                Kind::Map(ref map) if !map.is_empty() => {
                    stack.push(CloneFrame::map(map, child.meta.clone()));
                }
                ref leaf => {
                    let value = Value {
                        kind: clone_leaf(leaf),
                        meta: child.meta.clone(),
                    };
                    stack.last_mut().expect("empty stack").push(value);
                }
            },
            None => {
                let (kind, meta) = stack.pop().expect("empty stack").finish();
                match stack.last_mut() {
                    Some(parent) => parent.push(Value { kind, meta }),
                    None => return kind,
                }
            }
        }
    }
}

pub(crate) fn clone_kind(kind: &Kind) -> Kind {
    match kind {
        Kind::Seq(seq) => clone_seq(seq),
        Kind::Map(map) => clone_map(map),
        leaf => clone_leaf(leaf),
    }
}

pub(crate) fn clone_seq(seq: &Seq) -> Kind {
    if seq.is_empty() {
        return Kind::Seq(seq.empty_like(0));
    }
    clone_tree(CloneFrame::seq(seq, None))
}

pub(crate) fn clone_map(map: &Map) -> Kind {
    if map.is_empty() {
        return Kind::Map(Map::new().with_order(map.order()));
    }
    clone_tree(CloneFrame::map(map, None))
}

/// Compares values that have no children, or containers of different
/// kinds.
fn eq_leaf(a: &Kind, b: &Kind) -> bool {
    match (a, b) {
        (Kind::Null, Kind::Null) => true,
        (Kind::Bool(a), Kind::Bool(b)) => a == b,
        (Kind::U64(a), Kind::U64(b)) => a == b,
        (Kind::I64(a), Kind::I64(b)) => a == b,
        (Kind::U64(a), Kind::I64(b)) | (Kind::I64(b), Kind::U64(a)) => {
            i128::from(*a) == i128::from(*b)
        }
        (Kind::F64(a), Kind::F64(b)) => a.to_bits() == b.to_bits(),
        (Kind::F32(a), Kind::F32(b)) => a.to_bits() == b.to_bits(),
        (Kind::F32(a), Kind::F64(b)) | (Kind::F64(b), Kind::F32(a)) => {
            f64::from(*a).to_bits() == b.to_bits()
        }
        (Kind::Char(a), Kind::Char(b)) => a == b,
        (Kind::Str(a) | Kind::Lexical(a), Kind::Str(b) | Kind::Lexical(b)) => a == b,
        (Kind::Bytes(a), Kind::Bytes(b)) => a.data() == b.data(),
        (Kind::Ext(a), Kind::Ext(b)) => a == b,
        (Kind::Seq(a), Kind::Seq(b)) => a.is_empty() && b.is_empty(),
        (Kind::Map(a), Kind::Map(b)) => a.is_empty() && b.is_empty(),
        _ => false,
    }
}

type EqStack<'a> = Vec<(&'a Kind, &'a Kind)>;

fn push_seq<'a>(a: &'a Seq, b: &'a Seq, stack: &mut EqStack<'a>) -> bool {
    if a.len() != b.len() {
        return false;
    }
    stack.extend(a.iter().zip(b.iter()).map(|(a, b)| (&a.kind, &b.kind)));
    true
}

/// Returns `true` for keys which are compared in order.
fn is_ordered_key(key: &Value) -> bool {
    matches!(key.kind, Kind::Seq(_) | Kind::Map(_))
}

/// Compares the entries of maps.
///
/// Entries are looked up by their keys, except for entries with maps or
/// sequences as keys which are compared in order.  Looking up such keys
/// would compare them while comparing the maps they are in (which can
/// again contain such keys), which cannot be done without recursion.
fn push_map<'a>(a: &'a Map, b: &'a Map, stack: &mut EqStack<'a>) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut ordered = b
        .inner
        .entries
        .iter()
        .filter(|(key, _)| is_ordered_key(key));
    for (key, value) in a.inner.entries.iter() {
        if is_ordered_key(key) {
            match ordered.next() {
                Some((other_key, other_value)) => {
                    stack.push((&key.kind, &other_key.kind));
                    stack.push((&value.kind, &other_value.kind));
                }
                None => return false,
            }
        } else {
            match b.inner.entries.get(key) {
                Some(other) => stack.push((&value.kind, &other.kind)),
                None => return false,
            }
        }
    }
    ordered.next().is_none()
}

fn eq_stack(mut stack: EqStack<'_>) -> bool {
    while let Some((a, b)) = stack.pop() {
        let rv = match (a, b) {
            (Kind::Seq(a), Kind::Seq(b)) => push_seq(a, b, &mut stack),
            (Kind::Map(a), Kind::Map(b)) => push_map(a, b, &mut stack),
            _ => eq_leaf(a, b),
        };
        if !rv {
            return false;
        }
    }
    true
}

pub(crate) fn eq_kind(a: &Kind, b: &Kind) -> bool {
    eq_stack(vec![(a, b)])
}

pub(crate) fn eq_seq(a: &Seq, b: &Seq) -> bool {
    let mut stack = Vec::new();
    push_seq(a, b, &mut stack) && eq_stack(stack)
}

pub(crate) fn eq_map(a: &Map, b: &Map) -> bool {
    let mut stack = Vec::new();
    push_map(a, b, &mut stack) && eq_stack(stack)
}

// The tags of the kinds when hashed.  Integers are hashed by their value
// so that `U64` and `I64` of the same value hash the same, floats as `f64`
// so that `F32` and `F64` of the same value hash the same.
const TAG_NULL: u8 = 0;
const TAG_BOOL: u8 = 1;
const TAG_INT: u8 = 2;
const TAG_F64: u8 = 3;
const TAG_CHAR: u8 = 4;
const TAG_STR: u8 = 5;
const TAG_BYTES: u8 = 6;
const TAG_EXT: u8 = 7;
const TAG_SEQ: u8 = 8;
const TAG_MAP: u8 = 9;

pub(crate) fn hash_str<H: Hasher>(value: &str, state: &mut H) {
    state.write_u8(TAG_STR);
    value.hash(state);
}

pub(crate) fn hash_int<H: Hasher>(value: i128, state: &mut H) {
    state.write_u8(TAG_INT);
    state.write_i128(value);
}

pub(crate) fn hash_bool<H: Hasher>(value: bool, state: &mut H) {
    state.write_u8(TAG_BOOL);
    value.hash(state);
}

fn hash_f64<H: Hasher>(value: f64, state: &mut H) {
    state.write_u8(TAG_F64);
    value.to_bits().hash(state);
}

pub(crate) fn hash_char<H: Hasher>(value: char, state: &mut H) {
    state.write_u8(TAG_CHAR);
    value.hash(state);
}

/// Hashes the fallback of an extension value.
///
/// Extension values are equal only if they are of the same type and equal
/// values have the same fallback, which makes this consistent with the
/// comparison.
fn hash_fallback<H: Hasher>(atom: &Atom<'_>, state: &mut H) {
    match atom {
        Atom::Null => state.write_u8(TAG_NULL),
        Atom::Bool(value) => hash_bool(*value, state),
        Atom::Str(value) | Atom::Lexical(value) => hash_str(value, state),
        Atom::Bytes(value) => {
            state.write_u8(TAG_BYTES);
            value.data().hash(state);
        }
        Atom::Char(value) => hash_char(*value, state),
        Atom::U64(value) => hash_int(i128::from(*value), state),
        Atom::I64(value) => hash_int(i128::from(*value), state),
        Atom::F64(value) => hash_f64(*value, state),
        Atom::F32(value) => hash_f64(f64::from(*value), state),
        _ => {}
    }
}

/// Hashes a value, pushes the values of sequences to the stack.
///
/// Maps only hash their length as their entries are unordered.  They are
/// rarely used as keys, so this is good enough.
fn hash_node<'a, H: Hasher>(
    kind: &'a Kind,
    stack: &mut Vec<slice::Iter<'a, Value>>,
    state: &mut H,
) {
    match kind {
        Kind::Null => state.write_u8(TAG_NULL),
        Kind::Bool(value) => hash_bool(*value, state),
        Kind::U64(value) => hash_int(i128::from(*value), state),
        Kind::I64(value) => hash_int(i128::from(*value), state),
        Kind::F64(value) => hash_f64(*value, state),
        Kind::F32(value) => hash_f64(f64::from(*value), state),
        Kind::Char(value) => hash_char(*value, state),
        Kind::Str(value) | Kind::Lexical(value) => hash_str(value, state),
        Kind::Bytes(value) => {
            state.write_u8(TAG_BYTES);
            value.data().hash(state);
        }
        Kind::Ext(value) => {
            state.write_u8(TAG_EXT);
            hash_fallback(&value.fallback(), state);
        }
        Kind::Seq(seq) => {
            state.write_u8(TAG_SEQ);
            state.write_usize(seq.len());
            stack.push(seq.items.iter());
        }
        Kind::Map(map) => {
            state.write_u8(TAG_MAP);
            state.write_usize(map.len());
        }
    }
}

fn hash_stack<H: Hasher>(mut stack: Vec<slice::Iter<'_, Value>>, state: &mut H) {
    while let Some(iter) = stack.last_mut() {
        match iter.next() {
            Some(value) => hash_node(&value.kind, &mut stack, state),
            None => {
                stack.pop();
            }
        }
    }
}

pub(crate) fn hash_kind<H: Hasher>(kind: &Kind, state: &mut H) {
    let mut stack = Vec::new();
    hash_node(kind, &mut stack, state);
    hash_stack(stack, state);
}

pub(crate) fn hash_seq<H: Hasher>(seq: &Seq, state: &mut H) {
    state.write_u8(TAG_SEQ);
    state.write_usize(seq.len());
    hash_stack(vec![seq.items.iter()], state);
}

pub(crate) fn hash_map<H: Hasher>(map: &Map, state: &mut H) {
    state.write_u8(TAG_MAP);
    state.write_usize(map.len());
}

/// Formats a value that has no children.
fn fmt_leaf(kind: &Kind, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match kind {
        Kind::Null => f.write_str("null"),
        Kind::Bool(value) => write!(f, "{}", value),
        Kind::U64(value) => write!(f, "{}", value),
        Kind::I64(value) => write!(f, "{}", value),
        Kind::F32(value) => write!(f, "{:?}", value),
        Kind::F64(value) => write!(f, "{:?}", value),
        Kind::Char(value) => write!(f, "{:?}", value),
        Kind::Str(value) | Kind::Lexical(value) => write!(f, "{:?}", value),
        Kind::Bytes(value) => write!(f, "b\"{}\"", value.data().escape_ascii()),
        Kind::Ext(value) => write!(f, "{:?}", value),
        Kind::Seq(_) => f.write_str("[]"),
        Kind::Map(_) => f.write_str("{}"),
    }
}

/// A container that is being formatted.
enum FmtFrame<'a> {
    /// The flag is `true` before the first value.
    Seq(slice::Iter<'a, Value>, bool),
    /// Holds the value of the entry whose key was formatted last.
    Map(
        indexmap::map::Iter<'a, Value, Value>,
        Option<&'a Value>,
        bool,
    ),
}

fn fmt_indent(f: &mut fmt::Formatter<'_>, depth: usize) -> fmt::Result {
    for _ in 0..depth {
        f.write_str("    ")?;
    }
    Ok(())
}

fn fmt_separator(
    f: &mut fmt::Formatter<'_>,
    pretty: bool,
    depth: usize,
    first: &mut bool,
) -> fmt::Result {
    if pretty {
        f.write_str(if *first { "\n" } else { ",\n" })?;
        fmt_indent(f, depth)?;
    } else if !*first {
        f.write_str(", ")?;
    }
    *first = false;
    Ok(())
}

fn fmt_close(f: &mut fmt::Formatter<'_>, pretty: bool, depth: usize, s: &str) -> fmt::Result {
    if pretty {
        f.write_str(",\n")?;
        fmt_indent(f, depth - 1)?;
    }
    f.write_str(s)
}

/// Formats a value like the `Debug` output of the standard containers.
///
/// Maps are formatted as `{key: value}`, sequences as `[value]`.
pub(crate) fn fmt_kind(kind: &Kind, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match kind {
        Kind::Seq(seq) => fmt_seq(seq, f),
        Kind::Map(map) => fmt_map(map, f),
        leaf => fmt_leaf(leaf, f),
    }
}

pub(crate) fn fmt_seq(seq: &Seq, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if seq.is_empty() {
        return f.write_str("[]");
    }
    let pretty = f.alternate();
    fmt_container(FmtFrame::Seq(seq.items.iter(), true), "[", pretty, f)
}

pub(crate) fn fmt_map(map: &Map, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if map.is_empty() {
        return f.write_str("{}");
    }
    let pretty = f.alternate();
    fmt_container(
        FmtFrame::Map(map.inner.entries.iter(), None, true),
        "{",
        pretty,
        f,
    )
}

fn fmt_container(
    frame: FmtFrame<'_>,
    open: &str,
    pretty: bool,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    f.write_str(open)?;
    let mut stack = vec![frame];
    let mut next: Option<&Kind> = None;
    loop {
        if let Some(kind) = next.take() {
            match kind {
                Kind::Seq(seq) if !seq.is_empty() => {
                    f.write_str("[")?;
                    stack.push(FmtFrame::Seq(seq.items.iter(), true));
                }
                Kind::Map(map) if !map.is_empty() => {
                    f.write_str("{")?;
                    stack.push(FmtFrame::Map(map.inner.entries.iter(), None, true));
                }
                leaf => fmt_leaf(leaf, f)?,
            }
        }
        let depth = stack.len();
        let Some(frame) = stack.last_mut() else {
            return Ok(());
        };
        match frame {
            FmtFrame::Seq(iter, first) => match iter.next() {
                Some(value) => {
                    fmt_separator(f, pretty, depth, first)?;
                    next = Some(&value.kind);
                }
                None => {
                    fmt_close(f, pretty, depth, "]")?;
                    stack.pop();
                }
            },
            FmtFrame::Map(iter, pending, first) => {
                if let Some(value) = pending.take() {
                    f.write_str(": ")?;
                    next = Some(&value.kind);
                } else {
                    match iter.next() {
                        Some((key, value)) => {
                            fmt_separator(f, pretty, depth, first)?;
                            *pending = Some(value);
                            next = Some(&key.kind);
                        }
                        None => {
                            fmt_close(f, pretty, depth, "}")?;
                            stack.pop();
                        }
                    }
                }
            }
        }
    }
}
