# deser-validate

Validation for [deser](https://github.com/mitsuhiko/deser).  Values are
validated while they are deserialized, so errors point at the value in the
input (with line, column and path) like the errors of the format.

Validators are types.  The `Check` adapter validates a field with them,
the field keeps its type:

```rust
use deser::Deserialize;
use deser_validate::{Check, Email, Len, validator};

// a validator from a condition and a message
validator!(NonZero(port: &u16) => *port != 0, "must not be zero");

#[derive(Deserialize, Debug)]
struct Server {
    #[deser(as = Check<NonZero>)]
    port: u16,
    #[deser(as = Check<Len<1, 64>>)]
    name: String,
    #[deser(as = Vec<Check<Email>>)]
    admins: Vec<String>,
}

let json = r#"{"port": 0, "name": "web", "admins": []}"#;
let err = deser_json::from_str::<Server>(json).unwrap_err();
assert_eq!(
    err.to_string(),
    "Unexpected: invalid value: must not be zero at line 1 column 10"
);
```

## Writing Validators

The `validator!` macro turns a condition or a function into a validator
type.  There are three forms:

```rust
use deser_validate::{Validator, validator};

// a condition and the message if it's false
validator!(pub NonZero(port: &u16) => *port != 0, "must not be zero");

// a function
fn check_slug(value: &str) -> Result<(), &'static str> {
    let valid = |b: u8| b.is_ascii_lowercase() || b == b'-';
    if !value.is_empty() && value.bytes().all(valid) {
        Ok(())
    } else {
        Err("must be a lowercase identifier")
    }
}

validator!(pub Slug(value: &str) = check_slug);

// a block
validator!(pub EvenLength(items: &[u32]) {
    if items.len() % 2 != 0 {
        return Err(format!(
            "must have an even number of items, not {}",
            items.len()
        ));
    }
    Ok(())
});

assert!(NonZero::validate(&80u16).is_ok());
assert!(Slug::validate(&String::from("my-service")).is_ok());
assert_eq!(Slug::validate("My Service").unwrap_err().code(), "slug");
```

Things to know about the validators the macro creates:

* A validator of a type validates everything that borrows as it: a
  validator of `str` validates `String`, `Box<str>` and `Cow<str>`, a
  validator of `[T]` validates `Vec<T>`.
* Functions and blocks return a `Result<(), E>` where `E` is a message
  (`&'static str`, `String`) or a `Violation`, or a `bool`.  Messages
  become violations with the name of the validator in snake case as code
  (`Slug` has the code `slug`, `NonZero` the code `non_zero`).  A `bool`
  function that returns `false` fails with the message `is not valid`.
* Codes are what clients see, renaming a validator changes its code.  A
  code can be given as last argument instead:
  `validator!(Port(port: &u16) => *port != 0, "must not be zero", code = "port")`.
* The macro does not support types with generics or lifetimes.  For those
  implement `Validator` yourself, which is all the macro does too:

```rust
use deser_validate::{Validator, Violation};

enum Either<T> {
    Left(T),
    Right(String),
}

struct NonEmptyRight;

impl<T> Validator<Either<T>> for NonEmptyRight {
    fn validate(value: &Either<T>) -> Result<(), Violation> {
        match value {
            Either::Right(s) if s.is_empty() => {
                Err(Violation::new("not_empty", "must not be empty"))
            }
            _ => Ok(()),
        }
    }
}
```

Violations have a code and parameters (for programs, for instance to
translate the messages or to point at a field in a user interface) and a
message for humans.  Errors of invalid values have the `Violation`
attached.

## Checks Across Fields

On a type `Check` wraps its derived implementation (written as `_`), the
validator sees the whole value.  Plain functions work there as well:

```rust
use deser::Deserialize;
use deser_validate::{Check, validator};

#[derive(Deserialize, Debug)]
#[deser(deserialize_as = Check<OrderedPorts, _>)]
struct PortRange {
    min: u16,
    max: u16,
}

fn check_port_range(range: &PortRange) -> Result<(), String> {
    if range.min > range.max {
        let PortRange { min, max } = range;
        return Err(format!("min {min} is larger than max {max}"));
    }
    Ok(())
}

validator!(OrderedPorts(range: &PortRange) = check_port_range);

let json = r#"{"min": 90, "max": 80}"#;
let err = deser_json::from_str::<PortRange>(json).unwrap_err();
assert_eq!(err.message(), "invalid value: min 90 is larger than max 80");
```

This also works for values that are updated in place (layered
configuration): the value is checked once the update is complete.

## Naming Validators

Validators are named for the property that valid values have, as a noun or
an adjective: `Email`, `Slug`, `NonEmpty`, `NonZero`, `MaxLen<64>`.  They
read as `Check<NonZero>` and do not need affixes like `Valid`, `Is` or
`Rules`.  Negations start with `Non` (like `std::num::NonZero`).  Validators
of a whole type name the rule they check (`OrderedPorts`, not
`PortRangeRules`), combinators name their structure (`Each`).  The code of a
violation is the name of the validator in snake case (`max_len`,
`non_zero`), for the validators of this crate too.

## Invalid Values

What happens with an invalid value depends on how the validator is used:

| | invalid values |
|---|---|
| `#[deser(as = Check<V>)]` (fields) | fail the deserialization, the field keeps its type |
| `#[deser(deserialize_as = Check<V, _>)]` (types) | fail the deserialization |
| `Validated<T, V>` (the type of the field) | are kept with all their errors, deserialization continues |

Types that are always valid are newtypes that check themselves.  They are
often named like the validator, the validator is named by its path then:

```rust
use deser::Deserialize;
use deser_validate::Check;

#[derive(Deserialize)]
#[deser(transparent)]
pub struct Email(#[deser(as = Check<deser_validate::Email>)] String);
```

`Validated` keeps all errors of its value, also errors like a string where
a number is expected and errors deep inside the value.  This is what a form
that is shown again with its errors needs:

```rust
use deser::Deserialize;
use deser_validate::{Email, Range, Validated};

#[derive(Deserialize)]
struct Signup {
    email: Validated<String, Email>,
    age: Validated<u8, Range<13, 130>>,
}

let json = r#"{"email": "jane@", "age": "old"}"#;
let signup: Signup = deser_json::from_str(json).unwrap();
assert_eq!(signup.email.unchecked_value().unwrap(), "jane@");
assert!(signup.email.error().is_some());
assert!(signup.age.value().is_none());
```

## Reporting All Problems

A `Validation` reports all problems of an input at once: the errors that
`Validated` values keep and all errors that fail the deserialization, with
their paths.  This is what an API that answers with a list of problems or a
configuration loader wants:

```rust
use deser::Deserialize;
use deser_validate::{Check, Email, Range, Validation};

#[derive(Deserialize)]
struct Signup {
    #[deser(as = Check<Email>)]
    email: String,
    #[deser(as = Check<Range<13, 130>>)]
    age: u8,
    name: String,
}

let validation = Validation::new();
let json = r#"{"email": "jane@", "age": 7}"#;
let rv = deser_json::Deserializer::from_str(json)
    .deserialize_with::<Signup, _>(|driver| validation.setup(driver));
let report = validation.finish(rv).into_result().err().unwrap();
assert_eq!(
    report.to_string(),
    "email: invalid value: must be an email address \
     (at line 1 column 11)\n\
     age: invalid value: must be between 13 and 130 \
     (at line 1 column 27)\n\
     missing field `name` (at line 1 column 28)"
);
for issue in &report {
    // the path, the violation's code (for instance `email`) and the
    // message
    let code = issue.violation().map(|x| x.code());
    println!("{:?} {:?} {}", issue.path(), code, issue.message());
}
```

## Provided Validators

`NonEmpty`, `Len` (`MinLen`, `MaxLen`), `Range` (`Min`, `Max`), `Email` and
`Each` for the items of collections.  Tuples of validators require all of
them: `Check<(NonEmpty, MaxLen<64>)>`.

This is built on the error handling of deser: containers can recover from
the errors of their items (`Sink::recover`) and collect them
(`State::set_collect_errors`), and errors can hold multiple errors.
