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
/// This is the reference for [`f32_to_f16`].
///
/// NaN is never converted, the caller has to handle it.
#[cfg(test)]
fn f64_to_f16(value: f64) -> Option<u16> {
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

/// Encodes a single precision float as half precision float if that is
/// lossless.
///
/// This is the same as [`f64_to_f16`] for the value as `f64` but works on
/// the bits of the `f32`.  NaN is never converted, the caller has to handle
/// it.
#[inline]
pub fn f32_to_f16(value: f32) -> Option<u16> {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x7f_ffff;
    match exponent {
        // zero (subnormal floats are too small for half precision)
        0 => (mantissa == 0).then_some(sign),
        // infinity, NaN is never converted
        0xff => (mantissa == 0).then_some(sign | 0x7c00),
        _ => {
            let exponent = exponent - 127;
            if (-14..=15).contains(&exponent) {
                // a normal half has 10 bits of mantissa
                (mantissa & 0x1fff == 0)
                    .then(|| sign | (((exponent + 15) as u16) << 10) | (mantissa >> 13) as u16)
            } else if (-24..-14).contains(&exponent) {
                // a subnormal half is a multiple of 2^-24
                let significand = mantissa | 0x80_0000;
                let shift = (-exponent - 1) as u32;
                (significand & ((1 << shift) - 1) == 0)
                    .then(|| sign | (significand >> shift) as u16)
            } else {
                None
            }
        }
    }
}

#[test]
fn test_f32_to_f16() {
    // compare with the conversion of `f64`
    let step = if cfg!(miri) { 0x10_0001 } else { 0xfff };
    let special = [
        0.0f32,
        -0.0,
        1.0,
        65504.0,
        65505.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MIN_POSITIVE,
        1e-45,
    ];
    let halves = (0..=u16::MAX)
        .step_by(if cfg!(miri) { 97 } else { 1 })
        .map(|half| f16_to_f64(half) as f32);
    let values = (0..=u32::MAX)
        .step_by(step)
        .map(f32::from_bits)
        .chain(special)
        .chain(halves);
    for value in values {
        assert_eq!(
            f32_to_f16(value),
            f64_to_f16(f64::from(value)),
            "{:e} ({:08x})",
            value,
            value.to_bits()
        );
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
