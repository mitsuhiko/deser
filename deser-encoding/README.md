# deser-encoding

More encodings of bytes as strings for
[deser](https://github.com/mitsuhiko/deser): hexadecimal and base32.

deser writes bytes as base64 strings in formats without native bytes (like
JSON) and provides the base64 encodings.  This crate adds `Hex`,
`HexUpper`, `Base32`, `Base32NoPad`, `Base32Hex`, `Base32HexNoPad` and
`Base32Dnssec` (RFC 5155).  All of them decode lowercase and uppercase
letters.  Like the encodings of deser they are adapters:

```rust
use deser::adapters::BytesFallback;
use deser::{Deserialize, Serialize};
use deser_encoding::Hex;

#[derive(Serialize, Deserialize)]
pub struct Blob {
    // a hex string in all formats
    #[deser(as = Hex)]
    digest: [u8; 32],
    // hex in JSON, bytes in CBOR
    #[deser(as = BytesFallback<Hex>)]
    signature: Vec<u8>,
}
```

They can also be used to configure how formats represent all bytes (with
`deser::adapters::BytesFormat::encoded::<Hex>()`).

## License and Links

- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- [Documentation](https://docs.rs/deser-encoding)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/main/LICENSE)
