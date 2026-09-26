//! Conversions between half precision floats and `f64`.
//!
//! Rust has no stable `f16` type so the conversions are implemented by hand.

/// Returns `2^exp` for exponents in the normal range.
///
/// This is exact unlike `powi` which is allowed to be imprecise.
fn pow2(exp: i32) -> f64 {
    debug_assert!((-1022..=1023).contains(&exp));
    f64::from_bits(((exp + 1023) as u64) << 52)
}

/// Decodes a half precision float.
pub fn f16_to_f64(half: u16) -> f64 {
    let exponent = (half >> 10) & 0x1f;
    let mantissa = f64::from(half & 0x3ff);
    let value = match exponent {
        // subnormal numbers and zero
        0 => mantissa * pow2(-24),
        0x1f => {
            if mantissa == 0.0 {
                f64::INFINITY
            } else {
                f64::NAN
            }
        }
        _ => (1024.0 + mantissa) * pow2(i32::from(exponent) - 25),
    };
    if half & 0x8000 != 0 { -value } else { value }
}

/// Encodes a float as half precision float if that is lossless.
///
/// NaN is never converted, the caller has to handle it.
pub fn f64_to_f16(value: f64) -> Option<u16> {
    let sign = if value.is_sign_negative() { 0x8000 } else { 0 };
    let abs = value.abs();
    if abs == 0.0 {
        Some(sign)
    } else if abs == f64::INFINITY {
        Some(sign | 0x7c00)
    } else if abs < pow2(-14) {
        // subnormal half: a multiple of 2^-24 below 2^-14.  Scaling by a
        // power of two is exact.
        let scaled = abs * pow2(24);
        if scaled.fract() == 0.0 {
            Some(sign | scaled as u16)
        } else {
            None
        }
    } else if abs <= 65504.0 {
        let bits = abs.to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as i32 - 1023;
        let mantissa = bits & ((1 << 52) - 1);
        // a half precision float has 10 bits of mantissa
        if mantissa & ((1 << 42) - 1) == 0 {
            Some(sign | (((exponent + 15) as u16) << 10) | (mantissa >> 42) as u16)
        } else {
            None
        }
    } else {
        None
    }
}

#[test]
fn test_f16_roundtrip() {
    let step = if cfg!(miri) { 97 } else { 1 };
    for half in (0..=u16::MAX).step_by(step) {
        let value = f16_to_f64(half);
        if value.is_nan() {
            assert_eq!(f64_to_f16(value), None);
            continue;
        }
        assert_eq!(f64_to_f16(value), Some(half), "{:04x}", half);
    }
}

#[test]
fn test_f16_inexact() {
    assert_eq!(f64_to_f16(1.1), None);
    assert_eq!(f64_to_f16(65505.0), None);
    assert_eq!(f64_to_f16(65536.0), None);
    assert_eq!(f64_to_f16(100000.0), None);
    assert_eq!(f64_to_f16(pow2(-25)), None);
    assert_eq!(f64_to_f16(1.0 + pow2(-11)), None);
    assert_eq!(f64_to_f16(f64::MIN_POSITIVE), None);
}
