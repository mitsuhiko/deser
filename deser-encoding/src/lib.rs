//! More encodings of bytes as strings for [deser](https://docs.rs/deser).
//!
//! deser writes bytes as base64 strings in formats without native bytes (like
//! JSON) and provides the base64 encodings (see
//! [bytes in deser](deser_core::adapters#bytes)).  This crate adds hexadecimal and
//! base32:
//!
//! | Encoding           | Description                                        |
//! |--------------------|----------------------------------------------------|
//! | [`Hex`]            | hexadecimal, lowercase                             |
//! | [`HexUpper`]       | hexadecimal, uppercase                             |
//! | [`Base32`]         | base32 with padding                                |
//! | [`Base32NoPad`]    | base32 without padding                             |
//! | [`Base32Hex`]      | base32 with extended hex alphabet and padding      |
//! | [`Base32HexNoPad`] | base32 with extended hex alphabet without padding  |
//! | [`Base32Dnssec`]   | base32 of DNSSEC, lowercase extended hex alphabet  |
//!
//! Like the encodings of deser they are adapters which represent bytes as
//! strings in all formats.  They can be used with
//! [`BytesFallback`](deser_core::adapters::BytesFallback) to keep native bytes in
//! formats that have them, and with
//! [`BytesFormat`](deser_core::BytesFormat) to configure formats:
//!
//! ```
//! use deser::adapters::BytesFallback;
//! use deser::{BytesFormat, Deserialize, Serialize};
//! use deser_encoding::Hex;
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! pub struct Blob {
//!     // a hex string in all formats
//!     #[deser(as = Hex)]
//!     digest: [u8; 4],
//!     // hex in JSON, bytes in CBOR
//!     #[deser(as = BytesFallback<Hex>)]
//!     signature: Vec<u8>,
//!     // base64 in JSON, bytes in CBOR
//!     data: Vec<u8>,
//! }
//!
//! let blob = Blob {
//!     digest: [0xde, 0xad, 0xbe, 0xef],
//!     signature: vec![1, 255],
//!     data: vec![1, 255],
//! };
//! let json = deser_json::to_string(&blob).unwrap();
//! assert_eq!(
//!     json,
//!     r#"{"digest":"deadbeef","signature":"01ff","data":"Af8="}"#
//! );
//! assert_eq!(deser_json::from_str::<Blob>(&json).unwrap(), blob);
//!
//! // all bytes as hex (in the context of the serialization)
//! let hex = deser::Context::with(BytesFormat::encoded::<Hex>());
//! let mut ser = deser_json::Serializer::new();
//! ser.set_context(hex);
//! ser.serialize(&b"\x01\xff").unwrap();
//! assert_eq!(ser.finish(), r#""01ff""#);
//! ```
//!
//! All encodings decode lowercase and uppercase letters.
#![doc(html_logo_url = "https://raw.githubusercontent.com/mitsuhiko/deser/main/artwork/logo.svg")]

use std::sync::LazyLock;

use data_encoding::Encoding;
use deser_core::adapters::BytesEncoding;
use deser_core::{Error, ErrorKind};

fn decode(encoding: &Encoding, name: &str, s: &str) -> Result<Vec<u8>, Error> {
    encoding.decode(s.as_bytes()).map_err(|err| {
        Error::new(
            ErrorKind::InvalidValue,
            format!("invalid {} string: {}", name, err),
        )
    })
}

/// Returns the encoding which also decodes lowercase symbols.
fn case_insensitive(encoding: &Encoding) -> Encoding {
    let mut spec = encoding.specification();
    let upper: String = spec
        .symbols
        .chars()
        .filter(char::is_ascii_uppercase)
        .collect();
    spec.translate.from.push_str(&upper.to_ascii_lowercase());
    spec.translate.to.push_str(&upper);
    spec.encoding().expect("valid case insensitive encoding")
}

macro_rules! encoding {
    ($(#[$meta:meta])* $ty:ident, $name:expr, $encoding:ident) => {
        encoding!($(#[$meta])* $ty, $name, $encoding, &data_encoding::$encoding);
    };
    ($(#[$meta:meta])* $ty:ident, $name:expr, $encoding:ident, case_insensitive) => {
        encoding!(
            $(#[$meta])* $ty,
            $name,
            $encoding,
            {
                static DECODER: LazyLock<Encoding> =
                    LazyLock::new(|| case_insensitive(&data_encoding::$encoding));
                &*DECODER
            }
        );
    };
    ($(#[$meta:meta])* $ty:ident, $name:expr, $encoding:ident, $decoder:expr) => {
        $(#[$meta])*
        pub struct $ty;

        impl BytesEncoding for $ty {
            const NAME: &'static str = $name;

            fn encode(bytes: &[u8], out: &mut String) {
                data_encoding::$encoding.encode_append(bytes, out);
            }

            fn decode(s: &str) -> Result<Vec<u8>, Error> {
                decode($decoder, Self::NAME, s)
            }
        }
    };
}

encoding!(
    /// Hexadecimal with lowercase digits.
    ///
    /// Lowercase and uppercase digits are accepted when decoding.
    Hex,
    "hex",
    HEXLOWER_PERMISSIVE
);

encoding!(
    /// Hexadecimal with uppercase digits.
    ///
    /// Lowercase and uppercase digits are accepted when decoding.
    HexUpper,
    "hex-upper",
    HEXUPPER_PERMISSIVE
);

encoding!(
    /// Base32 with padding (RFC 4648 section 6).
    ///
    /// Uppercase and lowercase letters are accepted when decoding.
    Base32,
    "base32",
    BASE32,
    case_insensitive
);

encoding!(
    /// Base32 without padding.
    ///
    /// Uppercase and lowercase letters are accepted when decoding.
    Base32NoPad,
    "base32-nopad",
    BASE32_NOPAD,
    case_insensitive
);

encoding!(
    /// Base32 with the extended hex alphabet and padding (RFC 4648
    /// section 7).
    ///
    /// Uppercase and lowercase letters are accepted when decoding.
    Base32Hex,
    "base32hex",
    BASE32HEX,
    case_insensitive
);

encoding!(
    /// Base32 with the extended hex alphabet without padding.
    ///
    /// Uppercase and lowercase letters are accepted when decoding.
    Base32HexNoPad,
    "base32hex-nopad",
    BASE32HEX_NOPAD,
    case_insensitive
);

encoding!(
    /// Base32 of DNSSEC (RFC 5155 section 3.3): the extended hex alphabet
    /// with lowercase letters and without padding.
    ///
    /// This is the encoding of hashed owner names in NSEC3 records.
    /// Lowercase and uppercase letters are accepted when decoding.
    Base32Dnssec,
    "base32-dnssec",
    BASE32_DNSSEC
);

#[cfg(test)]
mod tests {
    use deser::adapters::{As, BytesFallback};
    use deser::{BytesFormat, Deserialize, Serialize};

    use super::*;

    fn encode<E: BytesEncoding>(bytes: &[u8]) -> String {
        let mut rv = String::from(">");
        E::encode(bytes, &mut rv);
        rv[1..].to_string()
    }

    #[test]
    fn test_hex() {
        assert_eq!(encode::<Hex>(b"\x00\x1f\xab"), "001fab");
        assert_eq!(encode::<HexUpper>(b"\x00\x1f\xab"), "001FAB");
        assert_eq!(encode::<Hex>(b""), "");
        for decode in [Hex::decode, HexUpper::decode] {
            assert_eq!(decode("001fAB").unwrap(), b"\x00\x1f\xab");
            assert_eq!(decode("09afAF").unwrap(), b"\x09\xaf\xaf");
            assert_eq!(decode("").unwrap(), b"");
            for invalid in ["0", "0g", "0/", "0:", "@0", "G0", "0`", "g0", "\u{ff}"] {
                assert!(decode(invalid).is_err(), "{invalid:?}");
            }
        }
        assert_eq!(
            Hex::decode("zz").unwrap_err().to_string(),
            "InvalidValue: invalid hex string: invalid symbol at 0"
        );
    }

    #[test]
    fn test_base32() {
        assert_eq!(encode::<Base32>(b"foo"), "MZXW6===");
        assert_eq!(encode::<Base32NoPad>(b"foo"), "MZXW6");
        assert_eq!(encode::<Base32Hex>(b"foo"), "CPNMU===");
        assert_eq!(encode::<Base32HexNoPad>(b"foo"), "CPNMU");
        assert_eq!(Base32::decode("MZXW6===").unwrap(), b"foo");
        assert_eq!(Base32NoPad::decode("MZXW6").unwrap(), b"foo");
        assert_eq!(Base32Hex::decode("CPNMU===").unwrap(), b"foo");
        assert_eq!(Base32HexNoPad::decode("CPNMU").unwrap(), b"foo");
        assert_eq!(Base32::decode("mzXw6===").unwrap(), b"foo");
        assert_eq!(Base32NoPad::decode("mzXw6").unwrap(), b"foo");
        assert_eq!(Base32Hex::decode("cpNmu===").unwrap(), b"foo");
        assert_eq!(Base32HexNoPad::decode("cpNmu").unwrap(), b"foo");
        // letters beyond the alphabet stay invalid in either case
        assert!(Base32Hex::decode("W0======").is_err());
        assert!(Base32Hex::decode("w0======").is_err());
        assert!(Base32::decode("MZXW6").is_err());
        assert!(Base32NoPad::decode("MZXW6===").is_err());
        assert_eq!(
            Base32::decode("x").unwrap_err().to_string(),
            "InvalidValue: invalid base32 string: invalid length at 0"
        );
    }

    #[test]
    fn test_base32_dnssec() {
        assert_eq!(encode::<Base32Dnssec>(b"foo"), "cpnmu");
        assert_eq!(Base32Dnssec::decode("cpnmu").unwrap(), b"foo");
        assert_eq!(Base32Dnssec::decode("CPNmu").unwrap(), b"foo");
        assert!(Base32Dnssec::decode("cpnmu===").is_err());
        // RFC 5155 appendix A: the hashed owner name of `example`
        let hash = Hex::decode("065368abeed7ec6e9feba96b8c8bc3e8b791f716").unwrap();
        assert_eq!(
            encode::<Base32Dnssec>(&hash),
            "0p9mhaveqvm6t7vbl5lop2u3t2rp3tom"
        );
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Blob {
        #[deser(as = Hex)]
        forced: [u8; 2],
        #[deser(as = BytesFallback<Hex>)]
        fallback: Vec<u8>,
        #[deser(as = Option<BytesFallback<Base32>>)]
        optional: Option<Vec<u8>>,
    }

    fn blob() -> Blob {
        Blob {
            forced: [1, 255],
            fallback: vec![2, 254],
            optional: Some(b"foo".to_vec()),
        }
    }

    #[test]
    fn test_adapters() {
        let json = deser_json::to_string(&blob()).unwrap();
        assert_eq!(
            json,
            r#"{"forced":"01ff","fallback":"02fe","optional":"MZXW6==="}"#
        );
        assert_eq!(deser_json::from_str::<Blob>(&json).unwrap(), blob());
        let upper = r#"{"forced":"01FF","fallback":"02FE","optional":"MZXW6==="}"#;
        assert_eq!(deser_json::from_str::<Blob>(upper).unwrap(), blob());

        // native bytes in CBOR, except for the forced string
        let cbor = deser_cbor::to_vec(&blob()).unwrap();
        assert!(cbor.windows(4).any(|x| x == b"01ff"));
        assert!(!cbor.windows(4).any(|x| x == b"02fe"));
        assert_eq!(deser_cbor::from_slice::<Blob>(&cbor).unwrap(), blob());

        let err = deser_json::from_str::<As<[u8; 2], Hex>>(r#""01""#).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::WrongLength);
        let err = deser_json::from_str::<As<Vec<u8>, Hex>>("1").unwrap_err();
        assert!(
            err.to_string()
                .contains("unexpected unsigned integer, expected bytes or hex string"),
            "{err}"
        );
    }

    #[test]
    fn test_bytes_format() {
        const HEX: BytesFormat = BytesFormat::encoded::<Hex>();
        assert_eq!(HEX.name(), "hex");
        assert_ne!(HEX, BytesFormat::encoded::<HexUpper>());
        assert_eq!(HEX.encode(b"\x01\xff").as_deref(), Some("01ff"));
        assert_eq!(HEX.decode("01FF").unwrap(), b"\x01\xff");

        let context = deser::Context::with(HEX);
        assert_eq!(
            deser_json::Deserializer::from_str(r#""01ff""#)
                .deserialize_with::<Vec<u8>, _>(|driver| driver.set_context(context.clone()))
                .unwrap(),
            [1, 255]
        );
    }
}
