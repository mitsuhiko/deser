use alloc::format;
use alloc::vec::Vec;
use core::mem::ManuallyDrop;

use deser_core::__format::extend;
use deser_core::State;
use deser_core::ext::{BigInt, ExtValue, Timestamp};
use deser_core::ser::{self, SerializeDriver};
use deser_core::{Atom, ContainerShape, Error, ErrorKind, Event, Serialize};

use crate::ext::{Ext, TIMESTAMP, encode_timestamp};

/// Configures how values are serialized to MessagePack.
///
/// Integers and the lengths of strings, binary data, arrays and maps are
/// written in their shortest form.  Floats keep their precision: `f32` is
/// written as float 32 and `f64` as float 64.
///
/// If [`canonical`](Self::canonical) is enabled, the output is additionally
/// deterministic: the entries of maps are sorted by the bytewise
/// lexicographic order of their encoded keys and duplicate keys are
/// rejected.
///
/// ```
/// use std::collections::HashMap;
/// use deser_msgpack::SerializerConfig;
///
/// const CANONICAL: SerializerConfig = SerializerConfig::new().canonical(true);
/// let map = HashMap::from([("b", 1), ("a", 2)]);
/// assert_eq!(CANONICAL.to_vec(&map).unwrap(), b"\x82\xa1a\x02\xa1b\x01");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SerializerConfig {
    canonical: bool,
}

/// An open map or array (or the top level).
///
/// This is kept small as it's moved to the stack and back for every
/// container.
#[derive(Clone, Copy)]
struct Frame {
    /// Counts down with every item (for maps keys and values are counted
    /// separately).  If the length is known, it starts at the number of
    /// items the header announced and ends at zero.  Otherwise it starts
    /// at `u64::MAX` and the length is patched into the header at the end.
    remaining: u64,
    /// The offset of the header shifted by two, the flags [`IS_MAP`] and
    /// [`UNKNOWN_LEN`] are in the lower bits.
    info: usize,
}

const IS_MAP: usize = 1;
const UNKNOWN_LEN: usize = 2;

impl Frame {
    /// The frame of the top level, which is not a container.
    const TOP: Frame = Frame {
        remaining: u64::MAX,
        info: UNKNOWN_LEN,
    };

    #[inline(always)]
    fn is_map(self) -> bool {
        self.info & IS_MAP != 0
    }

    #[inline(always)]
    fn header(self) -> usize {
        self.info >> 2
    }
}

/// The start of a map in canonical mode.
struct CanonicalMap {
    /// The offset of the content (after the header).
    body: usize,
    /// The index into the entry offsets where the offsets of this map
    /// begin.
    offsets_start: usize,
}

/// Holds the state of the serializer while writing.
struct Writer {
    out: Vec<u8>,
    canonical: bool,
    // the frame of the current container is held here, the frames of the
    // outer containers are saved on the stack.
    frame: Frame,
    stack: Vec<Frame>,
    // in canonical mode the open maps and the offsets of their keys and
    // values
    maps: Vec<CanonicalMap>,
    offsets: Vec<usize>,
    // bytes to be inserted into the output at the end, see `patch_length`.
    insertions: Vec<Insertion>,
}

/// The bytes of a container header that did not fit into the space
/// reserved for it.
struct Insertion {
    offset: usize,
    len: u8,
    bytes: [u8; 4],
}

impl ser::EventSink for Writer {
    #[inline(always)]
    fn event(&mut self, event: Event, _state: &mut State) -> Result<(), Error> {
        Writer::event(self, event)
    }
}

impl Writer {
    #[inline(always)]
    fn event(&mut self, event: Event) -> Result<(), Error> {
        match event {
            Event::Atom(atom) => {
                self.begin_item();
                self.write_atom(atom)
            }
            Event::MapStart(shape) => self.start(true, shape),
            Event::SeqStart(shape) => self.start(false, shape),
            Event::MapEnd | Event::SeqEnd => self.end(),
        }
    }

    /// Accounts for a new item in the current container.
    #[inline(always)]
    fn begin_item(&mut self) {
        self.frame.remaining = self.frame.remaining.wrapping_sub(1);
        if self.canonical && self.frame.is_map() {
            self.offsets.push(self.out.len());
        }
    }

