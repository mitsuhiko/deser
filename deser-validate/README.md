# deser-validate

Validation for [deser](https://github.com/mitsuhiko/deser).  Values are
validated while they are deserialized, so errors point at the value in the
input (with line, column and path) like the errors of the format.

Validators are types, so they are named in the types of fields.  Three
wrappers decide what happens with invalid values:

* `Checked<T, V>` fails the deserialization if the value is invalid.  A
  `Checked` value is always valid and derefs to the value.
* `Validated<T, V>` keeps the errors of the value in it instead of failing,
  so the value around it can still be deserialized.  This also covers
  errors like a string where a number is expected and errors deep inside
  the value.
* `Collect<T>` collects all errors of a value instead of failing on the
  first one.

```rust
use deser::Deserialize;
use deser_validate::{Checked, Email, Len, Range, Validated, Validation};

#[derive(Deserialize)]
struct Signup {
    name: Checked<String, Len<1, 32>>,
    email: Validated<String, Email>,
    age: Validated<u8, Range<13, 130>>,
}

let input = r#"{"name": "jane", "email": "jane@", "age": "old"}"#;
let signup: Signup = deser_json::from_str(input).unwrap();
assert_eq!(*signup.name, "jane");
assert_eq!(signup.email.unchecked_value().unwrap(), "jane@");
assert!(signup.email.error().is_some());
assert!(signup.age.value().is_none());
```

A `Validation` reports all problems of an input at once: the errors that
`Validated` values keep and all errors that fail the deserialization, with
their paths.  This is what an API that answers with a list of problems or a
configuration loader wants:

```rust
let validation = Validation::new();
let rv = deser_json::Deserializer::from_str(input)
    .deserialize_with::<Signup, _>(|driver| validation.setup(driver));
match validation.finish(rv).into_result() {
    Ok(signup) => { /* all good */ }
    Err(report) => {
        for issue in &report {
            // for instance "email: invalid value: must be an email address"
            println!("{}: {}", issue.path().unwrap_or(""), issue.message());
        }
    }
}
```

The validators of this crate are `NonEmpty`, `Len` (`MinLen`, `MaxLen`),
`Range` (`Min`, `Max`), `Email` and `Each` for the items of collections.
Tuples of validators require all of them.  Custom validators implement
`Validator`, their violations have a code and parameters (for programs, for
instance to translate them) and a message.

This is built on the error handling of deser: containers can recover from
the errors of their items (`Sink::recover`) and collect them
(`State::set_collect_errors`), and errors can hold multiple errors.
