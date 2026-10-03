# deep-nesting

```
cargo run -p deep-nesting
```

## Why

Most serialization libraries recurse on the call stack, one frame per
nesting level. Deeply nested input then overflows the stack. Such a crash
cannot be caught, which makes it a denial-of-service risk with untrusted
input. deser does not recurse: nested sinks and emitters are handed back
to a driver that keeps them in an arena on the heap. This example proves it with a
million levels of nesting. It also shows how to put a limit on nesting
depth anyway.

## What it shows

- Deserializing `{"children":[{"children":[...]}]}` nested 1,000,000
  levels deep into a recursive `Tree`.
- Serializing it to CBOR, reading it back and serializing to JSON again,
  still without recursion.
- `deser::de::Limits::builder().max_depth(64).build()` in the context to
  reject deep input.
- A custom iterative `Drop` for `Tree`. This is needed because Rust's own
  generated drop glue is recursive and would overflow. That limitation is
  in Rust, not in deser.

## What you should see

```
depth: 1000000
JSON: 15000000 bytes, CBOR: 11000000 bytes
with limits: LimitExceeded: recursion limit exceeded at line 1 column 417
```

It takes a moment in debug builds because the input is 15 MB.

## How to read it

The point is that the program finishes at all. Try removing the `Drop`
impl and see that the crash comes from dropping the tree, not from
deser.

Related: `layers` (`Limits` in combination with `PathLayer`).