    #[inline(always)]
    fn start(&mut self, is_map: bool, shape: ContainerShape) -> Result<(), Error> {
        self.begin_item();
        let header = self.out.len();
        let mut info = (header << 2) | if is_map { IS_MAP } else { 0 };
        // with a known length the header is written right away, otherwise a
        // byte is reserved and the length is patched in at the end.
        let remaining = match shape.len() {
            Some(len) => {
                let len = check_len(len)?;
                let mut buf = [0u8; 5];
                extend(&mut self.out, encode_container_head(&mut buf, is_map, len));
                if is_map {
                    u64::from(len) * 2
                } else {
                    u64::from(len)
                }
            }
            None => {
                self.out.push(if is_map { 0x80 } else { 0x90 });
                info |= UNKNOWN_LEN;
                u64::MAX
            }
        };
        if self.canonical && is_map {
            self.maps.push(CanonicalMap {
                body: self.out.len(),
                offsets_start: self.offsets.len(),
            });
        }
        self.stack.push(core::mem::replace(
            &mut self.frame,
            Frame { remaining, info },
        ));
        Ok(())
    }

    #[inline(always)]
    fn end(&mut self) -> Result<(), Error> {
        let Some(parent) = self.stack.pop() else {
            return Err(Error::new(ErrorKind::Unexpected, "unexpected end"));
        };
        let frame = core::mem::replace(&mut self.frame, parent);
        if frame.remaining == 0 && !self.canonical {
            Ok(())
        } else {
            self.end_slow(frame)
        }
    }

