# deser-transcode

Converts values from one data format to another with
[deser](https://github.com/mitsuhiko/deser): every deserializer can be
transcoded into every serializer, without types in between and without
code that is specific to the formats.

```rust
let mut de = deser_json::Deserializer::from_str(r#"{"name": "deser", "tags": ["a", "b"]}"#);
let mut ser = deser_yaml::Serializer::new();
deser_transcode::transcode(&mut de, &mut ser).unwrap();
assert_eq!(ser.finish(), "name: deser\ntags:\n  - a\n  - b\n");
```

The values are passed on as the deserializer emitted them and each
serializer does with them what it does with any value: values whose type
YAML infers from their text are written as that type, keys that are not
strings are written the way the target writes such keys, TOML skips map
entries that are null and so on.  One value is buffered at a time, strings
that the deserializer borrows from the input are not copied, and the
lengths of maps and sequences are known before they are written, so CBOR
and MessagePack write them upfront even when the input is JSON.

Streams of values (JSON Lines, YAML documents) are transcoded value by
value with a `Transcoder`, which reuses its buffer.  Layers (such as
limits for untrusted input) can be added to both sides with
`transcode_with`.

