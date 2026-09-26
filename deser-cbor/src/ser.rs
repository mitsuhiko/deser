use std::borrow::Cow;
use std::mem::ManuallyDrop;

use deser::State;
use deser::ext::{BigInt, Datetime, Decimal, ExtValue, Timestamp, Uuid};
use deser::ser::SerializeDriver;
use deser::{Atom, Bytes, ContainerShape, Error, ErrorKind, Event, Serialize};

use crate::buf::extend;
use crate::float::f64_to_f16;
use crate::simple::Simple;
use crate::tag::Tags;

const MAJOR_UNSIGNED: u8 = 0;
const MAJOR_NEGATIVE: u8 = 1;
const MAJOR_BYTES: u8 = 2;
const MAJOR_TEXT: u8 = 3;
const MAJOR_ARRAY: u8 = 4;
const MAJOR_MAP: u8 = 5;
const MAJOR_TAG: u8 = 6;

/// Configures how values are serialized to CBOR.
///
/// The output uses the preferred serialization of RFC 8949: integers,
/// lengths and floats use their shortest (lossless) form and all maps and
/// arrays have a definite length.
///
/// If [`canonical`](Self::canonical) is enabled, the output is
/// additionally deterministically encoded (RFC 8949 §4.2.1): the entries of
/// maps are sorted by the bytewise lexicographic order of their encoded keys
/// and duplicate keys are rejected.
///
/// ```
/// use std::collections::HashMap;
/// use deser_cbor::SerializerConfig;
///
/// const CANONICAL: SerializerConfig = SerializerConfig::new().canonical(true);
/// let map = HashMap::from([("b", 1), ("a", 2)]);
/// assert_eq!(CANONICAL.to_vec(&map).unwrap(), b"\xa2\x61a\x02\x61b\x01");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SerializerConfig {
    canonical: bool,
}

/// An open map or array.
struct Frame {
    /// The offset of the header.  If the length is not known upfront, the
    /// header is written with a length of zero and patched when the
    /// container ends.
    header: usize,
    /// The offset of the content (after the header).
    body: usize,
    /// The length written into the header if it was known upfront.
    len: Option<u64>,
    /// The number of items written into the container so far.  For maps
    /// both keys and values are counted.
    items: u64,
    is_map: bool,
    /// For maps in canonical mode: the index into the entry offsets where
    /// the offsets of this map begin.
    offsets_start: usize,
}

/// Holds the state of the serializer while writing.
struct Writer {
    out: Vec<u8>,
    canonical: bool,
    // the frame of the current container is held here, the frames of the
    // outer containers are saved on the stack.
    frame: Option<Frame>,
    stack: Vec<Frame>,
    // in canonical mode the offsets of the keys and values of the open maps
    offsets: Vec<usize>,
    // bytes to be inserted into the output at the end, see `patch_length`.
    insertions: Vec<Insertion>,
}

/// The bytes of a container header that did not fit into the space
/// reserved for it.
struct Insertion {
    offset: usize,
    len: u8,
    bytes: [u8; 8],
}

impl Writer {
    #[inline(always)]
    fn event(&mut self, event: Event, state: &State) -> Result<(), Error> {
        match event {
            Event::Atom(atom) => {
                self.begin_item(state);
                self.write_atom(atom)
            }
            Event::MapStart(shape) => self.start(true, shape, state),
            Event::SeqStart(shape) => self.start(false, shape, state),
            Event::MapEnd | Event::SeqEnd => self.end(),
        }
    }

    /// Accounts for a new item in the current container and writes the
    /// pending tags.
    #[inline(always)]
    fn begin_item(&mut self, state: &State) {
        if let Some(ref mut frame) = self.frame {
            frame.items += 1;
            if self.canonical && frame.is_map {
                self.offsets.push(self.out.len());
            }
        }
        if state.has_event_data() {
            self.write_tags(state);
        }
    }

    /// Writes the tags attached to the current event.
    #[cold]
    fn write_tags(&mut self, state: &State) {
        if let Some(tags) = state.event::<Tags>() {
            for &tag in tags.0.iter() {
                self.write_head(MAJOR_TAG, tag);
            }
        }
    }

