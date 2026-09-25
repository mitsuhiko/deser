//! The in-memory representation of a TOML document.
//!
//! TOML tables can be defined out of order, so a document is parsed into
//! this representation before it's passed on as events.  Tables and arrays
//! are stored in flat arenas and referenced by index so that neither
//! building nor dropping a document recurses.
use std::borrow::Cow;
use std::collections::HashMap;

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
    index: Option<HashMap<Cow<'a, str>, usize>>,
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
            Some(ref index) => index.get(key).map(|&idx| &table.entries[idx]),
            None => table.entries.iter().find(|x| x.key == key),
        }
    }

    /// Adds an entry to a table.  The key must not exist yet.
    pub fn insert(&mut self, table: usize, entry: Entry<'a>) {
        let table = &mut self.tables[table];
        table.entries.push(entry);
        let len = table.entries.len();
        if let Some(ref mut index) = table.index {
            index.insert(table.entries[len - 1].key.clone(), len - 1);
        } else if len > INDEX_THRESHOLD {
            table.index = Some(
                table
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(idx, entry)| (entry.key.clone(), idx))
                    .collect(),
            );
        }
    }
}
