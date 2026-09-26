use std::fmt;
use std::str::FromStr;

use crate::descriptors::{Descriptor, NamedDescriptor};
use crate::error::Error;
use crate::event::Atom;
use crate::ext::known::{WellKnown, impl_well_known, invalid};
use crate::ext::{BigInt, Extension, Number};

/// An exact decimal number of arbitrary precision.
///
/// This is a well-known extension type (see [`ext`](crate::ext)) which
/// holds the decimal as validated text in the syntax of JSON numbers
/// (`-12.50`, `1e-7`).  The text is kept as is, so trailing zeros (the
/// scale) are retained.  The fallback is the text as string.
///
/// When deserializing, decimals, strings, integers and floats are accepted.
/// [`Number`] extension values (which text formats like JSON emit for
/// floats) are converted exactly.
///
/// ```
/// use deser::ext::Decimal;
///
/// let value: Decimal = "-12.50".parse().unwrap();
/// assert_eq!(value.as_str(), "-12.50");
/// let (mantissa, exponent) = value.to_parts();
/// assert_eq!((mantissa.to_string(), exponent), ("-1250".to_string(), -2));
/// assert_eq!(Decimal::from_parts(&mantissa, exponent), value);
/// ```
///
/// With the `rust_decimal` and `bigdecimal` features the decimal types of
/// these crates serialize as [`Decimal`].
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Decimal(String);

impl Decimal {
    /// Creates a decimal from its text representation.
    ///
    /// The text has to follow the syntax of JSON numbers.
    pub fn new<S: Into<String>>(value: S) -> Result<Decimal, Error> {
        let value = value.into();
        if is_valid_decimal(&value) {
            Ok(Decimal(value))
        } else {
            Err(invalid("invalid decimal"))
        }
    }

    /// Creates a decimal from a mantissa and a base 10 exponent.
    ///
    /// The value is `mantissa * 10^exponent`.
    pub fn from_parts(mantissa: &BigInt, exponent: i64) -> Decimal {
        let mut digits = mantissa.to_string();
        let negative = digits.starts_with('-');
        if negative {
            digits.remove(0);
        }
        let mut rv = String::new();
        if negative {
            rv.push('-');
        }
        let scale = exponent.unsigned_abs();
        if exponent < 0 && scale <= digits.len() as u64 + 20 {
            let scale = scale as usize;
            if digits.len() > scale {
                rv.push_str(&digits[..digits.len() - scale]);
                rv.push('.');
                rv.push_str(&digits[digits.len() - scale..]);
            } else {
                rv.push_str("0.");
                rv.extend(std::iter::repeat_n('0', scale - digits.len()));
                rv.push_str(&digits);
            }
        } else {
            rv.push_str(&digits);
            if exponent != 0 {
                rv.push('e');
                rv.push_str(&exponent.to_string());
            }
        }
        Decimal(rv)
    }

    /// Returns the mantissa and the base 10 exponent.
    ///
    /// The value is `mantissa * 10^exponent`.  Exponents that do not fit
    /// into `i64` saturate.
    pub fn to_parts(&self) -> (BigInt, i64) {
        let s = self.0.as_str();
        let (mantissa, exponent) = match s.find(['e', 'E']) {
            Some(idx) => (&s[..idx], &s[idx + 1..]),
            None => (s, "0"),
        };
        let exponent: i64 =
            exponent
                .trim_start_matches('+')
                .parse()
                .unwrap_or(if exponent.starts_with('-') {
                    i64::MIN
                } else {
                    i64::MAX
                });
        let (digits, scale) = match mantissa.split_once('.') {
            Some((int, frac)) => (format!("{}{}", int, frac), frac.len() as i64),
            None => (mantissa.to_string(), 0),
        };
        // the text was validated
        let mantissa: BigInt = digits.parse().unwrap();
        (mantissa, exponent.saturating_sub(scale))
    }

    /// Returns the text of the decimal.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Converts the decimal into its text.
    pub fn into_string(self) -> String {
        self.0
    }
}

