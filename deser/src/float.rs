//! Float formatting for the text formats.
//!
//! This is not public API.  The text formats format floats with `zmij` when
//! their `speedups` feature is enabled.  Without it they use
//! [`format_finite`] which produces the same text, so the output does not
//! depend on the feature.
use std::fmt::{Debug, LowerExp};

/// The floats that can be formatted (`f32` and `f64`).
pub trait Float: Copy + LowerExp + Debug + sealed::Sealed {
    /// Values below `10^MAX_PLAIN` are written without exponent.
    #[doc(hidden)]
    const MAX_PLAIN: i32;
    /// Values of at least `10^MIN_PLAIN` are written without exponent.
    #[doc(hidden)]
    const MIN_PLAIN: i32;

    /// Returns `true` if the value is neither infinite nor NaN.
    fn is_finite(self) -> bool;

    /// Returns the value as `f64`.
    fn to_f64(self) -> f64;

    #[doc(hidden)]
    fn abs(self) -> Self;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for f32 {}
    impl Sealed for f64 {}
}

impl Float for f64 {
    const MAX_PLAIN: i32 = 16;
    const MIN_PLAIN: i32 = -5;

    #[inline(always)]
    fn is_finite(self) -> bool {
        f64::is_finite(self)
    }

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
    fn is_finite(self) -> bool {
        f32::is_finite(self)
    }

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
pub fn format_finite<F: Float>(val: F) -> String {
    // the standard library formats the shortest digits that read back
    let scientific = format!("{:e}", val);
    let (mantissa, exp) = scientific.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(mantissa) => ("-", mantissa),
        None => ("", mantissa),
    };
    let mut digits = mantissa.replace('.', "");
    // if the value is exactly between two candidates, the standard library
    // rounds up and zmij to the even one
    let last = digits.as_bytes()[digits.len() - 1];
    if (last - b'0') % 2 == 1 && is_tie(val.abs(), &digits, exp) {
        digits.pop();
        digits.push((last - 1) as char);
    }
    let len = digits.len() as i32;
    // the value is digits * 10^k and 10^(kk - 1) <= value < 10^kk
    let kk = exp + 1;
    let k = kk - len;
    let mut out = String::with_capacity(len as usize + 8);
    out.push_str(sign);
    if 0 <= k && kk <= F::MAX_PLAIN {
        // 1234e7 -> 12340000000.0
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', k as usize));
        out.push_str(".0");
    } else if 0 < kk && kk <= F::MAX_PLAIN {
        // 1234e-2 -> 12.34
        out.push_str(&digits[..kk as usize]);
        out.push('.');
        out.push_str(&digits[kk as usize..]);
    } else if F::MIN_PLAIN < kk && kk <= 0 {
        // 1234e-6 -> 0.001234
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', -kk as usize));
        out.push_str(&digits);
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
        out.push_str(&(kk - 1).to_string());
    }
    out
}

/// Returns `true` if the value is exactly between the digits and the digits
/// with the last one decremented (with the same exponent).
#[cold]
fn is_tie<F: Float>(val: F, digits: &str, exp: i32) -> bool {
    let mut middle = digits.to_string();
    let last = middle.pop().unwrap();
    middle.push((last as u8 - 1) as char);
    middle.push('5');
    let matches = |precision: usize| {
        let formatted = format!("{:.*e}", precision, val);
        let (mantissa, formatted_exp) = formatted.split_once('e').unwrap();
        formatted_exp.parse() == Ok(exp)
            && mantissa.replace('.', "").trim_end_matches('0') == middle
    };
    // the rounded digits are cheap to check, the exact ones are only
    // formatted if they match.  The exact expansion of a double has at
    // most 767 significant digits (a float at most 112).
    matches(digits.len()) && matches(800)
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
        // every iteration takes about 40ms in miri
        let iterations = if cfg!(miri) { 100 } else { 100_000 };
        let mut x: u64 = 0x2545_f491_4f6c_dd1d;
        for _ in 0..iterations {
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
        let iterations = if cfg!(miri) { 100 } else { 100_000 };
        let mut x: u32 = 0x4f6c_dd1d;
        for _ in 0..iterations {
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
