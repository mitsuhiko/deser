# deser-plist

[Property list](https://en.wikipedia.org/wiki/Property_list) support for
[deser](https://github.com/mitsuhiko/deser).  Property lists are the
configuration and serialization format of Apple's platforms: `Info.plist`
files, preferences, Xcode projects, `.strings` files and keyed archives.

All three formats are supported.  Deserialization detects the format, the
serializer writes the format of its configuration (XML by default):

```rust
use deser::{Deserialize, Serialize};
use deser_plist::{Format, SerializerConfig};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename_all = "PascalCase")]
struct Info {
    bundle_name: String,
    bundle_version: u32,
}

let info = Info { bundle_name: "Demo".into(), bundle_version: 42 };

let xml = deser_plist::to_vec(&info).unwrap();
let binary = SerializerConfig::builder()
    .format(Format::Binary).build()
    .to_vec(&info)
    .unwrap();

assert_eq!(deser_plist::from_slice::<Info>(&xml).unwrap(), info);
assert_eq!(deser_plist::from_slice::<Info>(&binary).unwrap(), info);
assert_eq!(
    deser_plist::from_slice::<Info>(b"{ BundleName = Demo; BundleVersion = 42; }").unwrap(),
    info
);
```

* Reads XML, binary (`bplist00`) and OpenStep property lists including
  `.strings` files and text in UTF-16.
* Writes XML and binary property lists like Core Foundation does: the XML
  output and the layout of binary property lists match `plutil`.  OpenStep
  is written in the style of Xcode.
* Dates map onto deser's well-known `Timestamp`, so
  `std::time::SystemTime` and the timestamp types of `jiff`, `chrono` and
  `time` work with the respective features of deser.
* UIDs of `NSKeyedArchiver` archives are passed through as `Uid`, which
  falls back to an integer.  In XML they are `CF$UID` dictionaries.
* Keys of dictionaries (and all strings of OpenStep property lists) are
  lexical, so they deserialize into numbers and booleans.
* Strings and data are borrowed from the input where possible.
* Deeply nested input does not overflow the stack.  Shared objects of
  binary property lists are supported, cycles are an error.
* Works without the standard library (with `alloc`).

The tests run the test files of the [`plist`
crate](https://github.com/ebarnard/rust-plist) and compare the results
against Apple's `plutil`.

## License and Links

- [Issue Tracker](https://github.com/mitsuhiko/deser/issues)
- [Documentation](https://docs.rs/deser-plist)
- License: [Apache-2.0](https://github.com/mitsuhiko/deser/blob/main/LICENSE)
