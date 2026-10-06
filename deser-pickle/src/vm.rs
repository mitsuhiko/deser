//! The pickle machine.
//!
//! A pickle is a program for a stack machine that builds the value.  The
//! machine here runs it like CPython's unpickler (`_pickle.c`), but instead
//! of Python objects it builds a graph of [`Node`]s.  Globals are never
//! imported and nothing is called: calling a class records an object with
//! its arguments (see [`Object`]), only a few globals that stand for
//! builtin types are understood (see [`Known`]).
//!
//! Containers are filled after they were created and can be reached from
//! anywhere through the memo, so the graph can be shared and cyclic.  The
//! graph is complete before the first event is emitted (see
//! [`emit`](crate::emit)).
use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use core::str;

use deser_core::ext::BigInt;
use deser_core::{Error, ErrorKind};

use crate::compat;
use crate::text::{self, Int};

/// The index of a node in the graph.
pub(crate) type Id = u32;

/// The highest protocol that is understood.
pub(crate) const HIGHEST_PROTOCOL: u8 = 5;

/// What bytes are in Python.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BytesKind {
    Bytes,
    ByteArray,
    /// A string of Python 2 (`STRING`, `BINSTRING` and `SHORT_BINSTRING`).
    Py2Str,
}

/// The globals that stand for builtin types and are understood.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Known {
    Set,
    FrozenSet,
    ByteArray,
    Bytes,
    /// `_codecs.encode` (protocols 0 to 2 write bytes with it)
    Encode,
    /// `copyreg._reconstructor` (protocols 0 and 1 write objects with it)
    Reconstructor,
}

impl Known {
    fn of(module: &str, name: &str) -> Option<Known> {
        Some(match (module, name) {
            ("builtins" | "__builtin__", "set") => Known::Set,
            ("builtins" | "__builtin__", "frozenset") => Known::FrozenSet,
            ("builtins" | "__builtin__", "bytearray") => Known::ByteArray,
            ("builtins" | "__builtin__", "bytes") => Known::Bytes,
            ("_codecs", "encode") => Known::Encode,
            ("copyreg" | "copy_reg", "_reconstructor") => Known::Reconstructor,
            _ => return None,
        })
    }
}

/// An instance of a class that is not understood.
#[derive(Debug, Default)]
pub(crate) struct Object {
    /// The class (a [`Node::Global`]).
    pub(crate) class: Id,
    pub(crate) args: Vec<Id>,
    pub(crate) kwargs: Vec<(Id, Id)>,
    /// The state set by `BUILD`.
    pub(crate) state: Option<Id>,
    /// The items added with `APPEND`, `APPENDS` and `ADDITEMS`.
    pub(crate) list_items: Vec<Id>,
    /// The items set with `SETITEM` and `SETITEMS`.
    pub(crate) dict_items: Vec<(Id, Id)>,
}

/// A node of the graph.
#[derive(Debug)]
pub(crate) enum Node<'i> {
    None,
    Bool(bool),
    Int(i64),
    BigInt(BigInt),
    Float(f64),
    Str(Cow<'i, str>),
    Bytes(Cow<'i, [u8]>, BytesKind),
    List(Vec<Id>),
    Tuple(Vec<Id>),
    /// The entries of a dict.  Repeated keys are not merged.
    Dict(Vec<(Id, Id)>),
    /// The items of a set.  Repeated items are not merged.
    Set(Vec<Id>),
    FrozenSet(Vec<Id>),
    Global {
        module: Cow<'i, str>,
        name: Cow<'i, str>,
        known: Option<Known>,
    },
    Object(Object),
}

/// The value of a pickle: a graph of nodes.
pub(crate) struct Graph<'i> {
    pub(crate) nodes: Vec<Node<'i>>,
    /// The byte range of the opcode that created a node.
    pub(crate) ranges: Vec<(usize, usize)>,
    pub(crate) root: Id,
}

impl<'i> Graph<'i> {
    pub(crate) fn node(&self, id: Id) -> &Node<'i> {
        &self.nodes[id as usize]
    }
}

#[cold]
pub(crate) fn syntax_error(offset: usize, msg: &str) -> Error {
    Error::with_offset(
        ErrorKind::Syntax,
        format!("invalid pickle: {}", msg),
        offset,
    )
}

#[cold]
fn eof_error(offset: usize) -> Error {
    Error::with_offset(ErrorKind::EndOfFile, "unexpected end of input", offset)
}

#[cold]
fn unsupported(offset: usize, msg: &str) -> Error {
    Error::with_offset(
        ErrorKind::UnsupportedType,
        format!("unsupported pickle: {}", msg),
        offset,
    )
}

