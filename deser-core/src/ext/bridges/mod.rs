//! Serialization of the types of the standard library and other crates as
//! well-known types.

/// Implements conversions between `num_bigint::BigInt` and [`BigInt`] for
/// a path to the `num_bigint` crate.
#[cfg(any(feature = "bigdecimal", feature = "num-bigint"))]
macro_rules! num_bigint_conversions {
    ($num_bigint:path) => {
        use $num_bigint as num_bigint_crate;

        /// Converts a `num_bigint::BigInt` into a [`BigInt`].
        fn from_num(value: &num_bigint_crate::BigInt) -> crate::ext::BigInt {
            let (sign, magnitude) = value.to_bytes_be();
            crate::ext::BigInt {
                negative: sign == num_bigint_crate::Sign::Minus,
                magnitude,
            }
        }

        /// Converts a [`BigInt`] into a `num_bigint::BigInt`.
        fn to_num(value: &crate::ext::BigInt) -> num_bigint_crate::BigInt {
            let sign = if value.is_zero() {
                num_bigint_crate::Sign::NoSign
            } else if value.negative {
                num_bigint_crate::Sign::Minus
            } else {
                num_bigint_crate::Sign::Plus
            };
            num_bigint_crate::BigInt::from_bytes_be(sign, &value.magnitude)
        }
    };
}

#[cfg(any(feature = "bigdecimal", feature = "num-bigint"))]
use num_bigint_conversions;

mod std_types;

#[cfg(feature = "bigdecimal")]
mod bigdecimal;
#[cfg(feature = "chrono")]
mod chrono;
#[cfg(feature = "jiff")]
mod jiff;
#[cfg(feature = "num-bigint")]
mod num_bigint;
#[cfg(feature = "rust_decimal")]
mod rust_decimal;
#[cfg(feature = "time")]
mod time;
#[cfg(feature = "uuid")]
mod uuid;