    /// Ends a container that needs more than a check: the length was not
    /// known upfront, the number of items does not match or maps in
    /// canonical mode.
    #[inline(never)]
    fn end_slow(&mut self, frame: Frame) -> Result<(), Error> {
        let unknown = frame.info & UNKNOWN_LEN != 0;
        if !unknown && frame.remaining != 0 {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "number of items does not match the length of the container",
            ));
        }
        let items = if unknown {
            u64::MAX - frame.remaining
        } else {
            0
        };
        if frame.is_map() {
            if !items.is_multiple_of(2) {
                return Err(Error::new(ErrorKind::Unexpected, "map without value"));
            }
            if self.canonical {
                let map = self.maps.pop().unwrap();
                self.sort_entries(&map)?;
            }
        }
        if unknown {
            let count = if frame.is_map() { items / 2 } else { items };
            let count = u32::try_from(count).map_err(|_| too_long())?;
            self.patch_length(frame.header(), frame.is_map(), count);
        }
        Ok(())
    }

    /// Patches the length of a container into the header.
    ///
    /// Only a single byte was reserved for the header.  If the length does
    /// not fit, the remaining bytes of the header need to be inserted after
    /// it.  Normally the insertions are deferred until the end so that the
    /// output is only moved once.  In canonical mode the entries of maps are
    /// moved when sorted, so the bytes are inserted immediately.
    fn patch_length(&mut self, header: usize, is_map: bool, count: u32) {
        let mut buf = [0u8; 5];
        let head = encode_container_head(&mut buf, is_map, count);
        self.out[header] = head[0];
        let extra = head.len() - 1;
        if extra == 0 {
            return;
        }
        if self.canonical {
            let len = self.out.len();
            self.out.resize(len + extra, 0);
            self.out.copy_within(header + 1..len, header + 1 + extra);
            self.out[header + 1..header + head.len()].copy_from_slice(&head[1..]);
        } else {
            let mut bytes = [0; 4];
            bytes[..extra].copy_from_slice(&head[1..]);
            self.insertions.push(Insertion {
                offset: header + 1,
                len: extra as u8,
                bytes,
            });
        }
    }

    /// Applies the deferred insertions.
    #[cold]
    fn apply_insertions(&mut self) {
        self.insertions
            .sort_unstable_by_key(|insertion| insertion.offset);
        let extra: usize = self.insertions.iter().map(|x| x.len as usize).sum();
        let mut src_end = self.out.len();
        self.out.resize(src_end + extra, 0);
        let mut dst_end = self.out.len();
        // move the segments between the insertions from the back so that
        // every byte is moved once.
        for insertion in self.insertions.iter().rev() {
            let segment = src_end - insertion.offset;
            self.out
                .copy_within(insertion.offset..src_end, dst_end - segment);
            dst_end -= segment + insertion.len as usize;
            self.out[dst_end..dst_end + insertion.len as usize]
                .copy_from_slice(&insertion.bytes[..insertion.len as usize]);
            src_end = insertion.offset;
        }
        debug_assert_eq!(src_end, dst_end);
    }

    /// Sorts the entries of a map for the deterministic encoding.
    #[cold]
    fn sort_entries(&mut self, map: &CanonicalMap) -> Result<(), Error> {
        let offsets = self.offsets.split_off(map.offsets_start);
        let body_start = map.body;
        let body_end = self.out.len();
        // (key start, value start, entry end)
        let mut entries: Vec<(usize, usize, usize)> = (0..offsets.len())
            .step_by(2)
            .map(|idx| {
                let end = offsets.get(idx + 2).copied().unwrap_or(body_end);
                (offsets[idx], offsets[idx + 1], end)
            })
            .collect();
        let out = &self.out;
        entries.sort_by(|a, b| out[a.0..a.1].cmp(&out[b.0..b.1]));
        if entries
            .windows(2)
            .any(|pair| out[pair[0].0..pair[0].1] == out[pair[1].0..pair[1].1])
        {
            return Err(Error::new(ErrorKind::Unexpected, "duplicate map key"));
        }
        let mut body = Vec::with_capacity(body_end - body_start);
        for (start, _, end) in entries {
            body.extend_from_slice(&out[start..end]);
        }
        self.out[body_start..body_end].copy_from_slice(&body);
        Ok(())
    }

    #[inline(always)]
    fn write_atom(&mut self, atom: Atom) -> Result<(), Error> {
        // borrowed strings and scalars do not need to be dropped, the atom
        // is only dropped for the other values.
        let atom = ManuallyDrop::new(atom);
        match *atom {
            Atom::Null => self.out.push(0xc0),
            Atom::Bool(false) => self.out.push(0xc2),
            Atom::Bool(true) => self.out.push(0xc3),
            Atom::Str(ref val) if val.is_borrowed() => self.write_str(val)?,
            Atom::Bytes(ref val) if val.is_borrowed() => self.write_bin(val)?,
            Atom::Char(c) => self.write_str(c.encode_utf8(&mut [0u8; 4]))?,
            Atom::U64(val) => self.write_u64(val),
            Atom::I64(val) => self.write_i64(val),
            Atom::F64(val) => {
                let mut buf = [0xcb; 9];
                buf[1..].copy_from_slice(&val.to_be_bytes());
                extend(&mut self.out, &buf);
            }
            Atom::F32(val) => {
                let mut buf = [0xca; 5];
                buf[1..].copy_from_slice(&val.to_be_bytes());
                extend(&mut self.out, &buf);
            }
            _ => return self.write_other_atom(ManuallyDrop::into_inner(atom)),
        }
        Ok(())
    }

    #[inline(never)]
    fn write_other_atom(&mut self, atom: Atom) -> Result<(), Error> {
        match atom {
            Atom::Str(ref val) | Atom::Lexical(ref val) => self.write_str(val),
            Atom::Bytes(ref val) => self.write_bin(val),
            Atom::Ext(ref ext) => self.write_ext(ext),
            // values whose type was inferred from text are written as value
            Atom::Implicit(ref val) => self.write_atom(val.value().to_atom()),
            _ => Err(Error::new(ErrorKind::UnsupportedType, "unknown atom")),
        }
    }

    /// Writes the head of a string, binary data or extension.
    ///
    /// The heads are given for the 8, 16 and 32 bit lengths.
    #[inline(always)]
    fn write_len(&mut self, heads: [u8; 3], len: usize) -> Result<(), Error> {
        let len = check_len(len)?;
        if len <= u32::from(u8::MAX) {
            extend(&mut self.out, &[heads[0], len as u8]);
        } else if len <= u32::from(u16::MAX) {
            let [a, b] = (len as u16).to_be_bytes();
            extend(&mut self.out, &[heads[1], a, b]);
        } else {
            let [a, b, c, d] = len.to_be_bytes();
            extend(&mut self.out, &[heads[2], a, b, c, d]);
        }
        Ok(())
    }

    #[inline(always)]
    fn write_bin(&mut self, val: &[u8]) -> Result<(), Error> {
        self.write_len([0xc4, 0xc5, 0xc6], val.len())?;
        extend(&mut self.out, val);
        Ok(())
    }

    #[inline(always)]
    fn write_str(&mut self, val: &str) -> Result<(), Error> {
        if val.len() < 32 {
            self.out.push(0xa0 | val.len() as u8);
        } else {
            self.write_len([0xd9, 0xda, 0xdb], val.len())?;
        }
        extend(&mut self.out, val.as_bytes());
        Ok(())
    }

    #[inline(always)]
    fn write_u64(&mut self, val: u64) {
        if val < 128 {
            self.out.push(val as u8);
        } else if val <= u64::from(u8::MAX) {
            extend(&mut self.out, &[0xcc, val as u8]);
        } else if val <= u64::from(u16::MAX) {
            let [a, b] = (val as u16).to_be_bytes();
            extend(&mut self.out, &[0xcd, a, b]);
        } else if val <= u64::from(u32::MAX) {
            let [a, b, c, d] = (val as u32).to_be_bytes();
            extend(&mut self.out, &[0xce, a, b, c, d]);
        } else {
            let mut buf = [0xcf; 9];
            buf[1..].copy_from_slice(&val.to_be_bytes());
            extend(&mut self.out, &buf);
        }
    }

    #[inline(always)]
    fn write_i64(&mut self, val: i64) {
        if val >= 0 {
            self.write_u64(val as u64);
        } else if val >= -32 {
            self.out.push(val as u8);
        } else if val >= i64::from(i8::MIN) {
            extend(&mut self.out, &[0xd0, val as u8]);
        } else if val >= i64::from(i16::MIN) {
            let [a, b] = (val as i16).to_be_bytes();
            extend(&mut self.out, &[0xd1, a, b]);
        } else if val >= i64::from(i32::MIN) {
            let [a, b, c, d] = (val as i32).to_be_bytes();
            extend(&mut self.out, &[0xd2, a, b, c, d]);
        } else {
            let mut buf = [0xd3; 9];
            buf[1..].copy_from_slice(&val.to_be_bytes());
            extend(&mut self.out, &buf);
        }
    }

    /// Writes an extension.
    fn write_ext_data(&mut self, kind: i8, data: &[u8]) -> Result<(), Error> {
        match data.len() {
            1 => self.out.push(0xd4),
            2 => self.out.push(0xd5),
            4 => self.out.push(0xd6),
            8 => self.out.push(0xd7),
            16 => self.out.push(0xd8),
            len => self.write_len([0xc7, 0xc8, 0xc9], len)?,
        }
        self.out.push(kind as u8);
        extend(&mut self.out, data);
        Ok(())
    }

    #[cold]
    fn write_ext(&mut self, ext: &ExtValue) -> Result<(), Error> {
        if let Some(val) = ext.downcast_ref::<Ext>() {
            self.write_ext_data(val.kind, &val.data)
        } else if let Some(val) = ext.downcast_ref::<Timestamp>() {
            let mut buf = [0; 12];
            self.write_ext_data(TIMESTAMP, encode_timestamp(val, &mut buf))
        } else if let Some(&val) = ext.downcast_ref::<u128>() {
            let val = u64::try_from(val).map_err(|_| int_out_of_range())?;
            self.write_u64(val);
            Ok(())
        } else if let Some(&val) = ext.downcast_ref::<i128>() {
            if let Ok(val) = i64::try_from(val) {
                self.write_i64(val);
            } else {
                let val = u64::try_from(val).map_err(|_| int_out_of_range())?;
                self.write_u64(val);
            }
            Ok(())
        } else if let Some(val) = ext
            .downcast_ref::<BigInt>()
            .and_then(|x| x.to_i128())
            .filter(|&x| i64::try_from(x).is_ok() || u64::try_from(x).is_ok())
        {
            // big integers are integers if they fit, otherwise their fallback
            match i64::try_from(val) {
                Ok(val) => self.write_i64(val),
                Err(_) => self.write_u64(val as u64),
            }
            Ok(())
        } else {
            match ext.fallback() {
                Atom::Ext(_) => Err(Error::new(
                    ErrorKind::UnsupportedType,
                    format!("MessagePack does not support {}", ext.name()),
                )),
                fallback => self.write_atom(fallback),
            }
        }
    }
}

