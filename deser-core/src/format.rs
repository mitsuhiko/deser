//! Helpers for writing formats.
//!
//! This is not public API.
//!
//! * The text formats format floats with `zmij` when their `speedups`
//!   feature is enabled.  Without it they use [`format_finite`] which
//!   produces the same text, so the output does not depend on the feature.
//! * [`extend`] and [`push_str`] append short bytes and strings without
//!   calling into `memcpy`.
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::fmt::{Debug, LowerExp};

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

/// Formats integers without the `fmt` machinery.
///
/// ```ignore
/// let mut buffer = IntBuffer::new();
/// assert_eq!(buffer.format_i64(-42), "-42");
/// ```
pub struct IntBuffer {
    bytes: [u8; 20],
}

/// The decimal digits of the numbers from 0 to 99.
const DIGITS: &[u8; 200] = b"\
    0001020304050607080910111213141516171819\
    2021222324252627282930313233343536373839\
    4041424344454647484950515253545556575859\
    6061626364656667686970717273747576777879\
    8081828384858687888990919293949596979899";

impl IntBuffer {
    #[inline(always)]
    pub fn new() -> IntBuffer {
        IntBuffer { bytes: [0; 20] }
    }

    /// Formats an unsigned integer.
    #[inline]
    pub fn format_u64(&mut self, value: u64) -> &str {
        let start = self.write_digits(value);
        // SAFETY: only ASCII digits were written
        unsafe { core::str::from_utf8_unchecked(&self.bytes[start..]) }
    }

    /// Formats a signed integer.
    #[inline]
    pub fn format_i64(&mut self, value: i64) -> &str {
        let mut start = self.write_digits(value.unsigned_abs());
        if value < 0 {
            // `u64::MAX` has 20 digits, but `i64::MIN` only 19
            start -= 1;
            self.bytes[start] = b'-';
        }
        // SAFETY: only ASCII digits and the sign were written
        unsafe { core::str::from_utf8_unchecked(&self.bytes[start..]) }
    }

    /// Writes the digits to the end of the buffer and returns the start.
    #[inline(always)]
    fn write_digits(&mut self, mut value: u64) -> usize {
        let mut pos = self.bytes.len();
        while value >= 100 {
            let pair = (value % 100) as usize * 2;
            value /= 100;
            pos -= 2;
            self.bytes[pos..pos + 2].copy_from_slice(&DIGITS[pair..pair + 2]);
        }
        if value >= 10 {
            let pair = value as usize * 2;
            pos -= 2;
            self.bytes[pos..pos + 2].copy_from_slice(&DIGITS[pair..pair + 2]);
        } else {
            pos -= 1;
            self.bytes[pos] = b'0' + value as u8;
        }
        pos
    }
}

impl Default for IntBuffer {
    fn default() -> IntBuffer {
        IntBuffer::new()
    }
}

/// Appends bytes to a vector.
///
/// Most writes of formats are short, these are performed inline rather
/// than by calling into `memcpy`.
#[inline(always)]
pub fn extend(out: &mut Vec<u8>, bytes: &[u8]) {
    out.reserve(bytes.len());
    let len = out.len();
    // SAFETY: the capacity was reserved above and the regions cannot overlap
    // as the vector is borrowed mutably.
    unsafe {
        copy_small(bytes.as_ptr(), out.as_mut_ptr().add(len), bytes.len());
        out.set_len(len + bytes.len());
    }
}

/// Appends a string to a string, see [`extend`].
#[inline(always)]
pub fn push_str(out: &mut String, s: &str) {
    // SAFETY: only a complete string is appended
    extend(unsafe { out.as_mut_vec() }, s.as_bytes());
}

/// Copies bytes between non overlapping regions.
///
/// Short copies are performed inline with (possibly overlapping) word
/// sized loads and stores rather than calling into `memcpy`.
///
/// # Safety
///
/// Same requirements as `std::ptr::copy_nonoverlapping`.
#[inline(always)]
pub unsafe fn copy_small(src: *const u8, dst: *mut u8, len: usize) {
    unsafe {
        use core::ptr::{read_unaligned as read, write_unaligned as write};
        if len >= 16 {
            if len <= 32 {
                let a = read(src.cast::<u128>());
                let b = read(src.add(len - 16).cast::<u128>());
                write(dst.cast::<u128>(), a);
                write(dst.add(len - 16).cast::<u128>(), b);
            } else {
                core::ptr::copy_nonoverlapping(src, dst, len);
            }
        } else if len >= 8 {
            let a = read(src.cast::<u64>());
            let b = read(src.add(len - 8).cast::<u64>());
            write(dst.cast::<u64>(), a);
            write(dst.add(len - 8).cast::<u64>(), b);
        } else if len >= 4 {
            let a = read(src.cast::<u32>());
            let b = read(src.add(len - 4).cast::<u32>());
            write(dst.cast::<u32>(), a);
            write(dst.add(len - 4).cast::<u32>(), b);
        } else if len > 0 {
            let a = *src;
            let b = *src.add(len / 2);
            let c = *src.add(len - 1);
            *dst = a;
            *dst.add(len / 2) = b;
            *dst.add(len - 1) = c;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int_buffer() {
        let mut buffer = IntBuffer::new();
        for value in [0, 1, 9, 10, 99, 100, 101, 12345, u64::MAX, u64::MAX - 1] {
            assert_eq!(buffer.format_u64(value), value.to_string());
        }
        for value in [0, -1, 1, -10, 99, -100, i64::MIN, i64::MAX, i64::MIN + 1] {
            assert_eq!(buffer.format_i64(value), value.to_string());
        }
        let mut x: u64 = 0x2545_f491_4f6c_dd1d;
        for _ in 0..1000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let digits = (x % 20) as u32;
            let value = x % 10u64.saturating_pow(digits).max(1);
            assert_eq!(buffer.format_u64(value), value.to_string());
            let signed = (value as i64).wrapping_neg();
            assert_eq!(buffer.format_i64(signed), signed.to_string());
        }
    }

    #[test]
    fn test_extend() {
        let source: Vec<u8> = (0..100u8).collect();
        for start in 0..4 {
            for len in 0..(100 - start) {
                let mut out = vec![b'x'];
                extend(&mut out, &source[start..start + len]);
                out.push(b'y');
                assert_eq!(&out[1..out.len() - 1], &source[start..start + len]);
                assert_eq!((out[0], out[out.len() - 1]), (b'x', b'y'));
            }
        }
        let mut out = String::from("x");
        push_str(&mut out, "äöü");
        assert_eq!(out, "xäöü");
    }

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