/// Checks the syntax of JSON numbers.
fn is_valid_decimal(s: &str) -> bool {
    crate::ext::number::is_json_number(s)
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Decimal({})", self.0)
    }
}

impl FromStr for Decimal {
    type Err = Error;

    fn from_str(s: &str) -> Result<Decimal, Error> {
        Decimal::new(s)
    }
}

impl From<BigInt> for Decimal {
    fn from(value: BigInt) -> Decimal {
        Decimal(value.to_string())
    }
}

impl From<i64> for Decimal {
    fn from(value: i64) -> Decimal {
        Decimal(value.to_string())
    }
}

impl From<u64> for Decimal {
    fn from(value: u64) -> Decimal {
        Decimal(value.to_string())
    }
}

impl TryFrom<f64> for Decimal {
    type Error = Error;

    /// Converts a float with the shortest representation that roundtrips.
    fn try_from(value: f64) -> Result<Decimal, Error> {
        if value.is_finite() {
            Decimal::new(format!("{:?}", value))
        } else {
            Err(invalid("decimals cannot be infinite or NaN"))
        }
    }
}

static DECIMAL_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "Decimal" };

impl Extension for Decimal {
    fn name(&self) -> &str {
        "decimal"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::Str(self.0.as_str().into())
    }
}

impl WellKnown for Decimal {
    const EXPECTING: &'static str = "decimal";

    fn descriptor() -> &'static dyn Descriptor {
        &DECIMAL_DESCRIPTOR
    }

    /// Accepts decimals, strings, integers and floats.
    fn from_atom(atom: &Atom) -> Result<Option<Decimal>, Error> {
        Ok(Some(match *atom {
            Atom::Ext(ref ext) => {
                if let Some(value) = ext.downcast_ref::<Decimal>() {
                    value.clone()
                } else if let Some(value) = ext.downcast_value_ref::<Number>() {
                    // numbers use the same syntax as decimals
                    Decimal(value.as_str().to_string())
                } else if let Some(value) = BigInt::from_atom(atom)? {
                    Decimal::from(value)
                } else {
                    return Ok(None);
                }
            }
            Atom::Str(ref value) => value.parse()?,
            Atom::U64(value) => Decimal::from(value),
            Atom::I64(value) => Decimal::from(value),
            Atom::F64(value) => Decimal::try_from(value)?,
            _ => return Ok(None),
        }))
    }
}

impl_well_known!(Decimal);

#[test]
fn test_decimal() {
    for valid in ["0", "-0", "1.5", "-12.50", "1e5", "1E+5", "1.5e-7", "0.001"] {
        assert_eq!(valid.parse::<Decimal>().unwrap().as_str(), valid);
    }
    for invalid in [
        "", "-", "01", "1.", ".5", "+1", "1e", "1e+", "NaN", "1.5.5", " 1",
    ] {
        assert!(invalid.parse::<Decimal>().is_err(), "{}", invalid);
    }
    let parts = |s: &str| {
        let (mantissa, exponent) = s.parse::<Decimal>().unwrap().to_parts();
        (mantissa.to_string(), exponent)
    };
    assert_eq!(parts("12.50"), ("1250".into(), -2));
    assert_eq!(parts("-0.001"), ("-1".into(), -3));
    assert_eq!(parts("1.5e10"), ("15".into(), 9));
    assert_eq!(parts("7"), ("7".into(), 0));
    let from_parts = |m: i64, e: i64| Decimal::from_parts(&BigInt::from(m), e).into_string();
    assert_eq!(from_parts(1250, -2), "12.50");
    assert_eq!(from_parts(-1, -3), "-0.001");
    assert_eq!(from_parts(15, 9), "15e9");
    assert_eq!(from_parts(7, 0), "7");
    assert_eq!(from_parts(1, -40), "1e-40");
    assert_eq!(Decimal::try_from(0.1).unwrap().as_str(), "0.1");
    assert_eq!(Decimal::try_from(1e100).unwrap().as_str(), "1e100");
    assert!(Decimal::try_from(f64::NAN).is_err());
}
