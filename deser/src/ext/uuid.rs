use std::fmt;
use std::str::FromStr;

use crate::descriptors::{Descriptor, NamedDescriptor};
use crate::error::Error;
use crate::event::Atom;
use crate::ext::known::{impl_well_known, invalid, WellKnown};
use crate::ext::Extension;

/// A UUID.
///
/// This is a well-known extension type (see [`ext`](crate::ext)) which
/// holds the 16 bytes of the UUID.  The fallback is the hyphenated
/// representation as string (`67e55044-10b1-426f-9247-bb680e5fe0c8`).
///
/// When deserializing, the hyphenated and the simple representation (32
/// hex digits) as well as 16 bytes are accepted.
///
/// ```
/// use deser::ext::Uuid;
///
/// let uuid: Uuid = "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap();
/// assert_eq!(uuid.0[0], 0x67);
/// assert_eq!(uuid.to_string(), "67e55044-10b1-426f-9247-bb680e5fe0c8");
/// ```
///
/// With the `uuid` feature, `uuid::Uuid` serializes as [`Uuid`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Uuid(pub [u8; 16]);

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (idx, byte) in self.0.iter().enumerate() {
            if matches!(idx, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uuid({})", self)
    }
}

impl FromStr for Uuid {
    type Err = Error;

    fn from_str(s: &str) -> Result<Uuid, Error> {
        let bytes = s.as_bytes();
        let hyphenated = bytes.len() == 36;
        if !(hyphenated || bytes.len() == 32) {
            return Err(invalid("invalid UUID"));
        }
        let mut rv = [0u8; 16];
        let mut pos = 0;
        for (idx, byte) in rv.iter_mut().enumerate() {
            if hyphenated && matches!(idx, 4 | 6 | 8 | 10) {
                if bytes[pos] != b'-' {
                    return Err(invalid("invalid UUID"));
                }
                pos += 1;
            }
            let hex = |c: u8| {
                (c as char)
                    .to_digit(16)
                    .ok_or_else(|| invalid("invalid UUID"))
            };
            *byte = (hex(bytes[pos])? << 4 | hex(bytes[pos + 1])?) as u8;
            pos += 2;
        }
        Ok(Uuid(rv))
    }
}

static UUID_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "Uuid" };

impl Extension for Uuid {
    fn name(&self) -> &str {
        "uuid"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::Str(self.to_string().into())
    }
}

impl WellKnown for Uuid {
    const EXPECTING: &'static str = "uuid";

    fn descriptor() -> &'static dyn Descriptor {
        &UUID_DESCRIPTOR
    }

    /// Accepts UUIDs, strings and 16 bytes.
    fn from_atom(atom: &Atom) -> Result<Option<Uuid>, Error> {
        Ok(Some(match *atom {
            Atom::Ext(ref ext) => match ext.downcast_ref::<Uuid>() {
                Some(value) => *value,
                None => return Ok(None),
            },
            Atom::Str(ref value) => value.parse()?,
            Atom::Bytes(ref value) => Uuid(
                value
                    .as_ref()
                    .try_into()
                    .map_err(|_| invalid("invalid UUID, expected 16 bytes"))?,
            ),
            _ => return Ok(None),
        }))
    }
}

impl_well_known!(Uuid);

#[test]
fn test_uuid() {
    let uuid: Uuid = "67E55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap();
    assert_eq!(uuid.to_string(), "67e55044-10b1-426f-9247-bb680e5fe0c8");
    let simple: Uuid = "67e5504410b1426f9247bb680e5fe0c8".parse().unwrap();
    assert_eq!(simple, uuid);
    for invalid in [
        "",
        "67e55044-10b1-426f-9247-bb680e5fe0c",
        "67e55044x10b1-426f-9247-bb680e5fe0c8",
        "67e55044-10b1-426f-9247-bb680e5fe0cg",
        "67e5504410b1426f9247bb680e5fe0c8aaaa",
    ] {
        assert!(invalid.parse::<Uuid>().is_err());
    }
}