/// The memo: values stored by index for later use.
#[derive(Default)]
struct Memo {
    dense: Vec<Option<Id>>,
    sparse: BTreeMap<u64, Id>,
    /// The number of entries (the index `MEMOIZE` uses).
    len: u64,
}

impl Memo {
    fn get(&self, idx: u64) -> Option<Id> {
        match usize::try_from(idx)
            .ok()
            .and_then(|idx| self.dense.get(idx))
        {
            Some(&value) => value,
            None => self.sparse.get(&idx).copied(),
        }
    }

    fn put(&mut self, idx: u64, value: Id) {
        let old = match usize::try_from(idx) {
            Ok(i) if i < self.dense.len() => self.dense[i].replace(value),
            // indexes are usually dense, huge ones are kept apart
            Ok(i) if i < 1024 || i <= self.dense.len() * 2 => {
                self.dense.resize(i + 1, None);
                self.dense[i].replace(value)
            }
            _ => self.sparse.insert(idx, value),
        };
        if old.is_none() {
            self.len += 1;
        }
    }
}

/// Runs a pickle.
pub(crate) struct Machine<'i> {
    input: &'i [u8],
    pos: usize,
    /// The start of the current opcode.
    op_start: usize,
    nodes: Vec<Node<'i>>,
    ranges: Vec<(usize, usize)>,
    stack: Vec<Id>,
    marks: Vec<usize>,
    memo: Memo,
    /// The protocol of the last `PROTO` opcode.
    proto: u8,
    /// The end of the last frame (`FRAME`).
    frame_end: usize,
}

impl<'i> Machine<'i> {
    pub(crate) fn new(input: &'i [u8], pos: usize) -> Machine<'i> {
        Machine {
            input,
            pos,
            op_start: pos,
            nodes: Vec::new(),
            ranges: Vec::new(),
            stack: Vec::new(),
            marks: Vec::new(),
            memo: Memo::default(),
            proto: 0,
            frame_end: 0,
        }
    }

    /// Runs the pickle up to its `STOP` and returns the value and the
    /// offset after the `STOP`.
    ///
    /// If the `STOP` is in a frame, the pickle ends with the frame (like
    /// when Python reads pickles from a file, where frames are read as a
    /// whole).
    pub(crate) fn run(mut self) -> Result<(Graph<'i>, usize), Error> {
        loop {
            self.op_start = self.pos;
            let Some(&op) = self.input.get(self.pos) else {
                return Err(eof_error(self.pos));
            };
            self.pos += 1;
            if op == b'.' {
                let root = self.pop()?;
                return Ok((
                    Graph {
                        nodes: self.nodes,
                        ranges: self.ranges,
                        root,
                    },
                    self.pos.max(self.frame_end),
                ));
            }
            self.op(op)?;
        }
    }

    // -- reading the input --------------------------------------------------

    fn take(&mut self, len: usize) -> Result<&'i [u8], Error> {
        match self.pos.checked_add(len) {
            Some(end) if end <= self.input.len() => {
                let bytes = &self.input[self.pos..end];
                self.pos = end;
                Ok(bytes)
            }
            _ => Err(eof_error(self.input.len())),
        }
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        Ok(self.take(N)?.try_into().unwrap())
    }

    /// Reads a counted length of `N` bytes (unsigned, little endian).
    fn take_len<const N: usize>(&mut self) -> Result<usize, Error> {
        let mut buf = [0u8; 8];
        buf[..N].copy_from_slice(&self.take_array::<N>()?);
        let len = u64::from_le_bytes(buf);
        if len > isize::MAX as u64 {
            return Err(syntax_error(self.op_start, "length out of range"));
        }
        // a length larger than the input cannot be read anyway
        usize::try_from(len).map_err(|_| eof_error(self.input.len()))
    }

