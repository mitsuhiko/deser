//! The reader of binary property lists (`bplist00`).
//!
//! A binary property list is a table of objects: the objects are followed
//! by a table with their offsets and a trailer that describes the table and
//! names the top object.  Containers refer to other objects by their index
//! in the table.
//!
//! The objects are read from the top object on with an explicit stack, so
//! deeply nested input does not overflow the stack.  Objects can be
//! referenced multiple times, which is supported for all objects.  Cycles
//! are an error.  As shared containers are emitted every time they are
//! referenced, the number of emitted values is limited to the size of the
//! input: without shared containers a value needs at least a byte for its
//! reference, so this only rejects input that would expand exponentially.
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::str;

use deser_core::ext::ExtValue;
use deser_core::{Atom, Bytes, ContainerShape, Error, ErrorKind, Event, Text};

use crate::common::{Out, eof_error, syntax_error, timestamp_from_plist};
use crate::uid::Uid;

const HEADER: &[u8] = b"bplist00";
const TRAILER_LEN: usize = 32;

/// An open array or dictionary.
struct Frame {
    object: usize,
    is_dict: bool,
    /// The offset of the object references.
    refs: usize,
    len: usize,
    /// The index of the next entry.
    idx: usize,
    /// For dictionaries: the key of the entry was emitted.
    in_value: bool,
}

