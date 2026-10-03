mod check;

use deser::de::Limits;
use deser::{Context, Deserialize, Serialize, TrackLocations};
use deser_path::{Path, PathLayer};
use deser_validate::{
    Check, Each, Email, Len, MaxLen, NonEmpty, Range, Validated, Validation, Violation,
};

#[derive(Debug, Deserialize)]
struct Address {
    street: String,
    #[deser(as = Check<Range<1000, 99999>>)]
    zip: u32,
}

#[derive(Debug, Deserialize)]
struct Signup {
    #[deser(as = Check<(NonEmpty, MaxLen<16>)>)]
    name: String,
    email: Validated<String, Email>,
    age: Validated<u8, Range<13, 130>>,
    address: Validated<Address>,
    tags: Validated<Vec<String>, Each<Len<1, 8>>>,
    nickname: Validated<Option<String>, Each<NonEmpty>>,
}

const VALID: &str = r#"{
    "name": "jane",
    "email": "jane@example.com",
    "age": 30,
    "address": {"street": "Main St", "zip": 12345},
    "tags": ["a", "b"]
}"#;

const INVALID: &str = r#"{
    "name": "jane",
    "email": "jane",
    "age": [1, {"x": 2}],
    "address": {"street": "Main St", "zip": {"deep": [1, 2]}},
    "tags": ["a", "much too long"],
    "nickname": ""
}"#;

/// A JSON deserializer that tracks locations.
fn tracked(input: &str) -> deser_json::Deserializer<'_> {
    let config = deser_json::DeserializerConfig::builder()
        .context(Context::with(TrackLocations(true)))
        .build();
    deser_json::Deserializer::from_str_with_config(input, config)
}

fn with_paths<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, deser::Error> {
    tracked(input).deserialize_with(|driver| driver.push_layer(PathLayer::new()))
}

#[test]
fn test_valid() {
    let signup: Signup = deser_json::from_str(VALID).unwrap();
    assert_eq!(signup.name, "jane");
    assert_eq!(signup.email.value().unwrap(), "jane@example.com");
    assert_eq!(*signup.age.value().unwrap(), 30);
    assert_eq!(signup.address.value().unwrap().zip, 12345);
    assert!(signup.tags.is_valid());
    // missing options are valid
    assert_eq!(signup.nickname.value(), Some(&None));
}

#[test]
fn test_kept_errors() {
    let signup: Signup = with_paths(INVALID).unwrap();
    // the errors have the context of the value (the path is displayed)
    let error = |value: Option<&deser::Error>| {
        let err = value.unwrap();
        assert!(err.attachment::<Path>().is_some());
        err.to_string()
    };

    // the value is kept if only the validator rejected it
    assert_eq!(signup.email.unchecked_value().unwrap(), "jane");
    assert_eq!(signup.email.value(), None);
    assert_eq!(
        error(signup.email.error()),
        "InvalidValue: invalid value: must be an email address at line 3 column 14 (path: email)"
    );
    assert_eq!(
        signup
            .email
            .error()
            .unwrap()
            .attachment::<Violation>()
            .unwrap()
            .code(),
        "email"
    );

    // values that could not be deserialized have no value
    assert_eq!(signup.age.unchecked_value(), None);
    assert_eq!(
        error(signup.age.error()),
        "InvalidType: unexpected sequence, expected u8 at line 4 column 12 (path: age)"
    );

    // errors deep inside the value, the rest of it was skipped
    assert!(signup.address.unchecked_value().is_none());
    assert_eq!(
        error(signup.address.error()),
        "InvalidType: unexpected map, expected u32 at line 5 column 45 (path: address.zip)"
    );

    let violation = signup
        .tags
        .error()
        .unwrap()
        .attachment::<Violation>()
        .unwrap();
    assert_eq!(
        violation.message(),
        "item 1: length must be between 1 and 8"
    );
    assert_eq!(violation.param("index").unwrap().to_string(), "1");

    assert_eq!(
        signup.nickname.error().unwrap().message(),
        "invalid value: item 0: must not be empty"
    );
}

#[test]
fn test_checked_fails() {
    let err = with_paths::<Signup>(&VALID.replace("\"jane\"", "\"\"")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidValue: invalid value: must not be empty at line 2 column 13 (path: name)"
    );
    // the error of a checked value in a validated value is kept
    let signup: Signup = deser_json::from_str(&VALID.replace("12345", "12")).unwrap();
    assert_eq!(
        signup.address.error().unwrap().message(),
        "invalid value: must be between 1000 and 99999"
    );
}

