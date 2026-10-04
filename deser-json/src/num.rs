//! Formats numbers without the `fmt` machinery.

/// The floats that are written (`f32` and `f64`), formatted with `zmij`.
pub(crate) trait Float: zmij::Float + Copy {
    /// Returns the value as `f64`.
    fn to_f64(self) -> f64;
}

impl Float for f64 {
    #[inline(always)]
    fn to_f64(self) -> f64 {
        self
    }
}

impl Float for f32 {
    #[inline(always)]
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}

/// Formats integers without the `fmt` machinery.
///
/// ```ignore
/// let mut buffer = IntBuffer::new();
/// assert_eq!(buffer.format_i64(-42), "-42");
/// ```
pub(crate) struct IntBuffer {
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
    pub(crate) fn new() -> IntBuffer {
        IntBuffer { bytes: [0; 20] }
    }

    /// Formats an unsigned integer.
    #[inline]
    pub(crate) fn format_u64(&mut self, value: u64) -> &str {
        let start = self.write_digits(value);
        // SAFETY: only ASCII digits were written
        unsafe { core::str::from_utf8_unchecked(&self.bytes[start..]) }
    }

    /// Formats a signed integer.
    #[inline]
    pub(crate) fn format_i64(&mut self, value: i64) -> &str {
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
}
