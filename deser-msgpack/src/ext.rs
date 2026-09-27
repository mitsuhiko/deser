use std::borrow::Cow;

use deser_core::State;
use deser_core::de::{Deserialize, Sink, SinkHandle};
use deser_core::ext::{ExtValue, Extension, Timestamp};
use deser_core::ser::{Chunk, Serialize};
use deser_core::{Atom, Bytes, Error};

/// The extension type of timestamps.
pub(crate) const TIMESTAMP: i8 = -1;

/// A MessagePack extension value: a type and binary data.
///
/// Extensions whose type the format does not understand are passed through
/// deser as extension atoms of this type.  Their fallback is the binary
/// data, so an extension deserializes into `Vec<u8>` if the type is not of
/// interest.  Values of this type are written as extensions.
///
/// ```
/// use deser_msgpack::Ext;
///
/// let bytes = deser_msgpack::to_vec(&Ext::new(7, vec![1, 2, 3])).unwrap();
/// assert_eq!(bytes, [0xc7, 0x03, 0x07, 0x01, 0x02, 0x03]);
/// let value: Ext = deser_msgpack::from_slice(&bytes).unwrap();
/// assert_eq!(value, Ext::new(7, vec![1, 2, 3]));
/// ```
///
/// Timestamps (type `-1`) are converted to and from
/// [`Timestamp`](deser_core::ext::Timestamp) instead.  They are still
/// accepted by this type which then holds their encoding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ext {
    /// The type of the extension (`-128` to `-1` are reserved by the
    /// specification).
    pub kind: i8,
    /// The data of the extension.
    pub data: Vec<u8>,
}

impl Ext {
    /// Creates an extension value.
    pub fn new(kind: i8, data: impl Into<Vec<u8>>) -> Ext {
        Ext {
            kind,
            data: data.into(),
        }
    }
}

impl Extension for Ext {
    fn name(&self) -> &str {
        "msgpack extension"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::Bytes(Bytes::borrowed(&self.data))
    }
}

impl Serialize for Ext {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(self))))
    }
}

impl<'de> Deserialize<'de> for Ext {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(ExtSink(out))
    }
}

struct ExtSink<'a>(&'a mut Option<Ext>);

impl<'a, 'de> Sink<'de> for ExtSink<'a> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("msgpack extension")
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let value = match atom {
            Atom::Ext(ref ext) => {
                if let Some(value) = ext.downcast_ref::<Ext>() {
                    value.clone()
                } else if let Some(value) = ext.downcast_ref::<Timestamp>() {
                    let mut buf = [0; 12];
                    Ext::new(TIMESTAMP, encode_timestamp(value, &mut buf))
                } else {
                    return self.unexpected_atom(atom, state);
                }
            }
            other => return self.unexpected_atom(other, state),
        };
        *self.0 = Some(value);
        Ok(())
    }
}

/// Decodes the data of a timestamp extension.
///
/// Returns `None` if the data is not a valid timestamp.
pub(crate) fn decode_timestamp(data: &[u8]) -> Option<Timestamp> {
    let (seconds, nanosecond) = match data.len() {
        // timestamp 32: seconds
        4 => (i64::from(u32::from_be_bytes(data.try_into().unwrap())), 0),
        // timestamp 64: 30 bits nanoseconds, 34 bits seconds
        8 => {
            let value = u64::from_be_bytes(data.try_into().unwrap());
            ((value & 0x3_ffff_ffff) as i64, (value >> 34) as u32)
        }
        // timestamp 96: 32 bits nanoseconds, 64 bits signed seconds
        12 => (
            i64::from_be_bytes(data[4..].try_into().unwrap()),
            u32::from_be_bytes(data[..4].try_into().unwrap()),
        ),
        _ => return None,
    };
    (nanosecond < 1_000_000_000).then_some(Timestamp {
        seconds,
        nanosecond,
    })
}

/// Encodes a timestamp in the smallest format.
pub(crate) fn encode_timestamp<'b>(value: &Timestamp, buf: &'b mut [u8; 12]) -> &'b [u8] {
    if value.seconds >> 34 == 0 {
        if value.nanosecond == 0 && value.seconds <= i64::from(u32::MAX) {
            buf[..4].copy_from_slice(&(value.seconds as u32).to_be_bytes());
            &buf[..4]
        } else {
            let packed = (u64::from(value.nanosecond) << 34) | value.seconds as u64;
            buf[..8].copy_from_slice(&packed.to_be_bytes());
            &buf[..8]
        }
    } else {
        buf[..4].copy_from_slice(&value.nanosecond.to_be_bytes());
        buf[4..].copy_from_slice(&value.seconds.to_be_bytes());
        &buf[..]
    }
}

#[test]
fn test_timestamp_codec() {
    for (seconds, nanosecond, len) in [
        (0, 0, 4),
        (u32::MAX as i64, 0, 4),
        (u32::MAX as i64 + 1, 0, 8),
        (0, 1, 8),
        ((1 << 34) - 1, 999_999_999, 8),
        (1 << 34, 0, 12),
        (-1, 0, 12),
        (i64::MIN, 999_999_999, 12),
        (i64::MAX, 0, 12),
    ] {
        let value = Timestamp {
            seconds,
            nanosecond,
        };
        let mut buf = [0; 12];
        let data = encode_timestamp(&value, &mut buf);
        assert_eq!(data.len(), len, "{:?}", value);
        assert_eq!(decode_timestamp(data), Some(value));
    }
    // nanoseconds out of range
    assert_eq!(decode_timestamp(&[0xee, 0x6b, 0x28, 0, 0, 0, 0, 0]), None);
    assert_eq!(
        decode_timestamp(&[0x3b, 0x9a, 0xca, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        None
    );
    assert_eq!(decode_timestamp(&[0, 0, 0]), None);
}
