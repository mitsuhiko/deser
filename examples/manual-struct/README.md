# manual-struct

```
cargo run -p manual-struct
```

## Why

This example implements `Serialize` and `Deserialize` by hand for a
simple struct. It is roughly what the derive generates. Read it to
understand deser's core model, or when you need an impl the derive can't
express. Note that `Cargo.toml` does not enable the `derive` feature.

## What it shows

- **Serialize**: `serialize` returns `Chunk::Struct` with a
  `StructEmitter` that yields `(key, SerializeHandle)` pairs one at a
  time. `describe` reports the Rust shape (`d.structure("User")`), which
  `deser-debug` uses.
- **Deserialize**: `deserialize_into(&mut Option<Self>)` returns a
  `SinkHandle` for a `Sink` that receives `map`, `next_key`, `next_value`
  and `finish` calls. Nested values are deserialized by returning
  handles for sub-sinks (`usize::deserialize_into(&mut self.id, state)`).
  Unknown keys get `SinkHandle::null()`. Missing fields become
  `ErrorKind::MissingField` errors.
- Neither side recurses: nested values are handed back to the driver.
  This is why `deep-nesting` works.
- Only compound values need a sink. Types that are deserialized from a
  single atom (like numbers or strings) implement
  `Deserialize::deserialize_atom` instead and need no sink at all (see the
  documentation of `deser::de`).
- Driving a sink by hand: `DeserializeDriver::new(&mut out)` plus
  `driver.emit(...)` of `Event`s. No data format is involved.

## What you should see

```
User {
    id: 23,
    emailAddress: "jane@example.com",
}
```

This is the `User` built from the hand-emitted events, printed through
`ToDebug` (which uses the manual `Serialize` impl).

## How to read it

Read from top to bottom: the emitter, then the sink, then `main`. The
event sequence in `main` (map start, key, value, ..., map end) is what a
format would feed in.

Related: `debug` (what `describe` enables), `located` (a wrapping `Sink`),
`layers` (the event model).