/// The decoded head of an object.
enum Object<'i> {
    Null,
    Bool(bool),
    Int(i128),
    Real(f64),
    Date(f64),
    Data(&'i [u8]),
    Ascii(&'i [u8]),
    Utf16(&'i [u8]),
    Uid(u64),
    /// Arrays and sets.
    Array {
        refs: usize,
        len: usize,
    },
    Dict {
        refs: usize,
        len: usize,
    },
}

struct Reader<'i> {
    input: &'i [u8],
    offset_size: usize,
    ref_size: usize,
    num_objects: usize,
    table: usize,
    /// The containers on the path from the top object.
    open: Vec<bool>,
    /// How many more values can be emitted.
    budget: usize,
}

/// Parses a binary property list and emits its events.
pub(crate) fn parse<'i, O: Out<'i>>(input: &'i [u8], out: &mut O) -> Result<(), Error> {
    if !input.starts_with(HEADER) {
        return Err(if input.starts_with(b"bplist") {
            Error::new(
                ErrorKind::UnsupportedType,
                "unsupported binary property list version",
            )
        } else {
            syntax_error(0, "not a binary property list")
        });
    }
    if input.len() < HEADER.len() + TRAILER_LEN {
        return Err(eof_error(input.len()));
    }
    let trailer_start = input.len() - TRAILER_LEN;
    let trailer = &input[trailer_start..];
    let offset_size = usize::from(trailer[6]);
    let ref_size = usize::from(trailer[7]);
    let read_u64 = |pos: usize| u64::from_be_bytes(trailer[pos..pos + 8].try_into().unwrap());
    let (num_objects, top, table) = (read_u64(8), read_u64(16), read_u64(24));
    let invalid = |msg| syntax_error(trailer_start, msg);
    if !(1..=8).contains(&offset_size) || !(1..=8).contains(&ref_size) {
        return Err(invalid("invalid integer sizes in trailer"));
    }
    let table = usize::try_from(table)
        .ok()
        .filter(|&table| table >= HEADER.len())
        .ok_or_else(|| invalid("invalid offset table offset"))?;
    let num_objects = usize::try_from(num_objects)
        .ok()
        .filter(|&num| {
            num >= 1
                && num
                    .checked_mul(offset_size)
                    .and_then(|len| len.checked_add(table))
                    .is_some_and(|end| end <= trailer_start)
        })
        .ok_or_else(|| invalid("invalid number of objects"))?;
    if top >= num_objects as u64 {
        return Err(invalid("invalid top object"));
    }

    let mut reader = Reader {
        input,
        offset_size,
        ref_size,
        num_objects,
        table,
        open: vec![false; num_objects],
        budget: input.len(),
    };
    reader.run(top as usize, out)
}

impl<'i> Reader<'i> {
    fn run<O: Out<'i>>(&mut self, top: usize, out: &mut O) -> Result<(), Error> {
        let mut stack = Vec::new();
        if let Some(frame) = self.value(top, out)? {
            stack.push(frame);
        }
        while let Some(frame) = stack.last_mut() {
            if frame.idx == frame.len {
                let frame = stack.pop().unwrap();
                self.open[frame.object] = false;
                let pos = self.offset_of(frame.object)?;
                out.emit_at(
                    pos,
                    pos,
                    if frame.is_dict {
                        Event::MapEnd
                    } else {
                        Event::SeqEnd
                    },
                )?;
                continue;
            }
            let child = if frame.is_dict && !frame.in_value {
                frame.in_value = true;
                let key = self.read_ref(frame.refs + frame.idx * self.ref_size)?;
                self.key(key, out)?;
                continue;
            } else if frame.is_dict {
                frame.in_value = false;
                self.read_ref(frame.refs + (frame.len + frame.idx) * self.ref_size)?
            } else {
                self.read_ref(frame.refs + frame.idx * self.ref_size)?
            };
            frame.idx += 1;
            if let Some(frame) = self.value(child, out)? {
                stack.push(frame);
            }
        }
        Ok(())
    }

    /// Accounts for an emitted value.
    fn spend(&mut self, pos: usize) -> Result<(), Error> {
        match self.budget.checked_sub(1) {
            Some(budget) => {
                self.budget = budget;
                Ok(())
            }
            None => Err(syntax_error(
                pos,
                "shared objects expand beyond the size of the input",
            )),
        }
    }

    /// Emits a key of a dictionary.
    fn key<O: Out<'i>>(&mut self, object: usize, out: &mut O) -> Result<(), Error> {
        let pos = self.offset_of(object)?;
        self.spend(pos)?;
        let (value, end) = self.object(pos)?;
        match value {
            Object::Ascii(bytes) => {
                let text =
                    str::from_utf8(bytes).map_err(|_| syntax_error(pos, "invalid string"))?;
                out.emit_input_at(pos, end, Event::Atom(Atom::Lexical(Text::borrowed(text))))
            }
            Object::Utf16(bytes) => {
                let text =
                    decode_utf16(bytes).ok_or_else(|| syntax_error(pos, "invalid string"))?;
                out.emit_at(pos, end, Atom::Lexical(Text::owned(text)))
            }
            _ => Err(syntax_error(pos, "dictionary key is not a string")),
        }
    }

    /// Emits a value.  For containers, the frame is returned.
    fn value<O: Out<'i>>(&mut self, object: usize, out: &mut O) -> Result<Option<Frame>, Error> {
        let pos = self.offset_of(object)?;
        self.spend(pos)?;
        let (value, end) = self.object(pos)?;
        match value {
            Object::Null => out.emit_at(pos, end, Atom::Null)?,
            Object::Bool(value) => out.emit_at(pos, end, Atom::Bool(value))?,
            Object::Int(value) => {
                if let Ok(value) = u64::try_from(value) {
                    out.emit_at(pos, end, Atom::U64(value))?
                } else if let Ok(value) = i64::try_from(value) {
                    out.emit_at(pos, end, Atom::I64(value))?
                } else {
                    out.emit_at(pos, end, Atom::Ext(ExtValue::borrowed(&value)))?
                }
            }
            Object::Real(value) => out.emit_at(pos, end, Atom::F64(value))?,
            Object::Date(value) => {
                let value = timestamp_from_plist(value)
                    .ok_or_else(|| syntax_error(pos, "date out of range"))?;
                out.emit_at(pos, end, Atom::Ext(ExtValue::borrowed(&value)))?
            }
            Object::Data(bytes) => {
                out.emit_input_at(pos, end, Event::Atom(Atom::Bytes(Bytes::borrowed(bytes))))?
            }
            Object::Ascii(bytes) => {
                let text =
                    str::from_utf8(bytes).map_err(|_| syntax_error(pos, "invalid string"))?;
                out.emit_input_at(pos, end, Event::Atom(Atom::Str(Text::borrowed(text))))?
            }
            Object::Utf16(bytes) => {
                let text =
                    decode_utf16(bytes).ok_or_else(|| syntax_error(pos, "invalid string"))?;
                out.emit_at(pos, end, Atom::Str(Text::owned(text)))?
            }
            Object::Uid(value) => {
                out.emit_at(pos, end, Atom::Ext(ExtValue::owned(Uid::new(value))))?
            }
            Object::Array { refs, len } | Object::Dict { refs, len } => {
                let is_dict = matches!(value, Object::Dict { .. });
                if self.open[object] {
                    return Err(syntax_error(pos, "object references itself"));
                }
                let shape = ContainerShape::new().with_len(len);
                out.emit_at(
                    pos,
                    end,
                    if is_dict {
                        Event::MapStart(shape)
                    } else {
                        Event::SeqStart(shape)
                    },
                )?;
                self.open[object] = true;
                return Ok(Some(Frame {
                    object,
                    is_dict,
                    refs,
                    len,
                    idx: 0,
                    in_value: false,
                }));
            }
        }
        Ok(None)
    }

    /// Returns the offset of an object.
    fn offset_of(&self, object: usize) -> Result<usize, Error> {
        let pos = self.table + object * self.offset_size;
        let offset = read_uint(&self.input[pos..pos + self.offset_size]);
        usize::try_from(offset)
            .ok()
            .filter(|&offset| offset >= HEADER.len() && offset < self.table)
            .ok_or_else(|| syntax_error(pos, "invalid object offset"))
    }

    /// Reads an object reference at `pos`.
    fn read_ref(&self, pos: usize) -> Result<usize, Error> {
        let object = read_uint(&self.input[pos..pos + self.ref_size]);
        usize::try_from(object)
            .ok()
            .filter(|&object| object < self.num_objects)
            .ok_or_else(|| syntax_error(pos, "invalid object reference"))
    }

    /// Returns `len` bytes at `pos` which have to be in front of the offset
    /// table.
    fn bytes(&self, pos: usize, len: usize) -> Result<&'i [u8], Error> {
        match pos.checked_add(len) {
            Some(end) if end <= self.table => Ok(&self.input[pos..end]),
            _ => Err(syntax_error(pos, "object extends beyond the object table")),
        }
    }

    /// Decodes the object at `pos` and returns it with its end.
    fn object(&self, pos: usize) -> Result<(Object<'i>, usize), Error> {
        let marker = self.input[pos];
        let (kind, info) = (marker >> 4, marker & 0xf);
        let mut end = pos + 1;
        let unknown = || syntax_error(pos, "unknown object type");
        let value = match kind {
            0x0 => match info {
                0x0 => Object::Null,
                0x8 => Object::Bool(false),
                0x9 => Object::Bool(true),
                _ => return Err(unknown()),
            },
            0x1 => {
                let (value, int_end) = self.int(pos)?;
                end = int_end;
                Object::Int(value)
            }
            0x2 => match info {
                2 => {
                    let bytes = self.bytes(end, 4)?;
                    end += 4;
                    Object::Real(f64::from(f32::from_be_bytes(bytes.try_into().unwrap())))
                }
                3 => {
                    let bytes = self.bytes(end, 8)?;
                    end += 8;
                    Object::Real(f64::from_be_bytes(bytes.try_into().unwrap()))
                }
                _ => return Err(syntax_error(pos, "invalid size of real")),
            },
            0x3 if info == 3 => {
                let bytes = self.bytes(end, 8)?;
                end += 8;
                Object::Date(f64::from_be_bytes(bytes.try_into().unwrap()))
            }
            0x4..=0x6 => {
                let (len, body) = self.len(pos)?;
                let size = if kind == 0x6 {
                    len.checked_mul(2)
                        .ok_or_else(|| syntax_error(pos, "string too long"))?
                } else {
                    len
                };
                let bytes = self.bytes(body, size)?;
                end = body + size;
                match kind {
                    0x4 => Object::Data(bytes),
                    0x5 => Object::Ascii(bytes),
                    _ => Object::Utf16(bytes),
                }
            }
            0x8 => {
                let size = usize::from(info) + 1;
                if size > 8 {
                    return Err(syntax_error(pos, "uid out of range"));
                }
                let bytes = self.bytes(end, size)?;
                end += size;
                Object::Uid(read_uint(bytes))
            }
            0xa | 0xc | 0xd => {
                let (len, refs) = self.len(pos)?;
                let count = if kind == 0xd {
                    len.checked_mul(2)
                } else {
                    Some(len)
                };
                let size = count
                    .and_then(|count| count.checked_mul(self.ref_size))
                    .ok_or_else(|| syntax_error(pos, "container too long"))?;
                self.bytes(refs, size)?;
                end = refs + size;
                if kind == 0xd {
                    Object::Dict { refs, len }
                } else {
                    Object::Array { refs, len }
                }
            }
            _ => return Err(unknown()),
        };
        Ok((value, end))
    }

    /// Reads an integer object at `pos`.
    fn int(&self, pos: usize) -> Result<(i128, usize), Error> {
        let marker = self.input[pos];
        if marker >> 4 != 0x1 {
            return Err(syntax_error(pos, "expected integer"));
        }
        let size = match marker & 0xf {
            info @ 0..=4 => 1usize << info,
            _ => return Err(syntax_error(pos, "invalid size of integer")),
        };
        let bytes = self.bytes(pos + 1, size)?;
        let value = match size {
            // up to four bytes the integers are unsigned
            1 | 2 | 4 => i128::from(read_uint(bytes)),
            8 => i128::from(i64::from_be_bytes(bytes.try_into().unwrap())),
            _ => i128::from_be_bytes(bytes.try_into().unwrap()),
        };
        Ok((value, pos + 1 + size))
    }

    /// Reads the length of the object at `pos` and returns it with the
    /// offset of the body.
    fn len(&self, pos: usize) -> Result<(usize, usize), Error> {
        let info = self.input[pos] & 0xf;
        if info != 0xf {
            return Ok((usize::from(info), pos + 1));
        }
        if pos + 1 >= self.table {
            return Err(syntax_error(pos, "object extends beyond the object table"));
        }
        let (len, end) = self.int(pos + 1)?;
        let len = usize::try_from(len).map_err(|_| syntax_error(pos, "invalid length"))?;
        Ok((len, end))
    }
}

/// Reads a big-endian unsigned integer of up to eight bytes.
fn read_uint(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0u64, |acc, &byte| (acc << 8) | u64::from(byte))
}

/// Decodes a UTF-16 string (big-endian).
fn decode_utf16(bytes: &[u8]) -> Option<String> {
    let units = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&unit| u16::from_be_bytes(unit));
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .ok()
}
