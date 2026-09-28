//! The writer of binary property lists (`bplist00`).
//!
//! Like Core Foundation the writer stores equal strings, numbers, dates,
//! data and UIDs only once (including the keys of dictionaries).
//! Containers are never shared.
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::common::timestamp_to_plist;
use crate::ser::{Node, Tree};

/// An object of the output.
#[derive(Clone, Copy)]
enum Object<'t> {
    Node(usize),
    /// A key of a dictionary.
    Key(&'t str),
}

/// The identity of objects that are stored once.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Unique<'t> {
    Bool(bool),
    Int(i128),
    Real(u64),
    Real32(u32),
    Str(&'t str),
    Data(&'t [u8]),
    Date(i64, u32),
    Uid(u64),
}

struct Writer<'t> {
    tree: &'t Tree,
    objects: Vec<Object<'t>>,
    unique: BTreeMap<Unique<'t>, usize>,
    /// The object indices of the children of containers (keys first for
    /// dictionaries), by node.
    children: BTreeMap<usize, Vec<usize>>,
}

/// Writes the tree as binary property list.
pub(crate) fn write(tree: &Tree) -> Vec<u8> {
    let mut writer = Writer {
        tree,
        objects: Vec::new(),
        unique: BTreeMap::new(),
        children: BTreeMap::new(),
    };
    writer.collect();
    writer.write()
}

impl<'t> Writer<'t> {
    /// Assigns the object indices.
    fn collect(&mut self) {
        let tree = self.tree;
        self.add(Object::Node(0));
        let mut pending = Vec::from([0usize]);
        while let Some(id) = pending.pop() {
            let mut children = Vec::new();
            match tree.nodes[id] {
                Node::Array(ref items) => {
                    for &item in items {
                        children.push(self.add(Object::Node(item)));
                        pending.push(item);
                    }
                }
                Node::Dict(ref entries) => {
                    for (key, _) in entries {
                        children.push(self.add(Object::Key(key)));
                    }
                    for &(_, value) in entries {
                        children.push(self.add(Object::Node(value)));
                        pending.push(value);
                    }
                }
                _ => continue,
            }
            self.children.insert(id, children);
        }
    }

    /// Adds an object and returns its index.  Objects that are stored once
    /// return the index of the first one.
    fn add(&mut self, object: Object<'t>) -> usize {
        let unique = match object {
            Object::Key(key) => Some(Unique::Str(key)),
            Object::Node(id) => match self.tree.nodes[id] {
                Node::Bool(value) => Some(Unique::Bool(value)),
                Node::Int(value) => Some(Unique::Int(value)),
                Node::Real(value) => Some(Unique::Real(value.to_bits())),
                Node::Real32(value) => Some(Unique::Real32(value.to_bits())),
                Node::Str(ref value) => Some(Unique::Str(value)),
                Node::Data(ref value) => Some(Unique::Data(value)),
                Node::Date(value) => Some(Unique::Date(value.seconds, value.nanosecond)),
                Node::Uid(value) => Some(Unique::Uid(value)),
                Node::Array(_) | Node::Dict(_) => None,
            },
        };
        let idx = self.objects.len();
        if let Some(unique) = unique {
            if let Some(&existing) = self.unique.get(&unique) {
                return existing;
            }
            self.unique.insert(unique, idx);
        }
        self.objects.push(object);
        idx
    }

    fn write(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        out.extend_from_slice(b"bplist00");
        let ref_size = int_size(self.objects.len() as u64 - 1);
        let mut offsets = Vec::with_capacity(self.objects.len());
        for &object in &self.objects {
            offsets.push(out.len() as u64);
            match object {
                Object::Key(key) => write_str(&mut out, key),
                Object::Node(id) => self.write_node(&mut out, id, ref_size),
            }
        }
        let table = out.len() as u64;
        let offset_size = int_size(table);
        for offset in offsets {
            out.extend_from_slice(&offset.to_be_bytes()[8 - offset_size..]);
        }
        // the trailer: unused, sort version, sizes, number of objects, top
        // object and offset of the table.
        out.extend_from_slice(&[0; 6]);
        out.push(offset_size as u8);
        out.push(ref_size as u8);
        out.extend_from_slice(&(self.objects.len() as u64).to_be_bytes());
        out.extend_from_slice(&0u64.to_be_bytes());
        out.extend_from_slice(&table.to_be_bytes());
        out
    }

    fn write_node(&self, out: &mut Vec<u8>, id: usize, ref_size: usize) {
        match self.tree.nodes[id] {
            Node::Bool(value) => out.push(if value { 0x09 } else { 0x08 }),
            Node::Int(value) => write_int(out, value),
            Node::Real(value) => {
                out.push(0x23);
                out.extend_from_slice(&value.to_be_bytes());
            }
            Node::Real32(value) => {
                out.push(0x22);
                out.extend_from_slice(&value.to_be_bytes());
            }
            Node::Str(ref value) => write_str(out, value),
            Node::Data(ref value) => {
                write_head(out, 0x40, value.len());
                out.extend_from_slice(value);
            }
            Node::Date(ref value) => {
                out.push(0x33);
                out.extend_from_slice(&timestamp_to_plist(value).to_be_bytes());
            }
            Node::Uid(value) => {
                let size = int_size(value);
                out.push(0x80 | (size as u8 - 1));
                out.extend_from_slice(&value.to_be_bytes()[8 - size..]);
            }
            Node::Array(ref items) => {
                write_head(out, 0xa0, items.len());
                self.write_refs(out, id, ref_size);
            }
            Node::Dict(ref entries) => {
                write_head(out, 0xd0, entries.len());
                self.write_refs(out, id, ref_size);
            }
        }
    }

    fn write_refs(&self, out: &mut Vec<u8>, id: usize, ref_size: usize) {
        for &child in &self.children[&id] {
            out.extend_from_slice(&(child as u64).to_be_bytes()[8 - ref_size..]);
        }
    }
}

/// Returns the number of bytes (1, 2, 4 or 8) needed for an unsigned
/// integer.
fn int_size(value: u64) -> usize {
    if value <= 0xff {
        1
    } else if value <= 0xffff {
        2
    } else if value <= 0xffff_ffff {
        4
    } else {
        8
    }
}

/// Writes an integer object.
///
/// Integers with up to four bytes are unsigned, eight bytes are signed.
/// Like Core Foundation, negative integers are always written with eight
/// bytes and integers that only fit into `u64` with sixteen.
fn write_int(out: &mut Vec<u8>, value: i128) {
    let size = if value < 0 {
        if value >= i128::from(i64::MIN) { 8 } else { 16 }
    } else if value <= i128::from(u32::MAX) {
        int_size(value as u64)
    } else if value <= i128::from(i64::MAX) {
        8
    } else {
        16
    };
    out.push(0x10 | size.trailing_zeros() as u8);
    out.extend_from_slice(&value.to_be_bytes()[16 - size..]);
}

/// Writes the marker of an object with a length.
fn write_head(out: &mut Vec<u8>, marker: u8, len: usize) {
    if len < 15 {
        out.push(marker | len as u8);
    } else {
        out.push(marker | 0xf);
        write_int(out, len as i128);
    }
}

/// Writes a string object: ASCII if possible, otherwise UTF-16.
fn write_str(out: &mut Vec<u8>, value: &str) {
    if value.is_ascii() {
        write_head(out, 0x50, value.len());
        out.extend_from_slice(value.as_bytes());
    } else {
        write_head(out, 0x60, value.encode_utf16().count());
        for unit in value.encode_utf16() {
            out.extend_from_slice(&unit.to_be_bytes());
        }
    }
}
