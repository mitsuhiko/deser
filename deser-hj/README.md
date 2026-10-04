# deser-hj

[Hjson](https://hjson.github.io/) support for
[deser](https://github.com/mitsuhiko/deser).  Hjson is JSON for humans, a
format for configuration files: comments (`#`, `//` and `/* */`), optional
commas, keys and strings without quotes, multiline strings and maps without
braces at the root.

Note that the [`deser-hjson`](https://crates.io/crates/deser-hjson) crate
is entirely unrelated to this crate and deser, it is a Hjson deserializer
for serde.

```rust
#[derive(deser::Deserialize)]
struct Config<'a> {
    name: &'a str,
    version: String,
    ports: Vec<u16>,
    motd: String,
}

let config: Config = deser_hj::from_str(r#"
    # the name of the service
    name: api
    version: 2
    ports: [80, 443]
    motd:
        '''
        Welcome!
        Have a nice day.
        '''
"#).unwrap();
assert_eq!(config.name, "api");
assert_eq!(config.version, "2");
assert_eq!(config.motd, "Welcome!\nHave a nice day.");
```

Numbers, `true`, `false` and `null` without quotes are implicit values: an
`u16` receives `8080` as number, a `String` as `"8080"`.  Otherwise it
works like [`deser-json`](https://docs.rs/deser-json) and serializes JSON
(which is valid Hjson).  The parser and the serializer are generated from
the ones of `deser-json` (see `deser-template-json` in the repository) and
passes the [Hjson test suite](https://github.com/hjson/hjson/tree/master/testCases).