/// Checks that a length fits into the 32 bits of MessagePack.
#[inline(always)]
fn check_len(len: usize) -> Result<u32, Error> {
    u32::try_from(len).map_err(|_| too_long())
}

#[cold]
fn too_long() -> Error {
    Error::new(ErrorKind::OutOfRange, "length out of range for MessagePack")
}

#[cold]
fn int_out_of_range() -> Error {
    Error::new(
        ErrorKind::OutOfRange,
        "integer out of range for MessagePack",
    )
}

/// Encodes the head of an array or map into the buffer.
#[inline]
fn encode_container_head(buf: &mut [u8; 5], is_map: bool, len: u32) -> &[u8] {
    if len < 16 {
        buf[0] = if is_map { 0x80 } else { 0x90 } | len as u8;
        &buf[..1]
    } else if len <= u32::from(u16::MAX) {
        buf[0] = if is_map { 0xde } else { 0xdc };
        buf[1..3].copy_from_slice(&(len as u16).to_be_bytes());
        &buf[..3]
    } else {
        buf[0] = if is_map { 0xdf } else { 0xdd };
        buf[1..5].copy_from_slice(&len.to_be_bytes());
        &buf[..5]
    }
}

impl SerializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> SerializerConfig {
        SerializerConfig { canonical: false }
    }

    /// Enables or disables the deterministic encoding.
    ///
    /// When enabled, the entries of maps are sorted by the bytewise
    /// lexicographic order of their encoded keys and duplicate keys are an
    /// error.  This makes the output independent of the iteration
    /// order of maps such as `HashMap`.
    pub const fn canonical(mut self, yes: bool) -> SerializerConfig {
        self.canonical = yes;
        self
    }

    /// Serializes the given value.
    pub fn to_vec(&self, value: &dyn Serialize) -> Result<Vec<u8>, Error> {
        self.to_vec_with(value, |_| {})
    }

    /// Serializes the given value with a configured driver.
    ///
    /// The callback is invoked with the driver before the serialization
    /// starts, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn to_vec_with<F>(&self, value: &dyn Serialize, setup: F) -> Result<Vec<u8>, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        self.serialize_driver(&mut driver)
    }

    /// Serializes the value of a driver.
    pub(crate) fn serialize_driver(
        &self,
        driver: &mut SerializeDriver<'_>,
    ) -> Result<Vec<u8>, Error> {
        let mut writer = Writer {
            out: Vec::with_capacity(128),
            canonical: self.canonical,
            frame: Frame::TOP,
            stack: Vec::new(),
            maps: Vec::new(),
            offsets: Vec::new(),
            insertions: Vec::new(),
        };
        driver.drive_sink(&mut writer)?;
        if !writer.insertions.is_empty() {
            writer.apply_insertions();
        }
        Ok(writer.out)
    }
}

