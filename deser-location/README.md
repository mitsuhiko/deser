# deser-location

Source locations (line and column) for
[deser](https://github.com/mitsuhiko/deser).  Wrap a field in `Spanned<T>`
and it remembers where in the input it came from.  This is what you want
for validation that happens after parsing: the file parsed fine, but a
value is semantically wrong and you want to point the user at the exact
spot, like a compiler would.

```rust
use deser::{Context, Deserialize, TrackLocations};
use deser_location::Spanned;

#[derive(Deserialize)]
struct Config {
    name: String,
    workers: Spanned<u32>,
}

let input = "name: web\nworkers: 0\n";
let mut de = deser_yaml::Deserializer::from_str(input);
de.set_context(Context::with(TrackLocations(true)));
let config: Config = de.deserialize().unwrap();

if config.workers.value == 0 {
    let span = config.workers.span.unwrap();
    assert_eq!(span.to_string(), "2:10-2:11");
    // error: workers must be at least 1 (at 2:10)
    println!("error: workers must be at least 1 (at {})", span.start);
}
```

Location tracking is opt-in with `TrackLocations` in the context of the
deserialization and is supported by all text formats (JSON and its
dialects, YAML, TOML, XML, CSV, query strings and property lists).  Formats publish
the byte range of every event, lines and columns are only computed when a
location is requested.  Locations survive buffering: `Spanned` values inside
internally tagged or untagged enums still know where they came from.
