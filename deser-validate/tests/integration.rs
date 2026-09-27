use deser::de::Limits;
use deser::{Deserialize, Serialize};
use deser_path::{Path, PathLayer};
use deser_validate::{
    Checked, Collect, Each, Email, Len, MaxLen, NonEmpty, Range, Validated, Validation, Violation,
};

#[derive(Debug, Deserialize)]
struct Address {
    street: String,
    zip: Checked<u32, Range<1000, 99999>>,
}

#[derive(Debug, Deserialize)]
struct Signup {
    name: Checked<String, (NonEmpty, MaxLen<16>)>,
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

fn with_paths<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, deser::Error> {
    let config = deser_json::DeserializerConfig::new().track_locations(true);
    deser_json::Deserializer::from_str_with_config(input, &config)
        .deserialize_with(|driver| driver.push_layer(PathLayer::new()))
}

#[test]
fn test_valid() {
    let signup: Signup = deser_json::from_str(VALID).unwrap();
    assert_eq!(*signup.name, "jane");
    assert_eq!(signup.email.value().unwrap(), "jane@example.com");
    assert_eq!(*signup.age.value().unwrap(), 30);
    assert_eq!(*signup.address.value().unwrap().zip, 12345);
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
        "Unexpected: invalid value: must be an email address at line 3 column 14 (path: email)"
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
        "Unexpected: unexpected sequence, expected u8 at line 4 column 12 (path: age)"
    );

    // errors deep inside the value, the rest of it was skipped
    assert!(signup.address.unchecked_value().is_none());
    assert_eq!(
        error(signup.address.error()),
        "Unexpected: unexpected map, expected u32 at line 5 column 45 (path: address.zip)"
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
        "Unexpected: invalid value: must not be empty at line 2 column 13 (path: name)"
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
            driver.push_layer(Limits::new().max_depth(3))
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
    shipping: Validated<Collect<Address>>,
    lines: Collect<Vec<u32>>,
}

#[test]
fn test_collect() {
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
    assert_eq!(*order.lines, [1, 2]);

    // collecting ends with the value
    let err =
        deser_json::from_str::<Order>(r#"{"id": "x", "shipping": {}, "lines": [1, "a", "b"]}"#)
            .unwrap_err();
    assert_eq!(err.error_count(), 1);
    let err = deser_json::from_str::<Order>(r#"{"id": 1, "shipping": {}, "lines": [1, "a", "b"]}"#)
        .unwrap_err();
    assert_eq!(err.error_count(), 2);
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
    let config = deser_json::DeserializerConfig::new().track_locations(true);
    let rv = deser_json::Deserializer::from_str_with_config(INVALID, &config)
        .deserialize_with::<Signup, _>(|driver| validation.setup(driver));
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
    let validation = Validation::new().max_errors(1);
    let rv = deser_json::Deserializer::from_str(r#"[1, "a", "b", "c"]"#)
        .deserialize_with::<Vec<u32>, _>(|driver| validation.setup(driver));
    let report = validation.finish(rv).into_result().unwrap_err();
    assert_eq!(report.len(), 2);
}

#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    port: Validated<u16, Range<1, 65535>>,
    name: Checked<String, NonEmpty>,
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
