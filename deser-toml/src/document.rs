//! The in-memory representation of a TOML document.
//!
//! TOML tables can be defined out of order, so a document is parsed into
//! this representation before it's passed on as events.  Tables and arrays
//! are stored in flat arenas and referenced by index so that neither
//! building nor dropping a document recurses.
use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::{BuildHasher, BuildHasherDefault, Hasher, RandomState};

use deser::ext::Datetime;

/// Tables with more entries than this are indexed with a hash map.
const INDEX_THRESHOLD: usize = 16;

/// A range of bytes in the input.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Value<'a> {
    Str(Cow<'a, str>),
    Int(i64),
    /// An integer larger than `i64::MAX`.
    UInt(u64),
    Float(f64),
    /// A single precision float (only used when serializing).
    Float32(f32),
    /// A float that is written as is (only used when serializing).
    FloatText(Cow<'a, str>),
    Bool(bool),
    Datetime(Datetime),
    Table(usize),
    Array(usize),
}

#[derive(Debug)]
pub(crate) struct Item<'a> {
    pub value: Value<'a>,
    /// The span of the value.  For tables and arrays the span is stored
    /// with the container.
    pub span: Span,
}

#[derive(Debug)]
pub(crate) struct Entry<'a> {
    pub key: Cow<'a, str>,
    pub key_span: Span,
    pub item: Item<'a>,
}

/// How a table was created.  This determines what can be added to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TableKind {
    /// Created as a parent of a `[table]` header.  Can be defined by a
    /// header later.
    Implicit,
    /// Defined by a `[table]` or `[[table]]` header (or the root table).
    Header,
    /// Created by dotted keys in the given section.  Only dotted keys in the
    /// same section can add to it.
    Dotted(u32),
    /// An inline table.  Nothing can be added to it.
    Inline,
}

#[derive(Debug)]
pub(crate) struct Table<'a> {
    pub entries: Vec<Entry<'a>>,
    pub kind: TableKind,
    pub span: Span,
    index: Option<Box<KeyIndex>>,
}

/// An index of the keys of a table.
///
/// Keys are hashed with a random key (so that inputs cannot provoke
/// collisions) and the hash is mapped to the last entry with the hash.
/// Entries with the same hash are chained.  This does not need to copy the
/// keys.
#[derive(Debug)]
struct KeyIndex {
    state: RandomState,
    heads: HashMap<u64, usize, BuildHasherDefault<HashIsKey>>,
    /// For every entry the previous entry with the same hash.
    chain: Vec<usize>,
}

/// The end of a chain in a [`KeyIndex`].
const NO_ENTRY: usize = usize::MAX;

/// A hasher for keys that are hashes already.
#[derive(Debug, Default)]
struct HashIsKey(u64);

impl Hasher for HashIsKey {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _bytes: &[u8]) {
        unreachable!("only hashes are hashed")
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = value;
    }
}

impl KeyIndex {
    fn new(entries: &[Entry<'_>]) -> KeyIndex {
        let mut index = KeyIndex {
            state: RandomState::new(),
            heads: HashMap::with_capacity_and_hasher(entries.len() * 2, Default::default()),
            chain: Vec::with_capacity(entries.len() * 2),
        };
        for (idx, entry) in entries.iter().enumerate() {
            index.insert(&entry.key, idx);
        }
        index
    }

    fn insert(&mut self, key: &str, idx: usize) {
        debug_assert_eq!(idx, self.chain.len());
        let hash = self.state.hash_one(key);
        let prev = self.heads.insert(hash, idx).unwrap_or(NO_ENTRY);
        self.chain.push(prev);
    }

    fn find(&self, entries: &[Entry<'_>], key: &str) -> Option<usize> {
        let mut idx = *self.heads.get(&self.state.hash_one(key))?;
        while idx != NO_ENTRY {
            if entries[idx].key == key {
                return Some(idx);
            }
            idx = self.chain[idx];
        }
        None
    }
}

#[derive(Debug)]
pub(crate) struct Array<'a> {
    pub items: Vec<Item<'a>>,
    /// `true` for arrays of tables (`[[table]]`), these can be extended.
    pub of_tables: bool,
    pub span: Span,
}

/// A TOML document.  The root table has index 0.
#[derive(Debug, Default)]
pub(crate) struct Document<'a> {
    pub tables: Vec<Table<'a>>,
    pub arrays: Vec<Array<'a>>,
}

impl<'a> Document<'a> {
    pub fn new_table(&mut self, kind: TableKind, span: Span) -> usize {
        self.tables.push(Table {
            entries: Vec::new(),
            kind,
            span,
            index: None,
        });
        self.tables.len() - 1
    }

    pub fn new_array(&mut self, of_tables: bool, span: Span) -> usize {
        self.arrays.push(Array {
            items: Vec::new(),
            of_tables,
            span,
        });
        self.arrays.len() - 1
    }

    /// Looks up the entry for a key in a table.
    pub fn find(&self, table: usize, key: &str) -> Option<&Entry<'a>> {
        let table = &self.tables[table];
        match table.index {
            Some(ref index) => index
                .find(&table.entries, key)
                .map(|idx| &table.entries[idx]),
            None => table.entries.iter().find(|x| x.key == key),
        }
    }

    /// Adds an entry to a table.  The key must not exist yet.
    pub fn insert(&mut self, table: usize, entry: Entry<'a>) {
        let table = &mut self.tables[table];
        table.entries.push(entry);
        let len = table.entries.len();
        if let Some(ref mut index) = table.index {
            index.insert(&table.entries[len - 1].key, len - 1);
        } else if len > INDEX_THRESHOLD {
            table.index = Some(Box::new(KeyIndex::new(&table.entries)));
        }
    }
}