    #[inline(never)]
    fn start(&mut self, is_map: bool, shape: ContainerShape, state: &State) -> Result<(), Error> {
        self.begin_item(state);
        let header = self.out.len();
        let major = if is_map { MAJOR_MAP } else { MAJOR_ARRAY };
        // with a known length the header is written right away, otherwise a
        // byte is reserved and the length is patched in at the end.
        let len = shape.len().map(|len| len as u64);
        match len {
            Some(len) => self.write_head(major, len),
            None => self.out.push(major << 5),
        }
        let frame = Frame {
            header,
            body: self.out.len(),
            items: 0,
            is_map,
            offsets_start: self.offsets.len(),
            len,
        };
        if let Some(parent) = self.frame.replace(frame) {
            self.stack.push(parent);
        }
        Ok(())
    }

    #[inline(never)]
    fn end(&mut self) -> Result<(), Error> {
        let frame = match self.frame.take() {
            Some(frame) => frame,
            None => return Err(Error::new(ErrorKind::Unexpected, "unexpected end")),
        };
        self.frame = self.stack.pop();
        let count = if frame.is_map {
            if frame.items % 2 != 0 {
                return Err(Error::new(ErrorKind::Unexpected, "map without value"));
            }
            if self.canonical {
                self.sort_entries(&frame)?;
            }
            frame.items / 2
        } else {
            frame.items
        };
        match frame.len {
            Some(len) if len != count => Err(Error::new(
                ErrorKind::Unexpected,
                "number of items does not match the length of the container",
            )),
            Some(_) => Ok(()),
            None => {
                self.patch_length(frame.header, count);
                Ok(())
            }
        }
    }

