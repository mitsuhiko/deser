# deser-pickle

Support for Python's pickle format for
[deser](https://github.com/mitsuhiko/deser).

```rust
use deser::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Session {
    user_id: u64,
    roles: Vec<String>,
}

// `pickle.dumps({"user_id": 42, "roles": ["admin"]}, 4)`
let input = b"\x80\x04\x95$\x00\x00\x00\x00\x00\x00\x00}\x94(\x8c\x07user_id\x94K*\x8c\x05roles\x94]\x94\x8c\x05admin\x94au.";
let session: Session = deser_pickle::from_slice(input).unwrap();
assert_eq!(session.roles, ["admin"]);
let output = deser_pickle::to_vec(&session).unwrap();
assert_eq!(deser_pickle::from_slice::<Session>(&output).unwrap(), session);
```

* Reads all protocols (0 to 5) like CPython's unpickler, but never imports
  or calls anything: objects are recorded with their class, arguments and
  state and passed on as their state (or items or arguments).  The class
  is passed on out of band.
* Shared values are emitted at every place they are reached, values that
  contain themselves are cut with references.  Both carry ids so that the
  graph can be restored.
* Writes protocols 2 to 5 (4 by default) that Python reads back, including
  objects of a class and shared and recursive values.

## License and Links

- [Documentation](https://docs.rs/deser-pickle)
- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/main/LICENSE)
