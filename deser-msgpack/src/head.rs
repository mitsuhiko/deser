//! Decoding of the heads of MessagePack items.
//!
//! The head is the format byte with its arguments (lengths, the type of
//! extensions and the values of numbers).  It's shared by the parser and
//! the scanner of streams.

/// The head of an item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Head {
    Nil,
    Bool(bool),
    /// Unsigned integers and signed integers that are not negative.
    Uint(u64),
    /// Negative integers.
    Int(i64),
    F32(f32),
    F64(f64),
    /// A string with the length of its body.
    Str(u32),
    /// Binary data with the length of its body.
    Bin(u32),
    /// An array with the number of its items.
    Array(u32),
    /// A map with the number of its entries.
    Map(u32),
    /// An extension with its type and the length of its body.
    Ext(i8, u32),
}

/// Why a head could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeadError {
    /// The input ends within the head.
    Incomplete,
    /// The format byte `0xc1` which is never used.
    Reserved,
}

/// Reads `N` bytes at `offset`.
#[inline(always)]
fn read<const N: usize>(input: &[u8], offset: usize) -> Result<[u8; N], HeadError> {
    match input.get(offset..offset + N) {
        Some(bytes) => Ok(bytes.try_into().unwrap()),
        None => Err(HeadError::Incomplete),
    }
}

#[inline(always)]
fn len8(input: &[u8]) -> Result<u32, HeadError> {
    Ok(u32::from(read::<1>(input, 1)?[0]))
}

#[inline(always)]
fn len16(input: &[u8]) -> Result<u32, HeadError> {
    Ok(u32::from(u16::from_be_bytes(read(input, 1)?)))
}

#[inline(always)]
fn len32(input: &[u8]) -> Result<u32, HeadError> {
    Ok(u32::from_be_bytes(read(input, 1)?))
}

/// Signed integers that are not negative are unsigned integers.
#[inline(always)]
fn int(value: i64) -> Head {
    if value < 0 {
        Head::Int(value)
    } else {
        Head::Uint(value as u64)
    }
}

/// Decodes the head of the item at the start of the input.
///
/// Returns the head and its length in bytes.  The body of strings, binary
/// data and extensions follows the head.
#[inline(always)]
pub(crate) fn decode_head(input: &[u8]) -> Result<(Head, usize), HeadError> {
    let Some(&byte) = input.first() else {
        return Err(HeadError::Incomplete);
    };
    Ok(match byte {
        0x00..=0x7f => (Head::Uint(u64::from(byte)), 1),
        0x80..=0x8f => (Head::Map(u32::from(byte & 0x0f)), 1),
        0x90..=0x9f => (Head::Array(u32::from(byte & 0x0f)), 1),
        0xa0..=0xbf => (Head::Str(u32::from(byte & 0x1f)), 1),
        0xc0 => (Head::Nil, 1),
        0xc1 => return Err(HeadError::Reserved),
        0xc2 => (Head::Bool(false), 1),
        0xc3 => (Head::Bool(true), 1),
        0xc4 => (Head::Bin(len8(input)?), 2),
        0xc5 => (Head::Bin(len16(input)?), 3),
        0xc6 => (Head::Bin(len32(input)?), 5),
        // ext 8, 16 and 32: the length and then the type
        0xc7 => (Head::Ext(read::<1>(input, 2)?[0] as i8, len8(input)?), 3),
        0xc8 => (Head::Ext(read::<1>(input, 3)?[0] as i8, len16(input)?), 4),
        0xc9 => (Head::Ext(read::<1>(input, 5)?[0] as i8, len32(input)?), 6),
        0xca => (Head::F32(f32::from_be_bytes(read(input, 1)?)), 5),
        0xcb => (Head::F64(f64::from_be_bytes(read(input, 1)?)), 9),
        0xcc => (Head::Uint(u64::from(read::<1>(input, 1)?[0])), 2),
        0xcd => (
            Head::Uint(u64::from(u16::from_be_bytes(read(input, 1)?))),
            3,
        ),
        0xce => (
            Head::Uint(u64::from(u32::from_be_bytes(read(input, 1)?))),
            5,
        ),
        0xcf => (Head::Uint(u64::from_be_bytes(read(input, 1)?)), 9),
        0xd0 => (int(i64::from(read::<1>(input, 1)?[0] as i8)), 2),
        0xd1 => (int(i64::from(i16::from_be_bytes(read(input, 1)?))), 3),
        0xd2 => (int(i64::from(i32::from_be_bytes(read(input, 1)?))), 5),
        0xd3 => (int(i64::from_be_bytes(read(input, 1)?)), 9),
        // fixext 1, 2, 4, 8 and 16
        0xd4..=0xd8 => {
            let len = 1 << (byte - 0xd4);
            (Head::Ext(read::<1>(input, 1)?[0] as i8, len), 2)
        }
        0xd9 => (Head::Str(len8(input)?), 2),
        0xda => (Head::Str(len16(input)?), 3),
        0xdb => (Head::Str(len32(input)?), 5),
        0xdc => (Head::Array(len16(input)?), 3),
        0xdd => (Head::Array(len32(input)?), 5),
        0xde => (Head::Map(len16(input)?), 3),
        0xdf => (Head::Map(len32(input)?), 5),
        0xe0..=0xff => (Head::Int(i64::from(byte as i8)), 1),
    })
}

#[test]
fn test_decode_head() {
    assert_eq!(decode_head(&[0x7f]), Ok((Head::Uint(127), 1)));
    assert_eq!(decode_head(&[0xe0]), Ok((Head::Int(-32), 1)));
    assert_eq!(decode_head(&[0xd0, 0x05]), Ok((Head::Uint(5), 2)));
    assert_eq!(decode_head(&[0xd0, 0xff]), Ok((Head::Int(-1), 2)));
    assert_eq!(decode_head(&[0xc7, 0x03, 0x07]), Ok((Head::Ext(7, 3), 3)));
    assert_eq!(
        decode_head(&[0xc8, 0x01, 0x00, 0xff]),
        Ok((Head::Ext(-1, 256), 4))
    );
    assert_eq!(decode_head(&[0xd8, 0x05]), Ok((Head::Ext(5, 16), 2)));
    assert_eq!(decode_head(&[0xc1]), Err(HeadError::Reserved));
    assert_eq!(decode_head(&[]), Err(HeadError::Incomplete));
    assert_eq!(decode_head(&[0xc7, 0x03]), Err(HeadError::Incomplete));
    assert_eq!(decode_head(&[0xcf, 0, 0]), Err(HeadError::Incomplete));
}
