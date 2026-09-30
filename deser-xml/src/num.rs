//! Formats floats.
//!
//! [`format_finite`] produces the same text as `zmij`.
use core::fmt::{self, Debug, LowerExp, Write};
use core::str::FromStr;

/// The floats that can be formatted (`f32` and `f64`).
pub(crate) trait Float: Copy + PartialEq + FromStr + LowerExp + Debug {
    /// Values below `10^MAX_PLAIN` are written without exponent.
    const MAX_PLAIN: i32;
    /// Values of at least `10^MIN_PLAIN` are written without exponent.
    const MIN_PLAIN: i32;

    /// Returns the value as `f64`.
    fn to_f64(self) -> f64;

    /// Returns the absolute value.
    fn abs(self) -> Self;
}

impl Float for f64 {
    const MAX_PLAIN: i32 = 16;
    const MIN_PLAIN: i32 = -5;

    #[inline(always)]
    fn to_f64(self) -> f64 {
        self
    }

    fn abs(self) -> Self {
        f64::abs(self)
    }
}

impl Float for f32 {
    const MAX_PLAIN: i32 = 13;
    const MIN_PLAIN: i32 = -6;

    #[inline(always)]
    fn to_f64(self) -> f64 {
        f64::from(self)
    }

    fn abs(self) -> Self {
        f32::abs(self)
    }
}

/// Formats a finite float like `zmij::Buffer::format_finite`.
///
/// The text has the shortest digits that read back as the value of its
/// type, in scientific notation only for very large and very small values
/// and always with a `.` or an exponent (`1.0`, `0.001`, `1e+16`,
/// `1.5e-7`).
pub(crate) fn format_finite<F: Float>(val: F) -> String {
    // the standard library formats the shortest digits that read back
    let mut scientific = StackText::new();
    write!(scientific, "{:e}", val).unwrap();
    let (mantissa, exp) = scientific.as_str().split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(mantissa) => ("-", mantissa),
        None => ("", mantissa),
    };
    // the digits without the point, at most 17
    let mut buffer = [0u8; 17];
    let mut len = 0;
    for &byte in mantissa.as_bytes() {
        if byte != b'.' {
            buffer[len] = byte;
            len += 1;
        }
    }
    let digits = &mut buffer[..len];
    // if the value is exactly between two candidates, the standard library
    // rounds up and zmij to the even one.  Only if both read back: the gap
    // to the next smaller value of a power of two is half as large.
    let last = digits[len - 1];
    if (last - b'0') % 2 == 1 && is_tie(val.abs(), ascii(digits), exp) {
        digits[len - 1] = last - 1;
        let mut even = StackText::new();
        write!(even, "{}e{}", ascii(digits), exp - (len as i32 - 1)).unwrap();
        if even.as_str().parse::<F>().ok() != Some(val.abs()) {
            digits[len - 1] = last;
        }
    }
    let digits = ascii(digits);
    let len = len as i32;
    // the value is digits * 10^k and 10^(kk - 1) <= value < 10^kk
    let kk = exp + 1;
    let k = kk - len;
    let mut out = String::with_capacity(len as usize + 8);
    out.push_str(sign);
    if 0 <= k && kk <= F::MAX_PLAIN {
        // 1234e7 -> 12340000000.0
        out.push_str(digits);
        out.extend(core::iter::repeat_n('0', k as usize));
        out.push_str(".0");
    } else if 0 < kk && kk <= F::MAX_PLAIN {
        // 1234e-2 -> 12.34
        out.push_str(&digits[..kk as usize]);
        out.push('.');
        out.push_str(&digits[kk as usize..]);
    } else if F::MIN_PLAIN < kk && kk <= 0 {
        // 1234e-6 -> 0.001234
        out.push_str("0.");
        out.extend(core::iter::repeat_n('0', -kk as usize));
        out.push_str(digits);
    } else {
        // 1e30, 1234e30 -> 1.234e+33
        out.push_str(&digits[..1]);
        if len > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        if kk > 1 {
            out.push('+');
        }
        write!(out, "{}", kk - 1).unwrap();
    }
    out
}

/// Returns ASCII bytes as string.
fn ascii(bytes: &[u8]) -> &str {
    core::str::from_utf8(bytes).unwrap()
}

