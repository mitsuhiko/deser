# no-std

```
cargo run -p no-std
cargo build -p no-std --lib --target thumbv7em-none-eabihf
```

## Why

Firmware and other code without an operating system cannot use the
standard library.  deser only needs `alloc` (a global allocator) if its
`std` feature is off, so the same types and derives work on a
microcontroller.

## What it shows

- `deser` with `default-features = false` (no `std` and no `io`), the
  formats `deser-cbor` and `deser-json` the same way.
- A `#![no_std]` library with derived types: an internally tagged enum,
  a catch-all variant with `Recording` that keeps messages of newer
  firmware, borrowed strings (`&'a str`), a default and bytes.
- Encoding and decoding CBOR and JSON without the standard library.

## What you should see

```
CBOR: 69 bytes
JSON: {"type":"reading","sensorId":7,"unit":"celsius",
  "values":[21.5,21.75],"scale":1.0,"raw":"3q2+7w=="}
forwarded: {"type":"calibration","offset":-0.5}
error: OutOfRange: invalid value -1, expected u64 at line 1 column 27
```

## How to read it

`src/lib.rs` is the library that does not use the standard library.
`src/main.rs` runs it on the host (the binary uses `std`).  The second
command above builds the library for a Cortex-M4F
(`rustup target add thumbv7em-none-eabihf` first).

Without `std`, `HashMap` and `HashSet` (use `hashbrown`), `Path`,
`OsString`, `SystemTime`, `Mutex`, `RwLock` and `OnceLock` are not
supported, and `deser::io` is not available.
