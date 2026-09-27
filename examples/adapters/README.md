# adapters

```
cargo run -p adapters
```

## Why

Often a field's type does not implement `Serialize`/`Deserialize`, for
example because it comes from another crate. Sometimes it does, but you want
a different representation on the wire. You can't add impls to foreign
types and you don't want to write newtypes. Adapters fix this: the field
keeps its real Rust type, and `#[deser(as = Adapter)]` picks how it is
serialized.

## What it shows

- `DisplayFromStr`: `SocketAddr`/`IpAddr` are written with `Display` and
  read with `FromStr`.
- Composition with containers: `Option<DisplayFromStr>` and
  `BTreeMap<_, Vec<DisplayFromStr>>`. `_` means "use the type's own impl"
  (here, for the map keys).
- `FromInto<(u8, u8, u8)>`: a foreign `Rgb` type is converted through a
  tuple that deser already understands.
- Error-tolerant adapters: `VecSkipError` drops list elements that fail
  (an unknown `"gopher"` protocol). `DefaultOnError` replaces an invalid
  value (`"workers": "many"`) with the default.
- `As<T, Adapter>`: applies an adapter outside of a derive, for example to
  a top-level `Vec<IpAddr>`.

## What you should see

1. A `{:#?}` dump of `Config`. `protocols` holds only `[Http, Https]`
   (gopher was skipped), `workers` is `0` and `upstream` is `None`.
2. The config serialized back to JSON. Addresses are strings again and
   `color` is `[255,128,0]`.
3. `[10.0.0.1, fe80::1]`, parsed through the `As` wrapper.

The example uses `assert!` to check these results, so a clean exit means it
behaved as documented.

## How to read it

Start at the `Config` struct. Each field has a doc comment that explains
its adapter. Then compare the JSON input in `main` with the debug output.
The differences are what the adapters did.

Related: `serde-types` (the `Serde` adapter) and `bytes` (byte encoding
adapters).
