# plist

```
cargo run -p plist
```

## Why

Property lists are how Apple's platforms store configuration: `Info.plist`
files of app bundles, preferences, Xcode projects and localized `.strings`
files.  They come in three formats (XML, binary and the older OpenStep
format) and the same file can be in any of them, so `deser-plist` detects
the format when reading.  Dates and data are native types, and the
OpenStep format only has strings, so there the types parse their values.

## What it shows

- An XML `Info.plist` read into a type with Apple's key names
  (`#[deser(rename = "CFBundleIdentifier")]`), with keys that are not
  of interest ignored and nested arrays of dictionaries.
- Preferences with a `SystemTime` (a `<date>`) and `Vec<u8>` (`<data>`)
  written as a binary property list (`bplist00`) and as XML.  `None` is
  left out as property lists have no null.  Both read back with
  `from_slice`, which detects the format (`Format::detect`).
- A `.strings` file, a dictionary without braces and with comments, read
  into a `BTreeMap<String, String>`.
- An OpenStep dictionary where `800` and `NO` are strings which the
  fields parse into `u32` and `bool`, and written back in the style of
  Xcode (`SerializerConfig::builder().format(Format::Ascii).build()`).
- An error in a text format with its line and column.

## What you should see

The `Info` struct, the size of the binary property list, the preferences
as XML:

```
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>LastOpened</key>
	<date>2024-05-01T10:40:00Z</date>
	<key>LaunchCount</key>
	<integer>17</integer>
	...
	<key>Bookmark</key>
	<data>
	Ym9vawAC
	</data>
</dict>
</plist>
```

followed by the strings, the window in OpenStep format and:

```
error: InvalidValue: invalid value "tall", expected u32 at line 3 column 14
```

## How to read it

Start with `INFO_PLIST` and the `Info` type, then the `Preferences` type,
then follow `main` from top to bottom.

Related: `formats` (the same type in other formats, with native dates),
`bytes` (bytes in formats with and without native bytes), `renames` (keys
with other names).