/// Serializes values into MessagePack.
///
/// Every call to [`serialize`](Self::serialize) writes an item, the items
/// follow each other (which is how MessagePack streams work).
///
/// ```
/// use deser_msgpack::Serializer;
///
/// let mut serializer = Serializer::new();
/// serializer.serialize(&1u32).unwrap();
/// serializer.serialize(&"hi").unwrap();
/// assert_eq!(serializer.finish(), [0x01, 0xa2, b'h', b'i']);
/// ```
///
/// To write to a [`Write`](std::io::Write) use a
/// [`deser::io::Writer`](deser_core::io::Writer) with the configuration.
#[derive(Debug, Clone)]
pub struct Serializer {
    config: SerializerConfig,
    out: Vec<u8>,
    written: usize,
}

impl Default for Serializer {
    fn default() -> Serializer {
        Serializer::new()
    }
}

impl Serializer {
    /// Creates a serializer.
    pub fn new() -> Serializer {
        Serializer::with_config(&SerializerConfig::new())
    }

    /// Creates a serializer with the given configuration.
    pub fn with_config(config: &SerializerConfig) -> Serializer {
        Serializer {
            config: config.clone(),
            out: Vec::new(),
            written: 0,
        }
    }

    /// Serializes a value.
    ///
    /// If the value fails to serialize, nothing is written.
    pub fn serialize(&mut self, value: &dyn Serialize) -> Result<(), Error> {
        ser::Serializer::serialize(self, value)
    }

    /// Serializes a value with a configured driver.
    ///
    /// The callback is invoked with the driver before the value is
    /// serialized, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn serialize_with<F>(&mut self, value: &dyn Serialize, setup: F) -> Result<(), Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        ser::Serializer::serialize_with(self, value, setup)
    }

    /// Returns the output written so far.
    pub fn output(&self) -> &[u8] {
        &self.out
    }

    /// Returns the output.
    pub fn finish(self) -> Vec<u8> {
        self.out
    }
}

impl ser::Serializer for Serializer {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        let bytes = self.config.serialize_driver(driver)?;
        self.out.extend_from_slice(&bytes);
        self.written += 1;
        Ok(())
    }
}

/// Serializes a value to MessagePack.
///
/// This uses the default [`SerializerConfig`].
pub fn to_vec(value: &dyn Serialize) -> Result<Vec<u8>, Error> {
    SerializerConfig::new().to_vec(value)
}