#[test]
fn test_format_and_layer_errors_are_not_kept() {
    let err =
        deser_json::from_str::<Signup>(&VALID.replace("\"age\": 30", "\"age\": [30")).unwrap_err();
    assert!(err.message().starts_with("expected"), "{}", err);

    let err = deser_json::Deserializer::from_str(r#"{"a": [[[1]]]}"#)
        .deserialize_with::<std::collections::BTreeMap<String, Validated<u32>>, _>(|driver| {
            driver.set_context(deser::Context::with(Limits::builder().max_depth(3).build()))
        })
        .unwrap_err();
    assert_eq!(err.message(), "recursion limit exceeded");
}

#[test]
fn test_root() {
    let value: Validated<u32> = deser_json::from_str(r#""x""#).unwrap();
    assert!(!value.is_valid());
    let value: Validated<u32> = deser_json::from_str("42").unwrap();
    assert_eq!(value.into_result().unwrap(), 42);
}

#[derive(Debug, Deserialize)]
struct Order {
    id: u64,
    shipping: Validated<Address>,
    lines: Vec<u32>,
}

#[test]
fn test_validated_collects_errors() {
    // all errors of the value are kept
    let order: Order =
        deser_json::from_str(r#"{"id": 1, "shipping": {"zip": 1, "extra": [1]}, "lines": [1, 2]}"#)
            .unwrap();
    let err = order.shipping.error().unwrap();
    let errors: Vec<_> = err.errors().map(|err| err.message()).collect();
    assert_eq!(
        errors,
        [
            "invalid value: must be between 1000 and 99999",
            "missing field `street`"
        ]
    );
    assert_eq!(order.lines, [1, 2]);

    // errors are collected in the value only, outside of it the first error
    // ends the deserialization
    let err = deser_json::from_str::<Order>(r#"{"id": 1, "shipping": {}, "lines": [1, "a", "b"]}"#)
        .unwrap_err();
    assert_eq!(err.errors().count(), 1);
}

#[test]
fn test_validated_error_limit() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Lists {
        a: Validated<Vec<u32>>,
        b: Validated<Vec<u32>>,
    }

    // below the limit the values keep their errors
    let mut validation = Validation::new();
    validation.set_max_errors(3);
    let rv = deser_json::Deserializer::from_str(r#"{"a": ["x", "y"], "b": ["z"]}"#)
        .deserialize_with::<Lists, _>(|driver| validation.setup(driver));
    let outcome = validation.finish(rv);
    assert!(outcome.value.is_some());
    assert_eq!(outcome.report.len(), 3);

    // the error that exceeds the limit ends the deserialization
    let mut validation = Validation::new();
    validation.set_max_errors(2);
    let rv = deser_json::Deserializer::from_str(r#"{"a": ["x", "y"], "b": ["z"]}"#)
        .deserialize_with::<Lists, _>(|driver| validation.setup(driver));
    let outcome = validation.finish(rv);
    assert!(outcome.value.is_none());
}

#[derive(Debug, Deserialize)]
#[deser(untagged)]
#[allow(dead_code)]
enum Contact {
    Email { email: Validated<String, Email> },
    Phone { phone: String },
}

#[test]
fn test_untagged_variants_do_not_keep_errors() {
    // a variant with an invalid value does not match
    let err = deser_json::from_str::<Contact>(r#"{"email": "nope"}"#).unwrap_err();
    assert_eq!(err.message(), "data did not match any variant of Contact");
    let contact: Contact = deser_json::from_str(r#"{"email": "jane@example.com"}"#).unwrap();
    assert!(matches!(contact, Contact::Email { email } if email.is_valid()));
}

#[test]
fn test_validation() {
    let validation = Validation::new();
    let rv = tracked(INVALID).deserialize_with::<Signup, _>(|driver| validation.setup(driver));
    let outcome = validation.finish(rv);
    // the value exists, it holds the invalid values
    assert!(!outcome.is_valid());
    assert!(outcome.value.is_some());
    assert_eq!(
        outcome.report.to_string(),
        "email: invalid value: must be an email address (at line 3 column 14)\n\
         age: unexpected sequence, expected u8 (at line 4 column 12)\n\
         address.zip: unexpected map, expected u32 (at line 5 column 45)\n\
         tags: invalid value: item 1: length must be between 1 and 8 (at line 6 column 13)\n\
         nickname: invalid value: item 0: must not be empty (at line 7 column 17)"
    );
    let violation = outcome.report.issues()[0].violation().unwrap();
    assert_eq!(violation.code(), "email");

    // errors that fail the deserialization are reported too, all of them
    let validation = Validation::new();
    let rv = deser_json::Deserializer::from_str(r#"{"email": "x", "name": "", "tags": [1]}"#)
        .deserialize_with::<Signup, _>(|driver| validation.setup(driver));
    let mut report = validation.finish(rv).into_result().unwrap_err();
    report.resolve_positions(br#"{"email": "x", "name": "", "tags": [1]}"#);
    assert_eq!(
        report.to_string(),
        "email: invalid value: must be an email address (at line 1 column 11)\n\
         name: invalid value: must not be empty (at line 1 column 24)\n\
         tags[0]: unexpected unsigned integer, expected string (at line 1 column 37)\n\
         missing field `age` (at line 1 column 39)\n\
         missing field `address` (at line 1 column 39)"
    );
}

#[test]
fn test_max_errors() {
    let mut validation = Validation::new();
    validation.set_max_errors(1);
    let rv = deser_json::Deserializer::from_str(r#"[1, "a", "b", "c"]"#)
        .deserialize_with::<Vec<u32>, _>(|driver| validation.setup(driver));
    let report = validation.finish(rv).into_result().unwrap_err();
    assert_eq!(report.len(), 2);
}

#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    port: Validated<u16, Range<1, 65535>>,
    #[deser(as = Check<NonEmpty>)]
    name: String,
}

#[test]
fn test_serialize() {
    let settings: Settings = deser_json::from_str(r#"{"port": 0, "name": "x"}"#).unwrap();
    assert!(!settings.port.is_valid());
    // invalid values are written as they were read
    assert_eq!(
        deser_json::to_string(&settings).unwrap(),
        r#"{"port":0,"name":"x"}"#
    );
    let settings: Settings = deser_json::from_str(r#"{"port": "x", "name": "x"}"#).unwrap();
    assert_eq!(
        deser_json::to_string(&settings).unwrap(),
        r#"{"port":null,"name":"x"}"#
    );
}

deser_validate::validator!(NonZero(port: &u16) => *port != 0, "must not be zero");

#[derive(Debug, Deserialize)]
struct Listener {
    #[deser(as = deser_validate::Check<NonZero>)]
    port: u16,
    #[deser(as = deser::adapters::VecSkipError<deser_validate::Check<Email>>)]
    admins: Vec<String>,
    #[deser(as = Option<deser_validate::Check<Email>>)]
    contact: Option<String>,
}

#[test]
fn test_check_adapter() {
    let listener: Listener = deser_json::from_str(
        r#"{"port": 80, "admins": ["a@example.com", "nope", "b@example.com"]}"#,
    )
    .unwrap();
    // invalid elements are skipped
    assert_eq!(listener.admins, ["a@example.com", "b@example.com"]);
    assert_eq!(listener.contact, None);

    // all errors are reported
    let validation = Validation::new();
    let rv = deser_json::Deserializer::from_str(r#"{"port": 0, "admins": [], "contact": "x"}"#)
        .deserialize_with::<Listener, _>(|driver| validation.setup(driver));
    let report = validation.finish(rv).into_result().unwrap_err();
    let codes: Vec<_> = report
        .iter()
        .map(|issue| {
            format!(
                "{} {}",
                issue.path().unwrap(),
                issue.violation().unwrap().code()
            )
        })
        .collect();
    assert_eq!(codes, ["port non_zero", "contact email"]);
}

#[test]
fn test_check_adapter_update() {
    use deser::de::Deserializer;

    let mut listener: Listener = deser_json::from_str(r#"{"port": 80, "admins": []}"#).unwrap();
    // fields are updated and then checked, the value keeps the update
    let err = deser_json::Deserializer::from_str(r#"{"port": 0}"#)
        .update(&mut listener)
        .unwrap_err();
    assert_eq!(err.message(), "invalid value: must not be zero");
    assert_eq!(listener.port, 0);
    deser_json::Deserializer::from_str(r#"{"port": 8080}"#)
        .update(&mut listener)
        .unwrap();
    assert_eq!(listener.port, 8080);
}

#[derive(Debug, Deserialize)]
#[deser(deserialize_as = deser_validate::Check<OrderedBounds, _>)]
struct Bounds {
    min: u32,
    max: u32,
}

deser_validate::validator!(OrderedBounds(bounds: &Bounds) => bounds.min <= bounds.max, "min is larger than max");

#[derive(Debug, Deserialize)]
struct ServerLimits {
    connections: Bounds,
    #[deser(as = deser_validate::Check<NonZero>)]
    port: u16,
}

#[test]
fn test_check_container() {
    let limits: ServerLimits =
        deser_json::from_str(r#"{"connections": {"min": 1, "max": 5}, "port": 80}"#).unwrap();
    assert_eq!(limits.connections.max, 5);

    // the error points at the start of the value
    let err = with_paths::<ServerLimits>(r#"{"connections": {"min": 6, "max": 5}, "port": 80}"#)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidValue: invalid value: min is larger than max at line 1 column 17 (path: connections)"
    );
    assert_eq!(
        err.attachment::<Violation>().unwrap().code(),
        "ordered_bounds"
    );

    // values that keep their errors
    let bounds: Validated<Bounds> = deser_json::from_str(r#"{"min": 6, "max": 5}"#).unwrap();
    assert_eq!(
        bounds.error().unwrap().message(),
        "invalid value: min is larger than max"
    );
}

#[test]
fn test_check_container_update() {
    use deser::de::Deserializer;

    let mut limits: ServerLimits =
        deser_json::from_str(r#"{"connections": {"min": 1, "max": 5}, "port": 80}"#).unwrap();

    // the update merges into the value, which is checked once it's complete
    deser_json::Deserializer::from_str(r#"{"connections": {"max": 10}}"#)
        .update(&mut limits)
        .unwrap();
    assert_eq!((limits.connections.min, limits.connections.max), (1, 10));

    let err = deser_json::Deserializer::from_str(r#"{"connections": {"min": 20}}"#)
        .update_with(&mut limits, |driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidValue: invalid value: min is larger than max at line 1 column 17 (path: connections)"
    );
}
