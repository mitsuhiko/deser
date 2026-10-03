# php

```
cargo run -p php
```

## Why

PHP applications store values with `serialize()`: WordPress keeps its
options that way, frameworks put models into caches and sessions are
serialized arrays.  Reading such data from Rust (or writing it for a PHP
application) means dealing with what PHP's arrays and objects are: arrays
are lists and maps at once, objects have a class and properties with a
visibility, strings are bytes and shared values are written as references.

## What it shows

- The `wp_user_roles` option of WordPress read into a
  `BTreeMap<String, Role>`: arrays with string keys are maps and
  structs.
- A cached model object read into a struct wrapped in `Object<T>`, which
  captures the class (`App\Models\User`).  Protected and private
  properties match fields by their name, a private property that is not
  valid UTF-8 goes into a `Vec<u8>`, an enum case (`E:`) into a Rust
  enum and an empty array (which is an empty list and an empty map in
  PHP) into a map.  Properties without a field are ignored.
- Writing the object back: an object of the same class, but with public
  properties as the struct does not keep the visibility.
- Two posts that share an author object.  The second one has a reference
  (`r:5;`) which is passed on as a `Reference` marker, it's not resolved
  to the author.
- A struct written for PHP: floats as PHP writes them, a key that is the
  text of an integer written as integer and `None` as `N;`.
- An error with its offset in the input.

## What you should see

The roles with their capabilities, the user with its class, the object
written back:

```
O:15:"App\Models\User":7:{s:2:"id";i:7;s:5:"email";s:16:"jane@example.com";...}
```

then

```
Hello: Author { name: "Jane" }
Again: Reference(Reference { kind: Object, number: 5 })
```

followed by the product as PHP reads it and:

```
error: Syntax: syntax error: invalid boolean at offset 19
```

## How to read it

Start with `WP_USER_ROLES` and `CACHED_USER` (the comment shows the PHP
code that wrote it) and the types below them, then follow `main` from top
to bottom.

Related: `plist` (another format with types that only some formats
have), `bytes` (bytes in formats with and without native bytes),
`dynamic-values` (data without types).
