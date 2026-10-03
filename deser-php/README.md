# deser-php

Support for PHP's serialization format (the format of PHP's `serialize`
and `unserialize`) for [deser](https://github.com/mitsuhiko/deser).

```rust
use deser::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Session {
    user_id: u64,
    roles: Vec<String>,
}

let input = br#"a:2:{s:7:"user_id";i:42;s:5:"roles";a:1:{i:0;s:5:"admin";}}"#;
let session: Session = deser_php::from_slice(input).unwrap();
assert_eq!(session.roles, ["admin"]);
assert_eq!(deser_php::to_vec(&session).unwrap(), input);
```

* Reads everything `unserialize` reads: arrays, objects (with their class
  and the visibility of their properties out of band), enum cases, custom
  serialized objects and the old escaped strings (`S:`).  Arrays whose
  keys are `0`, `1`, ... are sequences, all others maps.
* Never instantiates classes or runs code: objects are maps.
* Writes what `serialize` writes, including PHP's float formatting.
* References (`r:` and `R:`) are passed through as markers and are not
  resolved, which makes them mostly useless beyond writing the input back
  unchanged.

## License and Links

- [Documentation](https://docs.rs/deser-php)
- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/main/LICENSE)
