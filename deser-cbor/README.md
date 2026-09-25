# deser-cbor

[CBOR](https://www.rfc-editor.org/rfc/rfc8949) support for deser.

```rust
let bytes = deser_cbor::to_vec(&vec![1u32, 2, 3]).unwrap();
assert_eq!(bytes, [0x83, 0x01, 0x02, 0x03]);
let vec: Vec<u32> = deser_cbor::from_slice(&bytes).unwrap();
assert_eq!(vec, [1, 2, 3]);
```

* Reads all well-formed CBOR including indefinite length strings, arrays
  and maps and CBOR sequences.
* Writes the preferred serialization (shortest integers, lengths and
  lossless floats).  `to_canonical_vec` additionally sorts map entries for
  a deterministic encoding.
* Bignums (tags 2 and 3) map onto `u128` and `i128`.
* Other tags are transparent, `Tagged<T>` reads and writes them.
