use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::descriptors::{Descriptor, NamedDescriptor};
use crate::error::Error;
use crate::event::Atom;
use crate::ext::known::{impl_well_known, invalid, WellKnown};
use crate::ext::Extension;

/// An integer of arbitrary size.
///
/// This is a well-known extension type (see [`ext`](crate::ext)) for
/// integers that do not fit into 128 bits.  It holds the sign and the
/// magnitude as big-endian bytes.  Leading zero bytes are permitted, zero is
/// never negative.
///
/// The fallback is the decimal representation as string.  Integers that
/// fit into 64 or 128 bits are not represented as [`BigInt`]: they are
/// passed through deser as `U64`, `I64`, `u128` or `i128` instead.  That is
/// what the `num-bigint` support does.  When deserializing, all of these as
/// well as strings are accepted.
///
/// ```
/// use deser::ext::BigInt;
///
/// let value: BigInt = "-340282366920938463463374607431768211456".parse().unwrap();
/// assert!(value.negative);
/// assert_eq!(value.magnitude, [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
/// assert_eq!(value.to_string(), "-340282366920938463463374607431768211456");
/// ```
///
/// Values are compared and hashed by their numeric value.
#[derive(Clone, Default)]
pub struct BigInt {
    /// `true` if the integer is negative.
    pub negative: bool,
    /// The magnitude as big-endian bytes.
    pub magnitude: Vec<u8>,
}

impl BigInt {
    /// Creates a big integer from an `i128`.
    pub fn from_i128(value: i128) -> BigInt {
        let mut rv = BigInt::from_u128(value.unsigned_abs());
        rv.negative = value < 0;
        rv
    }

    /// Creates a big integer from an `u128`.
    pub fn from_u128(value: u128) -> BigInt {
        let bytes = value.to_be_bytes();
        let skip = (value.leading_zeros() / 8) as usize;
        BigInt {
            negative: false,
            magnitude: bytes[skip..].to_vec(),
        }
    }

    /// Returns the magnitude without leading zero bytes.
    pub fn significant_magnitude(&self) -> &[u8] {
        let skip = self.magnitude.iter().take_while(|&&x| x == 0).count();
        &self.magnitude[skip..]
    }

    /// Returns `true` if the value is zero.
    pub fn is_zero(&self) -> bool {
        self.significant_magnitude().is_empty()
    }

    /// Returns `true` if the value is negative (and not zero).
    pub fn is_negative(&self) -> bool {
        self.negative && !self.is_zero()
    }

    /// Returns the magnitude as `u128` if it fits.
    fn magnitude_u128(&self) -> Option<u128> {
        let significant = self.significant_magnitude();
        if significant.len() > 16 {
            return None;
        }
        let mut buf = [0u8; 16];
        buf[16 - significant.len()..].copy_from_slice(significant);
        Some(u128::from_be_bytes(buf))
    }

    /// Returns the value as `u128` if it fits.
    pub fn to_u128(&self) -> Option<u128> {
        if self.is_negative() {
            None
        } else {
            self.magnitude_u128()
        }
    }

    /// Returns the value as `i128` if it fits.
    pub fn to_i128(&self) -> Option<i128> {
        let magnitude = self.magnitude_u128()?;
        if self.is_negative() {
            0i128.checked_sub_unsigned(magnitude)
        } else {
            i128::try_from(magnitude).ok()
        }
    }

    /// Converts the value into the smallest atom that can hold it.
    ///
    /// This is `U64` or `I64` if the value fits into 64 bits, an `u128` or
    /// `i128` extension value if it fits into 128 bits and a [`BigInt`]
    /// extension value otherwise.
    pub fn into_atom(self) -> Atom<'static> {
        use crate::ext::ExtValue;
        if let Some(value) = self.to_u128() {
            match u64::try_from(value) {
                Ok(value) => Atom::U64(value),
                Err(_) => Atom::Ext(ExtValue::owned(value)),
            }
        } else if let Some(value) = self.to_i128() {
            match i64::try_from(value) {
                Ok(value) => Atom::I64(value),
                Err(_) => Atom::Ext(ExtValue::owned(value)),
            }
        } else {
            Atom::Ext(ExtValue::owned(self))
        }
    }
}

impl PartialEq for BigInt {
    fn eq(&self, other: &BigInt) -> bool {
        self.is_negative() == other.is_negative()
            && self.significant_magnitude() == other.significant_magnitude()
    }
}

impl Eq for BigInt {}

impl std::hash::Hash for BigInt {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.is_negative().hash(state);
        self.significant_magnitude().hash(state);
    }
}

impl PartialOrd for BigInt {
    fn partial_cmp(&self, other: &BigInt) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigInt {
    fn cmp(&self, other: &BigInt) -> Ordering {
        let (a, b) = (self.significant_magnitude(), other.significant_magnitude());
        let magnitude = a.len().cmp(&b.len()).then_with(|| a.cmp(b));
        match (self.is_negative(), other.is_negative()) {
            (false, false) => magnitude,
            (true, true) => magnitude.reverse(),
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
        }
    }
}

impl From<i128> for BigInt {
    fn from(value: i128) -> BigInt {
        BigInt::from_i128(value)
    }
}

impl From<u128> for BigInt {
    fn from(value: u128) -> BigInt {
        BigInt::from_u128(value)
    }
}

impl From<i64> for BigInt {
    fn from(value: i64) -> BigInt {
        BigInt::from_i128(value.into())
    }
}

impl From<u64> for BigInt {
    fn from(value: u64) -> BigInt {
        BigInt::from_u128(value.into())
    }
}

impl fmt::Display for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // repeatedly divide by 10^9 and collect the remainders
        let mut value = self.significant_magnitude().to_vec();
        let mut chunks = Vec::new();
        while !value.is_empty() {
            let mut remainder = 0u64;
            for byte in value.iter_mut() {
                let current = remainder << 8 | u64::from(*byte);
                *byte = (current / 1_000_000_000) as u8;
                remainder = current % 1_000_000_000;
            }
            chunks.push(remainder as u32);
            let skip = value.iter().take_while(|&&x| x == 0).count();
            value.drain(..skip);
        }
        if self.is_negative() {
            f.write_str("-")?;
        }
        match chunks.pop() {
            None => f.write_str("0"),
            Some(first) => {
                write!(f, "{}", first)?;
                for chunk in chunks.iter().rev() {
                    write!(f, "{:09}", chunk)?;
                }
                Ok(())
            }
        }
    }
}

