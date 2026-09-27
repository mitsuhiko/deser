# located

```
cargo run -p located
```

## Why

Tools such as linters, config validators and language servers need to
know *where* each value came from, not only what it is. In serde this is
done with `serde_spanned` or `serde_path_to_error`. That information is
usually lost as soon as a value is buffered, for example by untagged or
internally tagged enums (serde issue #1183). This example shows two ways
to carry extra information through deser, and both survive buffering.

## What it shows

1. **Out-of-band, through the state**: the JSON deserializer (with
   `track_locations(true)`) publishes the input range of each event.
   `deser_location::Spanned<T>` picks it up. This works for maps and
   sequences too (`hosts: Spanned<Vec<...>>`).
2. **In-band, as extension values**: the custom `Annotator` layer
   replaces every primitive value with a `LocatedAtom` extension that
   holds the path, the span and the value. Its `fallback()` is the plain
   value, so types that don't know about it (such as `debug: bool`) keep
   working. The custom `Located<T>` type unwraps it in its `Sink`.

It also shows how to write a deserialization `Layer`, an `Extension` and
a wrapping `Sink` around `OwnedSink`.

## What you should see

```
Config {
    name: "demo" (name @ 3:13-3:19),
    debug: true,
    limits: [
        Exact(
            100 (limits[0] @ 5:16-5:19),
        ),
        Range {
            min: 10 (limits[1].min @ 5:40-5:42),
            max: 20 (@ 5:29-5:31),
        },
    ],
    hosts: [
        "a.example.com" (hosts[0] @ 6:15-6:30),
        "b.example.com" (hosts[1] @ 6:32-6:47),
    ] (@ 6:14-6:48),
}
```

`limits` is an untagged enum, so its values were buffered and replayed,
and the paths and spans are still right. `max` uses `Spanned` (span only,
no path). `min` uses `Located` (path and span). The last step parses
without annotations and asserts that the spans are `None`.

## How to read it

Read the module docs first, then `Annotator` (the layer) and `LocatedSink`
(the consumer). `from_json_with_locations` shows how to set it up.

Related: `config-errors` (locations in errors), `dynamic-values` (spans on
`Value`), `layers`.