    /// Reads a line including its newline.
    fn line(&mut self) -> Result<&'i [u8], Error> {
        let rest = &self.input[self.pos..];
        match rest.iter().position(|&c| c == b'\n') {
            Some(end) => {
                self.pos += end + 1;
                Ok(&rest[..end + 1])
            }
            None => Err(eof_error(self.input.len())),
        }
    }

    /// Reads a line of a text opcode which must not be empty.
    fn arg_line(&mut self) -> Result<&'i [u8], Error> {
        let line = self.line()?;
        if line.len() < 2 {
            return Err(syntax_error(self.op_start, "empty argument"));
        }
        Ok(line)
    }

    // -- the stack ----------------------------------------------------------

    fn fence(&self) -> usize {
        self.marks.last().copied().unwrap_or(0)
    }

    #[cold]
    fn underflow(&self) -> Error {
        syntax_error(self.op_start, "stack underflow")
    }

    fn push_node(&mut self, node: Node<'i>) -> Result<Id, Error> {
        if self.nodes.len() >= Id::MAX as usize {
            return Err(Error::with_offset(
                ErrorKind::LimitExceeded,
                "too many values",
                self.op_start,
            ));
        }
        let id = self.nodes.len() as Id;
        self.nodes.push(node);
        self.ranges.push((self.op_start, self.pos));
        Ok(id)
    }

    fn push(&mut self, node: Node<'i>) -> Result<(), Error> {
        let id = self.push_node(node)?;
        self.stack.push(id);
        Ok(())
    }

    fn pop(&mut self) -> Result<Id, Error> {
        if self.stack.len() <= self.fence() {
            return Err(self.underflow());
        }
        Ok(self.stack.pop().unwrap())
    }

    fn top(&self) -> Result<Id, Error> {
        if self.stack.len() <= self.fence() {
            return Err(self.underflow());
        }
        Ok(*self.stack.last().unwrap())
    }

    /// Pops the topmost mark and returns its position.
    fn marker(&mut self) -> Result<usize, Error> {
        self.marks
            .pop()
            .ok_or_else(|| syntax_error(self.op_start, "MARK not found"))
    }

    /// Pops the values from `start` on.
    fn pop_from(&mut self, start: usize) -> Result<Vec<Id>, Error> {
        if start < self.fence() || start > self.stack.len() {
            return Err(self.underflow());
        }
        Ok(self.stack.split_off(start))
    }

    fn node(&self, id: Id) -> &Node<'i> {
        &self.nodes[id as usize]
    }

    fn node_mut(&mut self, id: Id) -> &mut Node<'i> {
        &mut self.nodes[id as usize]
    }

    fn memo_put(&mut self, idx: u64) -> Result<(), Error> {
        let value = self.top()?;
        self.memo.put(idx, value);
        Ok(())
    }

    fn memo_get(&mut self, idx: u64) -> Result<(), Error> {
        match self.memo.get(idx) {
            Some(id) => {
                self.stack.push(id);
                Ok(())
            }
            None => Err(syntax_error(self.op_start, "memo value not found")),
        }
    }

    // -- opcodes ------------------------------------------------------------

    fn op(&mut self, op: u8) -> Result<(), Error> {
        match op {
            // constants and numbers
            b'N' => self.push(Node::None),
            0x88 => self.push(Node::Bool(true)),
            0x89 => self.push(Node::Bool(false)),
            b'I' => {
                let line = self.arg_line()?;
                match text::load_int(line) {
                    Some(Ok(value)) => self.push_int(value),
                    Some(Err(value)) => self.push(Node::Bool(value)),
                    None => Err(syntax_error(self.op_start, "invalid integer")),
                }
            }
            b'J' => {
                let value = i32::from_le_bytes(self.take_array()?);
                self.push(Node::Int(value.into()))
            }
            b'K' => {
                let [value] = self.take_array()?;
                self.push(Node::Int(value.into()))
            }
            b'M' => {
                let value = u16::from_le_bytes(self.take_array()?);
                self.push(Node::Int(value.into()))
            }
            b'L' => {
                let line = self.arg_line()?;
                match text::load_long(line) {
                    Some(value) => self.push_int(value),
                    None => Err(syntax_error(self.op_start, "invalid integer")),
                }
            }
            0x8a => {
                let len = self.take_len::<1>()?;
                let bytes = self.take(len)?;
                self.push_int(text::int_from_le_bytes(bytes))
            }
            0x8b => {
                let len = i32::from_le_bytes(self.take_array()?);
                let len = usize::try_from(len)
                    .map_err(|_| syntax_error(self.op_start, "negative length"))?;
                let bytes = self.take(len)?;
                self.push_int(text::int_from_le_bytes(bytes))
            }
            b'F' => {
                let line = self.arg_line()?;
                match text::load_float(line) {
                    Some(value) => self.push(Node::Float(value)),
                    None => Err(syntax_error(self.op_start, "invalid float")),
                }
            }
            b'G' => {
                let value = f64::from_be_bytes(self.take_array()?);
                self.push(Node::Float(value))
            }

            // strings
            b'S' => {
                let line = self.line()?;
                let mut s = &line[..line.len() - 1];
                match s {
                    [first, .., last] if first == last && matches!(first, b'\'' | b'"') => {
                        s = &s[1..s.len() - 1];
                    }
                    _ => {
                        return Err(syntax_error(
                            self.op_start,
                            "the STRING argument must be quoted",
                        ));
                    }
                }
                let bytes = text::decode_escape(s)
                    .ok_or_else(|| syntax_error(self.op_start, "invalid escape in STRING"))?;
                self.push(Node::Bytes(Cow::Owned(bytes), BytesKind::Py2Str))
            }
            b'T' => {
                let len = i32::from_le_bytes(self.take_array()?);
                let len = usize::try_from(len)
                    .map_err(|_| syntax_error(self.op_start, "negative length"))?;
                let bytes = self.take(len)?;
                self.push(Node::Bytes(Cow::Borrowed(bytes), BytesKind::Py2Str))
            }
            b'U' => {
                let len = self.take_len::<1>()?;
                let bytes = self.take(len)?;
                self.push(Node::Bytes(Cow::Borrowed(bytes), BytesKind::Py2Str))
            }
            b'V' => {
                let line = self.line()?;
                let text = text::decode_raw_unicode_escape(&line[..line.len() - 1])
                    .ok_or_else(|| syntax_error(self.op_start, "invalid UNICODE argument"))?;
                self.push(Node::Str(Cow::Owned(text)))
            }
            b'X' => {
                let len = self.take_len::<4>()?;
                self.unicode(len)
            }
            0x8c => {
                let len = self.take_len::<1>()?;
                self.unicode(len)
            }
            0x8d => {
                let len = self.take_len::<8>()?;
                self.unicode(len)
            }
            b'B' => {
                let len = self.take_len::<4>()?;
                self.bytes(len, BytesKind::Bytes)
            }
            b'C' => {
                let len = self.take_len::<1>()?;
                self.bytes(len, BytesKind::Bytes)
            }
            0x8e => {
                let len = self.take_len::<8>()?;
                self.bytes(len, BytesKind::Bytes)
            }
            0x96 => {
                let len = self.take_len::<8>()?;
                self.bytes(len, BytesKind::ByteArray)
            }
            0x97 => Err(unsupported(self.op_start, "out-of-band buffers")),
            0x98 => {
                let top = self.top()?;
                let bytes = match self.node(top) {
                    Node::Bytes(bytes, BytesKind::ByteArray) => bytes.clone(),
                    Node::Bytes(..) => return Ok(()),
                    _ => return Err(syntax_error(self.op_start, "READONLY_BUFFER needs bytes")),
                };
                // the memo keeps the bytearray, only the stack has the view
                self.stack.pop();
                self.push(Node::Bytes(bytes, BytesKind::Bytes))
            }

            // containers
            b']' => self.push(Node::List(Vec::new())),
            b'}' => self.push(Node::Dict(Vec::new())),
            b')' => self.push(Node::Tuple(Vec::new())),
            0x8f => self.push(Node::Set(Vec::new())),
            b'l' => {
                let mark = self.marker()?;
                let items = self.pop_from(mark)?;
                self.push(Node::List(items))
            }
            b't' => {
                let mark = self.marker()?;
                let items = self.pop_from(mark)?;
                self.push(Node::Tuple(items))
            }
            0x85..=0x87 => {
                let len = usize::from(op - 0x84);
                let start = self
                    .stack
                    .len()
                    .checked_sub(len)
                    .ok_or_else(|| self.underflow())?;
                let items = self.pop_from(start)?;
                self.push(Node::Tuple(items))
            }
            b'd' => {
                let mark = self.marker()?;
                let items = self.pop_from(mark)?;
                if items.len() % 2 != 0 {
                    return Err(syntax_error(self.op_start, "odd number of items for DICT"));
                }
                let entries = items.chunks(2).map(|x| (x[0], x[1])).collect();
                self.push(Node::Dict(entries))
            }
            0x91 => {
                let mark = self.marker()?;
                let items = self.pop_from(mark)?;
                self.push(Node::FrozenSet(items))
            }
            b'a' => {
                if self.stack.len() <= self.fence() + 1 {
                    return Err(self.underflow());
                }
                self.append(self.stack.len() - 1)
            }
            b'e' => {
                let mark = self.marker()?;
                self.append(mark)
            }
            b's' => {
                let start = self
                    .stack
                    .len()
                    .checked_sub(2)
                    .ok_or_else(|| self.underflow())?;
                self.set_items(start)
            }
            b'u' => {
                let mark = self.marker()?;
                self.set_items(mark)
            }
            0x90 => {
                let mark = self.marker()?;
                self.add_items(mark)
            }

            // the stack and the memo
            b'(' => {
                self.marks.push(self.stack.len());
                Ok(())
            }
            b'0' => {
                if self.marks.last() == Some(&self.stack.len()) {
                    self.marks.pop();
                } else {
                    self.pop()?;
                }
                Ok(())
            }
            b'1' => {
                let mark = self.marker()?;
                self.pop_from(mark)?;
                Ok(())
            }
            b'2' => {
                let top = self.top()?;
                self.stack.push(top);
                Ok(())
            }
            b'p' => {
                let line = self.arg_line()?;
                self.top()?;
                match text::load_index(line).map(u64::try_from) {
                    Some(Ok(idx)) => self.memo_put(idx),
                    _ => Err(syntax_error(self.op_start, "invalid PUT argument")),
                }
            }
            b'q' => {
                let [idx] = self.take_array()?;
                self.memo_put(idx.into())
            }
            b'r' => {
                let idx = u32::from_le_bytes(self.take_array()?);
                self.memo_put(idx.into())
            }
            0x94 => {
                let idx = self.memo.len;
                self.memo_put(idx)
            }
            b'g' => {
                let line = self.arg_line()?;
                match text::load_index(line).map(u64::try_from) {
                    Some(Ok(idx)) => self.memo_get(idx),
                    _ => Err(syntax_error(self.op_start, "invalid GET argument")),
                }
            }
            b'h' => {
                let [idx] = self.take_array()?;
                self.memo_get(idx.into())
            }
            b'j' => {
                let idx = u32::from_le_bytes(self.take_array()?);
                self.memo_get(idx.into())
            }

            // globals and objects
            b'c' => {
                let module = self.global_name(false)?;
                let name = self.global_name(false)?;
                self.push_global(module, name)
            }
            0x93 => {
                let name = self.pop()?;
                let module = self.pop()?;
                match (self.node(module), self.node(name)) {
                    (Node::Str(module), Node::Str(name)) => {
                        let (module, name) = (module.clone(), name.clone());
                        self.push_global(module, name)
                    }
                    _ => Err(syntax_error(self.op_start, "STACK_GLOBAL requires str")),
                }
            }
            b'R' => {
                let args = self.pop()?;
                let callable = self.pop()?;
                let Node::Tuple(args) = self.node(args) else {
                    return Err(syntax_error(
                        self.op_start,
                        "REDUCE arguments must be a tuple",
                    ));
                };
                let args = args.clone();
                let id = self.call(callable, args, Vec::new())?;
                self.stack.push(id);
                Ok(())
            }
            0x81 | 0x92 => {
                let kwargs = match op {
                    0x92 => Some(self.pop()?),
                    _ => None,
                };
                let args = self.pop()?;
                let class = self.pop()?;
                if !matches!(self.node(class), Node::Global { .. }) {
                    return Err(syntax_error(
                        self.op_start,
                        "NEWOBJ class argument must be a class",
                    ));
                }
                let Node::Tuple(args) = self.node(args) else {
                    return Err(syntax_error(
                        self.op_start,
                        "NEWOBJ arguments must be a tuple",
                    ));
                };
                let args = args.clone();
                let kwargs = match kwargs.map(|x| self.node(x)) {
                    None => Vec::new(),
                    Some(Node::Dict(entries)) => {
                        if !entries
                            .iter()
                            .all(|&(k, _)| matches!(self.node(k), Node::Str(_)))
                        {
                            return Err(syntax_error(self.op_start, "keywords must be strings"));
                        }
                        entries.clone()
                    }
                    Some(_) => {
                        return Err(syntax_error(
                            self.op_start,
                            "NEWOBJ_EX keywords must be a dict",
                        ));
                    }
                };
                let id = self.call(class, args, kwargs)?;
                self.stack.push(id);
                Ok(())
            }
            b'o' => {
                let mark = self.marker()?;
                if self.stack.len() <= mark {
                    return Err(self.underflow());
                }
                let args = self.pop_from(mark + 1)?;
                let class = self.pop()?;
                let id = self.call(class, args, Vec::new())?;
                self.stack.push(id);
                Ok(())
            }
            b'i' => {
                let mark = self.marker()?;
                let module = self.global_name(true)?;
                let name = self.global_name(true)?;
                let args = self.pop_from(mark)?;
                self.push_global(module, name)?;
                let class = self.stack.pop().unwrap();
                let id = self.call(class, args, Vec::new())?;
                self.stack.push(id);
                Ok(())
            }
            b'b' => self.build(),

            // framing and the protocol
            0x80 => {
                let [proto] = self.take_array()?;
                if proto > HIGHEST_PROTOCOL {
                    return Err(unsupported(self.op_start, "protocol"));
                }
                self.proto = proto;
                Ok(())
            }
            0x95 => {
                let len = u64::from_le_bytes(self.take_array()?);
                if len > (self.input.len() - self.pos) as u64 {
                    return Err(eof_error(self.input.len()));
                }
                self.frame_end = self.frame_end.max(self.pos + len as usize);
                Ok(())
            }

            // things that are not supported
            b'P' => {
                self.line()?;
                Err(unsupported(self.op_start, "persistent ids"))
            }
            b'Q' => {
                self.pop()?;
                Err(unsupported(self.op_start, "persistent ids"))
            }
            0x82 => {
                self.take(1)?;
                Err(unsupported(self.op_start, "extension codes"))
            }
            0x83 => {
                self.take(2)?;
                Err(unsupported(self.op_start, "extension codes"))
            }
            0x84 => {
                self.take(4)?;
                Err(unsupported(self.op_start, "extension codes"))
            }
            _ => Err(syntax_error(self.op_start, "invalid opcode")),
        }
    }

    fn push_int(&mut self, value: Int) -> Result<(), Error> {
        self.push(match value {
            Int::Small(value) => Node::Int(value),
            Int::Big(value) => Node::BigInt(value),
        })
    }

    fn unicode(&mut self, len: usize) -> Result<(), Error> {
        let bytes = self.take(len)?;
        let text = str::from_utf8(bytes)
            .map_err(|_| syntax_error(self.op_start, "invalid UTF-8 (or surrogates) in string"))?;
        self.push(Node::Str(Cow::Borrowed(text)))
    }

    fn bytes(&mut self, len: usize, kind: BytesKind) -> Result<(), Error> {
        let bytes = self.take(len)?;
        self.push(Node::Bytes(Cow::Borrowed(bytes), kind))
    }

    /// Reads the module or name of `GLOBAL` (UTF-8) or `INST` (ASCII).
    fn global_name(&mut self, ascii: bool) -> Result<Cow<'i, str>, Error> {
        let line = self.arg_line()?;
        let line = &line[..line.len() - 1];
        if ascii && !line.is_ascii() {
            return Err(syntax_error(self.op_start, "non-ASCII name"));
        }
        str::from_utf8(line)
            .map(Cow::Borrowed)
            .map_err(|_| syntax_error(self.op_start, "invalid UTF-8 in name"))
    }

    fn push_global(
        &mut self,
        mut module: Cow<'i, str>,
        mut name: Cow<'i, str>,
    ) -> Result<(), Error> {
        // like Python, the names of Python 2 are read as the ones of Python 3
        if self.proto < 3
            && let Some((new_module, new_name)) = compat::fix_import(&module, &name)
        {
            module = Cow::Borrowed(new_module);
            if let Some(new_name) = new_name {
                name = Cow::Borrowed(new_name);
            }
        }
        let known = Known::of(&module, &name);
        self.push(Node::Global {
            module,
            name,
            known,
        })
    }

    /// Calls a class (or function) with arguments.
    fn call(&mut self, callable: Id, args: Vec<Id>, kwargs: Vec<(Id, Id)>) -> Result<Id, Error> {
        let known = match self.node(callable) {
            Node::Global { known, .. } => *known,
            _ => return Err(syntax_error(self.op_start, "called value is not a class")),
        };
        let Some(known) = known else {
            let object = Object {
                class: callable,
                args,
                kwargs,
                ..Object::default()
            };
            return self.push_node(Node::Object(object));
        };
        if !kwargs.is_empty() {
            return Err(self.unsupported_args());
        }
        let node = match known {
            Known::Set | Known::FrozenSet => {
                let items = match args[..] {
                    [] => Vec::new(),
                    [arg] => match self.node(arg) {
                        Node::List(items)
                        | Node::Tuple(items)
                        | Node::Set(items)
                        | Node::FrozenSet(items) => items.clone(),
                        _ => return Err(self.unsupported_args()),
                    },
                    _ => return Err(self.unsupported_args()),
                };
                match known {
                    Known::Set => Node::Set(items),
                    _ => Node::FrozenSet(items),
                }
            }
            Known::Bytes | Known::ByteArray | Known::Encode => {
                let bytes = match (known, &args[..]) {
                    (Known::Bytes | Known::ByteArray, []) => Cow::Borrowed(&b""[..]),
                    (Known::Bytes | Known::ByteArray, &[arg]) => match self.node(arg) {
                        Node::Bytes(bytes, _) => bytes.clone(),
                        _ => return Err(self.unsupported_args()),
                    },
                    (Known::ByteArray | Known::Encode, &[text, encoding]) => {
                        match (self.node(text), self.node(encoding)) {
                            (Node::Str(text), Node::Str(encoding))
                                if encoding == "latin1" || encoding == "latin-1" =>
                            {
                                let bytes = text
                                    .chars()
                                    .map(|c| u8::try_from(u32::from(c)))
                                    .collect::<Result<Vec<u8>, _>>()
                                    .map_err(|_| {
                                        syntax_error(
                                            self.op_start,
                                            "character out of range for latin-1",
                                        )
                                    })?;
                                Cow::Owned(bytes)
                            }
                            _ => return Err(self.unsupported_args()),
                        }
                    }
                    _ => return Err(self.unsupported_args()),
                };
                Node::Bytes(
                    bytes,
                    match known {
                        Known::ByteArray => BytesKind::ByteArray,
                        _ => BytesKind::Bytes,
                    },
                )
            }
            Known::Reconstructor => {
                let &[class, base, state] = &args[..] else {
                    return Err(self.unsupported_args());
                };
                let base = match (self.node(class), self.node(base)) {
                    (Node::Global { known: None, .. }, Node::Global { module, name, .. }) => {
                        match &**module {
                            "builtins" | "__builtin__" => Some(&**name),
                            _ => None,
                        }
                    }
                    _ => return Err(self.unsupported_args()),
                };
                // `list.__init__` and `dict.__init__` add the state as items
                let (list_items, dict_items) = match (base, self.node(state)) {
                    (Some("object"), _) => (Vec::new(), Vec::new()),
                    (Some("list"), Node::List(items)) => (items.clone(), Vec::new()),
                    (Some("dict"), Node::Dict(entries)) => (Vec::new(), entries.clone()),
                    _ => return self.call(class, vec![state], Vec::new()),
                };
                let object = Object {
                    class,
                    list_items,
                    dict_items,
                    ..Object::default()
                };
                return self.push_node(Node::Object(object));
            }
        };
        self.push_node(node)
    }

    #[cold]
    fn unsupported_args(&self) -> Error {
        unsupported(self.op_start, "arguments of a builtin type")
    }

    /// `APPEND` and `APPENDS`: appends the values from `start` on to the
    /// value before them.
    fn append(&mut self, start: usize) -> Result<(), Error> {
        if start > self.stack.len() || start <= self.fence() {
            return Err(self.underflow());
        }
        if start == self.stack.len() {
            return Ok(());
        }
        let target = self.stack[start - 1];
        let items = self.stack.split_off(start);
        match self.node_mut(target) {
            Node::List(list) => list.extend(items),
            Node::Object(object) => object.list_items.extend(items),
            _ => return Err(syntax_error(self.op_start, "cannot append to this value")),
        }
        Ok(())
    }

    /// `SETITEM` and `SETITEMS`: sets the keys and values from `start` on
    /// in the value before them.
    fn set_items(&mut self, start: usize) -> Result<(), Error> {
        if start > self.stack.len() || start <= self.fence() {
            return Err(self.underflow());
        }
        if start == self.stack.len() {
            return Ok(());
        }
        if !(self.stack.len() - start).is_multiple_of(2) {
            return Err(syntax_error(
                self.op_start,
                "odd number of items for SETITEMS",
            ));
        }
        let target = self.stack[start - 1];
        let items = self.stack.split_off(start);
        for pair in items.chunks(2) {
            let (key, value) = (pair[0], pair[1]);
            let index = match self.node(key) {
                Node::Int(index) => Some(*index),
                Node::Bool(index) => Some(i64::from(*index)),
                _ => None,
            };
            match self.node_mut(target) {
                Node::Dict(entries) => entries.push((key, value)),
                Node::Object(object) => object.dict_items.push((key, value)),
                // lists support item assignment
                Node::List(list) => {
                    let len = list.len() as i64;
                    match index.map(|x| if x < 0 { x + len } else { x }) {
                        Some(index) if (0..len).contains(&index) => list[index as usize] = value,
                        _ => return Err(syntax_error(self.op_start, "list index out of range")),
                    }
                }
                _ => {
                    return Err(syntax_error(
                        self.op_start,
                        "cannot set items of this value",
                    ));
                }
            }
        }
        Ok(())
    }

    /// `ADDITEMS`: adds the values from `start` on to the set before them.
    fn add_items(&mut self, start: usize) -> Result<(), Error> {
        if start > self.stack.len() || start <= self.fence() {
            return Err(self.underflow());
        }
        if start == self.stack.len() {
            return Ok(());
        }
        let target = self.stack[start - 1];
        let items = self.stack.split_off(start);
        match self.node_mut(target) {
            Node::Set(set) => set.extend(items),
            Node::Object(object) => object.list_items.extend(items),
            _ => {
                return Err(syntax_error(
                    self.op_start,
                    "cannot add items to this value",
                ));
            }
        }
        Ok(())
    }

    /// `BUILD`: sets the state of an object.
    fn build(&mut self) -> Result<(), Error> {
        if self.stack.len() < self.fence() + 2 {
            return Err(self.underflow());
        }
        let state = self.stack.pop().unwrap();
        let target = *self.stack.last().unwrap();
        match self.node(target) {
            Node::Object(_) => {
                if let Node::Object(object) = self.node_mut(target) {
                    object.state = Some(state);
                }
                return Ok(());
            }
            // the classes we do not know have a `__setstate__` method
            Node::Global { known: None, .. } => {
                return Err(syntax_error(
                    self.op_start,
                    "cannot set the state of a class",
                ));
            }
            _ => {}
        }
        // values without `__setstate__` only accept no state
        let (state, slots) = match self.node(state) {
            Node::Tuple(items) if items.len() == 2 => (items[0], Some(items[1])),
            _ => (state, None),
        };
        let empty_slots = match slots.map(|x| self.node(x)) {
            None => true,
            Some(Node::Dict(entries)) => entries.is_empty(),
            Some(_) => false,
        };
        if !matches!(self.node(state), Node::None) || !empty_slots {
            return Err(syntax_error(
                self.op_start,
                "cannot set the state of this value",
            ));
        }
        Ok(())
    }
}