impl fmt::Debug for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BigInt({})", self)
    }
}

impl FromStr for BigInt {
    type Err = Error;

    /// Parses a decimal integer with an optional sign.
    fn from_str(s: &str) -> Result<BigInt, Error> {
        let (negative, digits) = match s.as_bytes().first() {
            Some(b'-') => (true, &s[1..]),
            Some(b'+') => (false, &s[1..]),
            _ => (false, s),
        };
        if digits.is_empty() || !digits.bytes().all(|x| x.is_ascii_digit()) {
            return Err(invalid("invalid integer"));
        }
        // multiply by 10^9 and add, working on chunks of nine digits
        let mut magnitude: Vec<u8> = Vec::new();
        let first = digits.len() % 9;
        let chunks = (first != 0).then(|| &digits[..first]).into_iter().chain(
            digits.as_bytes()[first..].chunks(9).map(|x| {
                // the digits are ASCII
                std::str::from_utf8(x).unwrap()
            }),
        );
        for chunk in chunks {
            let factor = 10u64.pow(chunk.len() as u32);
            let mut carry: u64 = chunk.parse().unwrap();
            for byte in magnitude.iter_mut().rev() {
                let current = u64::from(*byte) * factor + carry;
                *byte = current as u8;
                carry = current >> 8;
            }
            while carry != 0 {
                magnitude.insert(0, carry as u8);
                carry >>= 8;
            }
        }
        let rv = BigInt {
            negative,
            magnitude,
        };
        Ok(BigInt {
            negative: rv.is_negative(),
            ..rv
        })
    }
}

static BIGINT_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "BigInt" };

impl Extension for BigInt {
    fn name(&self) -> &str {
        "big integer"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::Str(self.to_string().into())
    }
}

impl WellKnown for BigInt {
    const EXPECTING: &'static str = "integer";

    fn descriptor() -> &'static dyn Descriptor {
        &BIGINT_DESCRIPTOR
    }

    /// Accepts integers of all sizes and strings.
    fn from_atom(atom: &Atom) -> Result<Option<BigInt>, Error> {
        Ok(Some(match *atom {
            Atom::Ext(ref ext) => {
                if let Some(value) = ext.downcast_ref::<BigInt>() {
                    value.clone()
                } else if let Some(&value) = ext.downcast_ref::<u128>() {
                    BigInt::from(value)
                } else if let Some(&value) = ext.downcast_ref::<i128>() {
                    BigInt::from(value)
                } else if let Some(value) = ext
                    .downcast_value_ref::<crate::ext::Number>()
                    .filter(|x| x.is_integer())
                {
                    // integer literals that do not fit into 128 bits
                    value.as_str().parse()?
                } else {
                    return Ok(None);
                }
            }
            Atom::U64(value) => BigInt::from(value),
            Atom::I64(value) => BigInt::from(value),
            Atom::Str(ref value) => value.parse()?,
            _ => return Ok(None),
        }))
    }
}

impl_well_known!(BigInt);

#[test]
fn test_bigint() {
    for s in [
        "0",
        "1",
        "-1",
        "255",
        "256",
        "999999999",
        "1000000000",
        "-18446744073709551616",
        "340282366920938463463374607431768211456",
        "-123456789012345678901234567890123456789012345678901234567890",
    ] {
        let value: BigInt = s.parse().unwrap();
        assert_eq!(value.to_string(), s);
    }
    assert_eq!("-0".parse::<BigInt>().unwrap().to_string(), "0");
    assert_eq!("+007".parse::<BigInt>().unwrap().to_string(), "7");
    assert_eq!(BigInt::from(i128::MIN).to_i128(), Some(i128::MIN));
    assert_eq!(BigInt::from(u128::MAX).to_u128(), Some(u128::MAX));
    assert_eq!(BigInt::from(u128::MAX).to_i128(), None);
    assert_eq!(BigInt::from(-1i64).to_u128(), None);
    assert_eq!(BigInt::from(-1i64).into_atom(), Atom::I64(-1));
    for invalid in ["", "-", "1.0", "1e5", " 1", "0x10"] {
        assert!(invalid.parse::<BigInt>().is_err());
    }
    let n = |s: &str| s.parse::<BigInt>().unwrap();
    assert_eq!(
        BigInt {
            negative: true,
            magnitude: vec![0, 0]
        },
        n("0")
    );
    assert_eq!(
        BigInt {
            negative: false,
            magnitude: vec![0, 1]
        },
        n("1")
    );
    let mut values = vec![n("256"), n("-1"), n("0"), n("-256"), n("255"), n("1")];
    values.sort();
    assert_eq!(
        values,
        [n("-256"), n("-1"), n("0"), n("1"), n("255"), n("256")]
    );
}