/// Text formatted on the stack.
///
/// This holds the scientific notation of floats (at most 24 bytes, like
/// `-2.2250738585072014e-308`).
struct StackText {
    bytes: [u8; 32],
    len: usize,
}

impl StackText {
    fn new() -> StackText {
        StackText {
            bytes: [0; 32],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        // only strings are written
        core::str::from_utf8(&self.bytes[..self.len]).unwrap()
    }
}

impl fmt::Write for StackText {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        self.bytes
            .get_mut(self.len..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// Returns `true` if the value is exactly between the digits and the digits
/// with the last one decremented (with the same exponent).
///
/// The value is positive, the digits `d` (`n` of them) stand for
/// `d * 10^(exp - n + 1)`.  With `q = exp - n` the middle is
/// `(2d - 1) * 5 * 10^q`, which is `(2d - 1) * 5^(q + 1) * 2^q`.  The value
/// is `m * 2^e` with an odd `m` (floats are exact as doubles).  As the
/// factors before the powers of two are odd on both sides, they are equal
/// if the exponents of two are and the rest is.
fn is_tie<F: Float>(val: F, digits: &str, exp: i32) -> bool {
    let bits = val.to_f64().to_bits();
    let (mut m, mut e) = match (bits >> 52) as i32 & 0x7ff {
        0 => (bits & ((1 << 52) - 1), -1074),
        biased => (bits & ((1 << 52) - 1) | (1 << 52), biased - 1075),
    };
    let zeros = m.trailing_zeros();
    m >>= zeros;
    e += zeros as i32;

    let q = exp - digits.len() as i32;
    if e != q {
        return false;
    }
    // at most 17 digits
    let odd = u128::from(digits.parse::<u64>().unwrap()) * 2 - 1;
    let m = u128::from(m);
    // the power of five on the side of the middle
    let p = q + 1;
    let (small, large, power) = if p >= 0 {
        (m, odd, p as u32)
    } else {
        (odd, m, p.unsigned_abs())
    };
    // both are below 2^64, a larger product cannot be equal
    5u128.checked_pow(power).and_then(|x| x.checked_mul(large)) == Some(small)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check<F: Float + zmij::Float>(val: F) {
        assert_eq!(
            format_finite(val),
            zmij::Buffer::new().format_finite(val),
            "{:e}",
            val
        );
    }

    #[test]
    #[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
    fn test_like_zmij_f64() {
        for val in [
            0.0,
            -0.0,
            1.0,
            0.1,
            1e15,
            1e16,
            1.5e16,
            123456789012345.6,
            1e-5,
            1e-4,
            1.5e-5,
            1e-7,
            1e100,
            f64::MAX,
            f64::MIN,
            f64::MIN_POSITIVE,
            f64::EPSILON,
            5e-324,
            // exactly between two shortest candidates, the even one is used
            -(1149636667324797.0 + 0.25),
            165793407361858.0 + 0.125,
        ] {
            check(val);
        }
        // the candidate below a power of two does not always read back
        for exp in -1074..1024 {
            check(2f64.powi(exp));
            check(-2f64.powi(exp));
        }
        let mut x: u64 = 0x2545_f491_4f6c_dd1d;
        for _ in 0..100_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let val = f64::from_bits(x);
            if val.is_finite() {
                check(val);
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
    fn test_like_zmij_f32() {
        for val in [
            0.0,
            -0.0,
            1.0,
            0.1,
            1e12,
            1e13,
            1.5e13,
            1234567.8,
            1e-5,
            1e-6,
            1e-7,
            1.5e-6,
            f32::MAX,
            f32::MIN,
            f32::MIN_POSITIVE,
            f32::EPSILON,
            1e-45,
            // exactly between two shortest candidates, the even one is used
            f32::from_bits(0x3980_0000),
            f32::from_bits(0x3b90_0000),
        ] {
            check(val);
        }
        for exp in -149..128 {
            check(2f32.powi(exp));
        }
        let mut x: u32 = 0x4f6c_dd1d;
        for _ in 0..100_000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let val = f32::from_bits(x);
            if val.is_finite() {
                check(val);
            }
        }
    }
}