/// Finds the end of a pickle without running it.
///
/// Returns the offset after the `STOP` opcode (or the end of the frame
/// it's in, see [`Machine::run`]) or `None` if the input ends before.
/// This only reads the opcodes and their arguments, invalid pickles fail
/// when they are run.
pub(crate) fn find_end(input: &[u8], start: usize) -> Result<Option<usize>, Error> {
    let mut pos = start;
    let mut frame_end = 0;
    // returns the length of a counted argument
    let counted = |pos: usize, size: usize, signed: bool| -> Result<Option<(usize, u64)>, Error> {
        let Some(bytes) = input.get(pos..pos + size) else {
            return Ok(None);
        };
        let mut buf = [0u8; 8];
        buf[..size].copy_from_slice(bytes);
        let len = u64::from_le_bytes(buf);
        if signed && size == 4 && len & 0x8000_0000 != 0 {
            return Err(syntax_error(pos - 1, "negative length"));
        }
        Ok(Some((size, len)))
    };
    loop {
        let Some(&op) = input.get(pos) else {
            return Ok(None);
        };
        let op_start = pos;
        pos += 1;
        let skip = match op {
            b'.' if frame_end > input.len() => return Ok(None),
            b'.' => return Ok(Some(pos.max(frame_end))),
            b'(' | b'0' | b'1' | b'2' | b'N' | 0x88 | 0x89 | b'Q' | b'R' | b'a' | b'b' | b'd'
            | b'}' | b'e' | b'l' | b']' | b'o' | b's' | b't' | b')' | b'u' | 0x81 | 0x85 | 0x86
            | 0x87 | 0x8f | 0x90 | 0x91 | 0x92 | 0x93 | 0x94 | 0x97 | 0x98 => 0,
            b'K' | b'q' | b'h' | 0x80 | 0x82 => 1,
            b'M' | 0x83 => 2,
            b'J' | b'r' | b'j' | 0x84 => 4,
            0x95 => {
                let Some((size, len)) = counted(pos, 8, false)? else {
                    return Ok(None);
                };
                frame_end = frame_end.max(
                    usize::try_from(len)
                        .ok()
                        .and_then(|len| (pos + size).checked_add(len))
                        .unwrap_or(usize::MAX),
                );
                size
            }
            b'G' => 8,
            b'I' | b'L' | b'F' | b'S' | b'V' | b'P' | b'p' | b'g' | b'c' | b'i' => {
                let lines = if matches!(op, b'c' | b'i') { 2 } else { 1 };
                for _ in 0..lines {
                    match input[pos..].iter().position(|&c| c == b'\n') {
                        Some(end) => pos += end + 1,
                        None => return Ok(None),
                    }
                }
                0
            }
            b'T' | b'U' | b'X' | 0x8c | 0x8d | b'B' | b'C' | 0x8e | 0x96 | 0x8a | 0x8b => {
                let (size, signed) = match op {
                    b'U' | 0x8c | b'C' | 0x8a => (1, false),
                    b'T' | 0x8b => (4, true),
                    b'X' | b'B' => (4, false),
                    _ => (8, false),
                };
                let Some((size, len)) = counted(pos, size, signed)? else {
                    return Ok(None);
                };
                pos += size;
                match usize::try_from(len) {
                    Ok(len) if len <= input.len() - pos => len,
                    _ => return Ok(None),
                }
            }
            _ => return Err(syntax_error(op_start, "invalid opcode")),
        };
        if skip > input.len() - pos {
            return Ok(None);
        }
        pos += skip;
    }
}
