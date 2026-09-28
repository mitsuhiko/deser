use alloc::boxed::Box;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::mem::ManuallyDrop;

use deser_core::__format::extend;
use deser_core::State;
use deser_core::ext::{BigInt, Datetime, Decimal, ExtValue, Timestamp, Uuid};
use deser_core::ser::{self, PausableSink, SerializeDriver, Written};
use deser_core::{Atom, ContainerShape, Error, ErrorKind, Event, Serialize};

use crate::float::f32_to_f16;
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
/// const CANONICAL: SerializerConfig =
///     SerializerConfig::new().canonical(true);
/// let map = HashMap::from([("b", 1), ("a", 2)]);
/// assert_eq!(CANONICAL.to_vec(&map).unwrap(), b"\xa2\x61a\x02\x61b\x01");
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
pub(crate) struct Writer {
    pub(crate) out: Vec<u8>,
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
    // the number of open containers whose length is patched in at the end
    open_unknown: usize,
    // the output is passed on once it's this long (see `PausableSink`)
    limit: usize,
}

/// The bytes of a container header that did not fit into the space
/// reserved for it.
struct Insertion {
    offset: usize,
    len: u8,
    bytes: [u8; 8],
}

impl ser::EventSink for Writer {
    #[inline(always)]
    fn event(&mut self, event: Event, state: &mut State) -> Result<(), Error> {
        Writer::event(self, event, state)
    }
}

impl PausableSink for Writer {
    #[inline(always)]
    fn event(
        &mut self,
        event: Event<'_>,
        _value: &dyn Serialize,
        state: &mut State,
    ) -> Result<(), Error> {
        Writer::event(self, event, state)
    }

    #[inline]
    fn pause(&mut self) -> bool {
        // the output is final once no length has to be patched in and no
        // map has to be sorted
        if self.out.len() < self.limit || self.open_unknown > 0 || !self.maps.is_empty() {
            return false;
        }
        self.finish();
        true
    }
}

impl Writer {
    /// Creates a writer that writes into the output.
    pub(crate) fn new(canonical: bool, out: Vec<u8>) -> Writer {
        Writer {
            out,
            canonical,
            frame: Frame::TOP,
            stack: Vec::new(),
            maps: Vec::new(),
            offsets: Vec::new(),
            insertions: Vec::new(),
            open_unknown: 0,
            limit: usize::MAX,
        }
    }

    /// Makes the output final once no container is open whose length is
    /// patched in or which is sorted.
    pub(crate) fn finish(&mut self) {
        if !self.insertions.is_empty() {
            self.apply_insertions();
            self.insertions.clear();
        }
    }