    /// Patches the length of a container into the header.
    ///
    /// Only a single byte was reserved for the header.  If the length does
    /// not fit, the remaining bytes of the header need to be inserted after
    /// it.  Normally the insertions are deferred until the end so that the
    /// output is only moved once.  In canonical mode the entries of maps are
    /// moved when sorted, so the bytes are inserted immediately.
    fn patch_length(&mut self, header: usize, count: u64) {
        let major = self.out[header];
        if count < 24 {
            self.out[header] = major | count as u8;
            return;
        }
        let mut buf = [0u8; 9];
        let head = encode_head(&mut buf, major >> 5, count);
        self.out[header] = head[0];
        if self.canonical {
            let extra = head.len() - 1;
            let len = self.out.len();
            self.out.resize(len + extra, 0);
            self.out.copy_within(header + 1..len, header + 1 + extra);
            self.out[header + 1..header + head.len()].copy_from_slice(&head[1..]);
        } else {
            self.insertions.push(Insertion {
                offset: header + 1,
                len: head.len() as u8 - 1,
                bytes: {
                    let mut bytes = [0; 8];
                    bytes[..head.len() - 1].copy_from_slice(&head[1..]);
                    bytes
                },
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
    fn sort_entries(&mut self, frame: &Frame) -> Result<(), Error> {
        let offsets = self.offsets.split_off(frame.offsets_start);
        let body_start = frame.body;
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
    fn write_head(&mut self, major: u8, value: u64) {
        if value < 24 {
            self.out.push(major << 5 | value as u8);
        } else {
            let mut buf = [0u8; 9];
            let head = encode_head(&mut buf, major, value);
            extend(&mut self.out, head);
        }
    }

    #[inline(always)]
    fn write_atom(&mut self, atom: Atom) -> Result<(), Error> {
        // borrowed strings and scalars do not need to be dropped, the atom
        // is only dropped for the other values.
        let atom = ManuallyDrop::new(atom);
        match *atom {
            Atom::Null => self.out.push(0xf6),
            Atom::Bool(false) => self.out.push(0xf4),
            Atom::Bool(true) => self.out.push(0xf5),
            Atom::Str(Cow::Borrowed(val)) => self.write_str(val),
            Atom::Bytes(Bytes {
                data: Cow::Borrowed(val),
                ..
            }) => self.write_bytes(val),
            Atom::Char(c) => self.write_str(c.encode_utf8(&mut [0u8; 4])),
            Atom::U64(val) => self.write_head(MAJOR_UNSIGNED, val),
            Atom::I64(val) => self.write_i64(val),
            Atom::F64(val) => self.write_f64(val),
            _ => return self.write_other_atom(ManuallyDrop::into_inner(atom)),
        }
        Ok(())
    }

    #[inline(never)]
    fn write_other_atom(&mut self, atom: Atom) -> Result<(), Error> {
        match atom {
            Atom::Str(ref val) => self.write_str(val),
            Atom::Bytes(ref val) => self.write_bytes(val),
            Atom::Ext(ref ext) => return self.write_ext(ext),
            _ => return Err(Error::new(ErrorKind::UnsupportedType, "unknown atom")),
        }
        Ok(())
    }

    #[inline(always)]
    fn write_bytes(&mut self, val: &[u8]) {
        self.write_head(MAJOR_BYTES, val.len() as u64);
        extend(&mut self.out, val);
    }

    #[inline(always)]
    fn write_str(&mut self, val: &str) {
        self.write_head(MAJOR_TEXT, val.len() as u64);
        extend(&mut self.out, val.as_bytes());
    }

    #[inline(always)]
    fn write_i64(&mut self, val: i64) {
        if val >= 0 {
            self.write_head(MAJOR_UNSIGNED, val as u64);
        } else {
            // -1 - val without overflows
            self.write_head(MAJOR_NEGATIVE, !val as u64);
        }
    }

    /// Writes a float in the shortest form that preserves its value.
    fn write_f64(&mut self, val: f64) {
        if val.is_nan() {
            self.out.extend_from_slice(&[0xf9, 0x7e, 0x00]);
        } else if let Some(half) = f64_to_f16(val) {
            self.out.push(0xf9);
            self.out.extend_from_slice(&half.to_be_bytes());
        } else if f64::from(val as f32) == val {
            self.out.push(0xfa);
            self.out.extend_from_slice(&(val as f32).to_be_bytes());
        } else {
            self.out.push(0xfb);
            self.out.extend_from_slice(&val.to_be_bytes());
        }
    }

    #[cold]
    fn write_ext(&mut self, ext: &ExtValue) -> Result<(), Error> {
        if let Some(&val) = ext.downcast_ref::<u128>() {
            self.write_u128(val);
        } else if let Some(&val) = ext.downcast_ref::<i128>() {
            self.write_i128(val);
        } else if let Some(val) = ext.downcast_ref::<BigInt>() {
            self.write_bigint(val);
        } else if let Some(val) = ext.downcast_ref::<Datetime>() {
            if val.offset.is_some() {
                // standard date/time string
                self.write_head(MAJOR_TAG, 0);
            } else if val.date.is_some() && val.time.is_none() {
                // full-date string (RFC 8943)
                self.write_head(MAJOR_TAG, 1004);
            }
            self.write_str(&val.to_string());
        } else if let Some(val) = ext.downcast_ref::<Timestamp>() {
            if val.nanosecond == 0 {
                self.write_head(MAJOR_TAG, 1);
                self.write_i64(val.seconds);
            } else if let Some(datetime) = val.to_datetime() {
                // date/time strings retain the precision
                self.write_head(MAJOR_TAG, 0);
                self.write_str(&datetime.to_string());
            } else {
                self.write_head(MAJOR_TAG, 1);
                self.write_f64(val.as_secs_f64());
            }
        } else if let Some(val) = ext.downcast_ref::<Uuid>() {
            self.write_head(MAJOR_TAG, 37);
            self.write_bytes(&val.0);
        } else if let Some(val) = ext.downcast_ref::<Decimal>() {
            // decimal fraction: [exponent, mantissa]
            let (mantissa, exponent) = val.to_parts();
            self.write_head(MAJOR_TAG, 4);
            self.write_head(MAJOR_ARRAY, 2);
            self.write_i64(exponent);
            self.write_bigint(&mantissa);
        } else if let Some(&simple) = ext.downcast_ref::<Simple>() {
            let value = simple.value();
            if value < 24 {
                self.out.push(0xe0 | value);
            } else {
                self.out.extend_from_slice(&[0xf8, value]);
            }
        } else {
            match ext.fallback() {
                Atom::Ext(_) => {
                    return Err(Error::new(
                        ErrorKind::UnsupportedType,
                        "unsupported extension value",
                    ));
                }
                fallback => return self.write_atom(fallback),
            }
        }
        Ok(())
    }

    fn write_u128(&mut self, val: u128) {
        match u64::try_from(val) {
            Ok(val) => self.write_head(MAJOR_UNSIGNED, val),
            Err(_) => self.write_bignum(2, val),
        }
    }

    fn write_i128(&mut self, val: i128) {
        if let Ok(val) = i64::try_from(val) {
            self.write_i64(val);
        } else if val >= 0 {
            self.write_u128(val as u128);
        } else {
            // -1 - val without overflows
            let magnitude = !val as u128;
            match u64::try_from(magnitude) {
                Ok(magnitude) => self.write_head(MAJOR_NEGATIVE, magnitude),
                Err(_) => self.write_bignum(3, magnitude),
            }
        }
    }

    /// Writes an integer of any size in the shortest form.
    fn write_bigint(&mut self, val: &BigInt) {
        if let Some(val) = val.to_i128() {
            self.write_i128(val);
        } else if let Some(val) = val.to_u128() {
            self.write_u128(val);
        } else if val.is_negative() {
            // negative bignums hold -1 - n
            let mut magnitude = val.significant_magnitude().to_vec();
            for byte in magnitude.iter_mut().rev() {
                let (value, overflow) = byte.overflowing_sub(1);
                *byte = value;
                if !overflow {
                    break;
                }
            }
            let skip = magnitude.iter().take_while(|&&x| x == 0).count();
            self.write_head(MAJOR_TAG, 3);
            self.write_bytes(&magnitude[skip..]);
        } else {
            self.write_head(MAJOR_TAG, 2);
            self.write_bytes(val.significant_magnitude());
        }
    }

    /// Writes a bignum (tag 2 or 3) with the minimal number of bytes.
    fn write_bignum(&mut self, tag: u64, val: u128) {
        let bytes = val.to_be_bytes();
        let skip = (val.leading_zeros() / 8) as usize;
        self.write_head(MAJOR_TAG, tag);
        self.write_head(MAJOR_BYTES, (bytes.len() - skip) as u64);
        self.out.extend_from_slice(&bytes[skip..]);
    }
}

/// Encodes the head of a data item into the buffer.
#[inline]
fn encode_head(buf: &mut [u8; 9], major: u8, value: u64) -> &[u8] {
    let major = major << 5;
    if value < 24 {
        buf[0] = major | value as u8;
        &buf[..1]
    } else if value <= u64::from(u8::MAX) {
        buf[0] = major | 24;
        buf[1] = value as u8;
        &buf[..2]
    } else if value <= u64::from(u16::MAX) {
        buf[0] = major | 25;
        buf[1..3].copy_from_slice(&(value as u16).to_be_bytes());
        &buf[..3]
    } else if value <= u64::from(u32::MAX) {
        buf[0] = major | 26;
        buf[1..5].copy_from_slice(&(value as u32).to_be_bytes());
        &buf[..5]
    } else {
        buf[0] = major | 27;
        buf[1..9].copy_from_slice(&value.to_be_bytes());
        &buf[..9]
    }
}

impl SerializerConfig {
    /// Creates the default configuration.
    pub const fn new() -> SerializerConfig {
        SerializerConfig { canonical: false }
    }

    /// Enables or disables the deterministic encoding.
    ///
    /// When enabled, the entries of maps are sorted by their encoded keys
    /// (bytewise lexicographic order, RFC 8949 §4.2.1) and duplicate keys
    /// are an error.  This makes the output independent of the iteration
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
    /// starts, for instance to add [`Layer`](deser::ser::Layer)s.
    pub fn to_vec_with<F>(&self, value: &dyn Serialize, setup: F) -> Result<Vec<u8>, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        let mut writer = Writer {
            out: Vec::with_capacity(128),
            canonical: self.canonical,
            frame: None,
            stack: Vec::new(),
            offsets: Vec::new(),
            insertions: Vec::new(),
        };
        driver.drive(|event, state| writer.event(event, state))?;
        if !writer.insertions.is_empty() {
            writer.apply_insertions();
        }
        Ok(writer.out)
    }
}

/// Serializes a value to CBOR.
///
/// This uses the default [`SerializerConfig`].
pub fn to_vec(value: &dyn Serialize) -> Result<Vec<u8>, Error> {
    SerializerConfig::new().to_vec(value)
}
