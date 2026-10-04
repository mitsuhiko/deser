# deser-ini

INI files (and git's config files) for [deser](https://github.com/mitsuhiko/deser).

INI has no specification, every implementation reads it a little
differently.  The default dialect is based on a survey of INI files on GitHub
and reads what is common today: `;` and `#` comments (also after values),
`=` and `:` delimiters, values continued on indented lines (like Python's
`configparser`), quoted values (like PHP, MySQL and Windows) and keys without
values.  Presets read the files of Python's `configparser` and git's config
files.

```rust
use deser::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct Config {
    name: String,
    server: Server,
}

#[derive(Debug, Deserialize, Serialize)]
struct Server {
    host: String,
    port: u16,
    #[deser(default)]
    tags: Vec<String>,
}

let config: Config = deser_ini::from_str("
name = shop

[server]
host = localhost  ; the host to bind to
port = 8080
tags = a
tags = b
").unwrap();
assert_eq!(config.server.port, 8080);
assert_eq!(config.server.tags, ["a", "b"]);

assert_eq!(
    deser_ini::to_string(&config).unwrap(),
    "name = shop\n\n[server]\nhost = localhost\nport = 8080\ntags = a\ntags = b\n"
);
```

What works:

* Sections, keys before the first section, sections that repeat (they are
  merged) and keys that repeat (collected by `Vec<T>` fields, otherwise the
  last value is used).
* Comments after values, `:` delimiters, continuation lines, quotes, keys
  without values and lowercased names can be configured.
  `DeserializerConfig::python()` reads `setup.cfg`, `tox.ini` and the like,
  `DeserializerConfig::git()` reads `.gitconfig` and `.gitmodules` like git
  (subsections, escapes and git's quoting).
* Everything is text that is parsed by the type it ends up in, also after
  buffering (flattened structs, internally tagged and untagged enums).
  Booleans accept `yes`, `on` and `1`, empty values are `None` for
  optionals.
* Errors point at the line and column (and with `deser-path` at the path of
  the value).
* Serializing writes the keys before the first section, quotes values only
  where needed and writes values with line breaks as continuation lines.

Tested against inih, Python's `configparser` and git with a corpus of the
test inputs of INI parsers and real world files (see
[`tests/data`](https://github.com/mitsuhiko/deser/tree/main/deser-ini/tests/data)).

## License and Links

- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- [Documentation](https://docs.rs/deser-ini)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/master/LICENSE)