    /// Writes the events of the driver.
    ///
    /// Returns `false` if the driver was paused as the output holds at
    /// least `limit` bytes that are final.  With a limit of `usize::MAX`
    /// the value is written at once.
    pub(crate) fn drive(
        &mut self,
        driver: &mut SerializeDriver<'_>,
        limit: usize,
    ) -> Result<bool, Error> {
        let done = if limit == usize::MAX {
            driver.drive_sink(self)?;
            true
        } else {
            self.limit = limit;
            driver.drive_until(self)?
        };
        if done {
            self.finish();
        }
        Ok(done)
    }

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
        self.frame.remaining = self.frame.remaining.wrapping_sub(1);
        if self.canonical && self.frame.is_map() {
            self.offsets.push(self.out.len());
        }
        if let Some(tags) = state.event::<Tags>() {
            self.write_tags(tags);
        }
    }

    /// Writes the tags attached to the current event.
    #[cold]
    fn write_tags(&mut self, tags: &Tags) {
        for &tag in tags.0.iter() {
            self.write_head(MAJOR_TAG, tag);
        }
    }

    #[inline(always)]
    fn start(&mut self, is_map: bool, shape: ContainerShape, state: &State) -> Result<(), Error> {
        self.begin_item(state);
        let header = self.out.len();
        let major = if is_map { MAJOR_MAP } else { MAJOR_ARRAY };
        let mut info = (header << 2) | if is_map { IS_MAP } else { 0 };
        // with a known length the header is written right away, otherwise a
        // byte is reserved and the length is patched in at the end.
        let remaining = match shape.len() {
            Some(len) => {
                self.write_head(major, len as u64);
                if is_map { len as u64 * 2 } else { len as u64 }
            }
            None => {
                self.out.push(major << 5);
                info |= UNKNOWN_LEN;
                self.open_unknown += 1;
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
            self.open_unknown -= 1;
            let count = if frame.is_map() { items / 2 } else { items };
            self.patch_length(frame.header(), count);
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
            Atom::Str(ref val) if val.is_borrowed() => self.write_str(val),
            Atom::Bytes(ref val) if val.is_borrowed() => self.write_bytes(val),
            Atom::Char(c) => self.write_str(c.encode_utf8(&mut [0u8; 4])),
            Atom::U64(val) => self.write_head(MAJOR_UNSIGNED, val),
            Atom::I64(val) => self.write_i64(val),
            Atom::F64(val) => self.write_f64(val),
            Atom::F32(val) => self.write_f32(val),
            _ => return self.write_other_atom(ManuallyDrop::into_inner(atom)),
        }
        Ok(())
    }

    #[inline(never)]
    fn write_other_atom(&mut self, atom: Atom) -> Result<(), Error> {
        match atom {
            Atom::Str(ref val) | Atom::Lexical(ref val) => self.write_str(val),
            Atom::Bytes(ref val) => self.write_bytes(val),
            Atom::Ext(ref ext) => return self.write_ext(ext),
            // values whose type was inferred from text are written as value
            Atom::Implicit(ref val) => return self.write_atom(val.value().to_atom()),
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
    #[inline]
    fn write_f64(&mut self, val: f64) {
        // a value that fits into a half fits into a single
        if f64::from(val as f32) == val {
            self.write_f32(val as f32);
        } else if val.is_nan() {
            self.write_nan();
        } else {
            let mut buf = [0xfb; 9];
            buf[1..].copy_from_slice(&val.to_be_bytes());
            extend(&mut self.out, &buf);
        }
    }

    /// Writes a single precision float in the shortest form that preserves
    /// its value.
    #[inline]
    fn write_f32(&mut self, val: f32) {
        if let Some(half) = f32_to_f16(val) {
            let [a, b] = half.to_be_bytes();
            extend(&mut self.out, &[0xf9, a, b]);
        } else if val.is_nan() {
            self.write_nan();
        } else {
            let mut buf = [0xfa; 5];
            buf[1..].copy_from_slice(&val.to_be_bytes());
            extend(&mut self.out, &buf);
        }
    }

    /// Writes NaN, always as the canonical half precision NaN.
    #[cold]
    fn write_nan(&mut self) {
        self.out.extend_from_slice(&[0xf9, 0x7e, 0x00]);
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
    /// starts, for instance to add [`Layer`](deser_core::ser::Layer)s.
    pub fn to_vec_with<F>(&self, value: &dyn Serialize, setup: F) -> Result<Vec<u8>, Error>
    where
        F: FnOnce(&mut SerializeDriver<'_>),
    {
        let mut driver = SerializeDriver::new(value);
        setup(&mut driver);
        self.serialize_driver(&mut driver)
    }

    /// Serializes (a part of) the value of a driver and appends the output
    /// that is final.
    ///
    /// The progress of the value is kept in `item` (see
    /// `StreamSerializer::drive_partial`), `true` is returned once the
    /// value is complete.  If this fails, what was appended by the call is
    /// removed from the output.
    pub(crate) fn serialize_part(
        &self,
        item: &mut Option<Box<Writer>>,
        driver: &mut SerializeDriver<'_>,
        out: &mut Vec<u8>,
        limit: usize,
    ) -> Result<bool, Error> {
        let len = out.len();
        // a value that is written at once is written into the output
        // directly without boxing the writer
        if item.is_none() && limit == usize::MAX {
            let mut writer = Writer::new(self.canonical, core::mem::take(out));
            let rv = writer.drive(driver, usize::MAX);
            *out = writer.out;
            if rv.is_err() {
                out.truncate(len);
            }
            return rv;
        }
        // the writer writes into an empty output directly, otherwise its
        // output is appended
        let adopt = out.is_empty();
        let mut writer = item
            .take()
            .unwrap_or_else(|| Box::new(Writer::new(self.canonical, Vec::new())));
        if adopt {
            writer.out = core::mem::take(out);
        }
        // after an error the value is abandoned, its writer is dropped
        let rv = writer.drive(driver, limit);
        let output = core::mem::take(&mut writer.out);
        if adopt {
            *out = output;
        } else if rv.is_ok() {
            out.extend_from_slice(&output);
        }
        let done = match rv {
            Ok(done) => done,
            Err(err) => {
                out.truncate(len);
                return Err(err);
            }
        };
        if !done {
            *item = Some(writer);
        }
        Ok(done)
    }

    /// Serializes the value of a driver.
    pub(crate) fn serialize_driver(
        &self,
        driver: &mut SerializeDriver<'_>,
    ) -> Result<Vec<u8>, Error> {
        let mut writer = Writer::new(self.canonical, Vec::with_capacity(128));
        writer.drive(driver, usize::MAX)?;
        Ok(writer.out)
    }
}

/// Serializes values into CBOR.
///
/// Every call to [`serialize`](Self::serialize) writes a data item, the
/// items follow each other which makes the output a [CBOR
/// sequence](https://www.rfc-editor.org/rfc/rfc8742).
///
/// ```
/// use deser_cbor::Serializer;
///
/// let mut serializer = Serializer::new();
/// serializer.serialize(&1u32).unwrap();
/// serializer.serialize(&"hi").unwrap();
/// assert_eq!(serializer.finish(), [0x01, 0x62, b'h', b'i']);
/// ```
///
/// The serializer is also the stream serializer of CBOR (see
/// [`StreamSerializer`](ser::StreamSerializer)): the output can be taken
/// while values are written, and large values can be written in parts.
/// The output of arrays and maps whose length is not known upfront (and of
/// maps in canonical mode) is held back until they are complete, as their
/// header or the order of their entries is only known then.  To write to
/// a [`Write`](std::io::Write) use [`SerializerConfig::writer`].
pub struct Serializer {
    config: SerializerConfig,
    out: Vec<u8>,
    written: usize,
    // the value that is written in parts
    item: Option<Box<Writer>>,
    // a value was started with `drive_partial` and is not complete
    in_progress: bool,
}

impl Default for Serializer {
    fn default() -> Serializer {
        Serializer::new()
    }
}

impl Clone for Serializer {
    /// Clones the serializer.
    ///
    /// The clone of a serializer that writes a value in parts cannot write
    /// more values (see
    /// [`StreamSerializer::in_progress`](ser::StreamSerializer::in_progress)).
    fn clone(&self) -> Serializer {
        Serializer {
            config: self.config.clone(),
            out: self.out.clone(),
            written: self.written,
            item: None,
            in_progress: self.in_progress,
        }
    }
}

impl core::fmt::Debug for Serializer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Serializer")
            .field("config", &self.config)
            .field("output", &self.out)
            .field("written", &self.written)
            .field("in_progress", &self.in_progress)
            .finish()
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
            item: None,
            in_progress: false,
        }
    }

    /// Returns the configuration.
    pub fn config(&self) -> &SerializerConfig {
        &self.config
    }

    /// Returns the number of values that were written.
    pub fn written(&self) -> usize {
        self.written
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

    /// Returns the output written so far (that was not cleared).
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
        // only `drive_partial` continues a value
        if self.in_progress {
            return Err(Error::in_progress());
        }
        ser::StreamSerializer::drive_partial(self, driver, usize::MAX).map(|_| ())
    }
}

impl ser::StreamSerializer for Serializer {
    fn output(&self) -> &[u8] {
        &self.out
    }

    fn clear_output(&mut self) {
        self.out.clear();
    }

    fn supports_partial(&self) -> bool {
        true
    }

    fn drive_partial(
        &mut self,
        driver: &mut SerializeDriver<'_>,
        limit: usize,
    ) -> Result<Written, Error> {
        if self.item.is_none() && self.in_progress {
            return Err(Error::in_progress());
        }
        // the parts of a value that failed stay written (see
        // `in_progress`)
        if !self
            .config
            .serialize_part(&mut self.item, driver, &mut self.out, limit)?
        {
            self.in_progress = true;
            return Ok(Written::Partial);
        }
        self.in_progress = false;
        self.written += 1;
        Ok(Written::Done)
    }

    fn in_progress(&self) -> bool {
        self.in_progress
    }
}

#[cfg(feature = "io")]
impl SerializerConfig {
    /// Creates a writer of CBOR data items to a stream
    /// (see [`deser::io::Writer`](deser_core::io::Writer)).
    ///
    /// The items follow each other which makes the stream a [CBOR
    /// sequence](https://www.rfc-editor.org/rfc/rfc8742).
    ///
    /// ```
    /// use deser_cbor::SerializerConfig;
    ///
    /// let mut writer = SerializerConfig::new().writer(Vec::new());
    /// writer.write(&1u32).unwrap();
    /// writer.write(&"hi").unwrap();
    /// assert_eq!(writer.into_inner(), [0x01, 0x62, b'h', b'i']);
    /// ```
    pub fn writer<W: std::io::Write>(&self, writer: W) -> deser_core::io::Writer<W, Serializer> {
        deser_core::io::Writer::new(writer, Serializer::with_config(self))
    }

    /// Serializes a value to a writer.
    ///
    /// See [`to_writer`](crate::to_writer).
    pub fn to_writer<W: std::io::Write>(
        &self,
        writer: W,
        value: &dyn Serialize,
    ) -> Result<(), Error> {
        self.writer(writer).write(value)
    }
}

/// Serializes a value to a writer.
///
/// The output of large values is written in pieces while they are
/// serialized (see [`deser::io`](deser_core::io)), the writer does not need to be
/// buffered.
///
/// ```
/// let mut out = Vec::new();
/// deser_cbor::to_writer(&mut out, &vec![1u32, 2]).unwrap();
/// assert_eq!(out, [0x82, 0x01, 0x02]);
/// ```
#[cfg(feature = "io")]
pub fn to_writer<W: std::io::Write>(writer: W, value: &dyn Serialize) -> Result<(), Error> {
    SerializerConfig::new().to_writer(writer, value)
}

/// Serializes a value to CBOR.
///
/// This uses the default [`SerializerConfig`].
pub fn to_vec(value: &dyn Serialize) -> Result<Vec<u8>, Error> {
    SerializerConfig::new().to_vec(value)
}
