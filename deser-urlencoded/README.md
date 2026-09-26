# deser-urlencoded

Query strings and form data (`application/x-www-form-urlencoded`) for
[deser](https://github.com/mitsuhiko/deser).

Everything in a query string is text, only the type a value is deserialized
into knows what it means.  With serde this information gets lost as soon as
values are buffered, which is why numbers stop parsing in flattened structs
and in internally tagged and untagged enums.  deser passes the text on as
lexical atoms which are parsed by the type they end up in, also after
buffering:

```rust
use deser::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct Search {
    q: String,
    #[deser(flatten)]
    paginate: Paginate,
    #[deser(default)]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Paginate {
    limit: u32,
    offset: Option<u32>,
}

let search: Search =
    deser_urlencoded::from_str("q=rust&limit=10&offset=&tags=a&tags=b").unwrap();
assert_eq!(search.paginate.limit, 10);
assert_eq!(search.paginate.offset, None);
assert_eq!(search.tags, ["a", "b"]);

assert_eq!(
    deser_urlencoded::to_string(&search).unwrap(),
    "q=rust&limit=10&tags=a&tags=b"
);
```

What works:

* Repeated keys (`a=1&a=2`) as sent by `<select multiple>`, brackets
  (`a[]=1`), indexes (`a[0][b]=1`) and dots (`a.b=1`).  A single value is
  accepted for sequences, a key given more than once for a single value uses
  the last value (or the first one or it's an error).
* Nested structs, maps and enums.
* Empty values are `None` for optionals of types that do not accept them
  (`page=` for an `Option<u32>`).  Booleans accept what checkboxes send.
* Errors point at the position in the input and (with `deser-path`) at the
  path of the value.
* Serializing like `serde_url_params` (`a=1&a=2`), with brackets or indexes.

## License and Links

- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- [Documentation](https://docs.rs/deser-urlencoded)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/master/LICENSE)
