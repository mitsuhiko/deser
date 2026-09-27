//! Validating input with `deser-validate`.
//!
//! Two typical cases:
//!
//! * An HTML form is submitted with invalid values.  The form is shown
//!   again with the values the user entered and an error next to every
//!   invalid field.  For that the form has to deserialize even if fields
//!   are invalid: `Validated` fields keep their errors (and the value if
//!   it could be read at all).
//! * A JSON API rejects a request with a list of all problems, not just
//!   the first one.  A `Validation` collects them with their paths and the
//!   codes of the rules that were violated.
use deser::Deserialize;
use deser_validate::{
    Checked, Collect, Each, Email, Len, MaxLen, NonEmpty, Range, Validated, Validation, Validator,
    Violation,
};

/// A lowercase identifier like `jane-doe`.
pub struct Slug;

impl<T: AsRef<str> + ?Sized> Validator<T> for Slug {
    fn validate(value: &T) -> Result<(), Violation> {
        let value = value.as_ref();
        let valid = value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if valid && !value.is_empty() {
            Ok(())
        } else {
            Err(Violation::new(
                "slug",
                "may only contain lowercase letters, digits and dashes",
            ))
        }
    }
}

/// The signup form of a website.
#[derive(Debug, Deserialize)]
pub struct SignupForm {
    username: Validated<String, (Slug, Len<3, 20>)>,
    email: Validated<String, Email>,
    age: Validated<u8, Range<13, 130>>,
    #[deser(default)]
    newsletter: bool,
}

/// Renders a field of the form: the value the user entered and the error.
fn field<T: ToString, V>(name: &str, value: &Validated<T, V>) -> String {
    let entered = value
        .unchecked_value()
        .map(|x| x.to_string())
        .unwrap_or_default();
    match value.error() {
        None => format!("{name}: {entered:?}"),
        Some(err) => format!("{name}: {entered:?} <- {}", err.message()),
    }
}

fn form() {
    println!("-- HTML form --");
    let body = "username=Jane+Doe&email=jane%40example.com&age=eleven&newsletter=on";
    let form: SignupForm = deser_urlencoded::from_str(body).unwrap();

    let rendered = [
        field("username", &form.username),
        field("email", &form.email),
        field("age", &form.age),
    ];
    for line in &rendered {
        println!("{line}");
    }
    assert_eq!(
        rendered,
        [
            r#"username: "Jane Doe" <- invalid value: may only contain lowercase letters, digits and dashes"#,
            r#"email: "jane@example.com""#,
            r#"age: "" <- invalid value "eleven", expected u8"#,
        ]
    );
    assert!(form.newsletter);
}

#[derive(Debug, Deserialize)]
pub struct Address {
    street: Checked<String, NonEmpty>,
    city: String,
    zip: Checked<String, Len<4, 10>>,
}

#[derive(Debug, Deserialize)]
pub struct OrderLine {
    sku: Checked<String, Slug>,
    quantity: Checked<u32, Range<1, 100>>,
}

/// An order submitted to a JSON API.
#[derive(Debug, Deserialize)]
pub struct Order {
    customer: Checked<String, Email>,
    lines: Checked<Vec<OrderLine>, (NonEmpty, MaxLen<50>)>,
    shipping: Address,
    notes: Checked<Vec<String>, Each<MaxLen<200>>>,
    // the gift message is optional, if it's invalid it's dropped (with a
    // warning) rather than rejecting the order.  Like options it can be
    // missing.
    gift_message: Validated<Option<String>, Each<MaxLen<20>>>,
}

fn api() {
    println!("-- JSON API --");
    let request = r#"{
        "customer": "jane@example",
        "lines": [
            {"sku": "red-shoes", "quantity": 2},
            {"sku": "Blue Shirt", "quantity": 0},
            {"sku": "socks", "quantity": "three"}
        ],
        "shipping": {"street": "", "zip": "1"},
        "notes": ["leave at the door"],
        "gift_message": "Happy birthday, have a wonderful day!"
    }"#;

    // with the locations tracked, the errors that values keep (like the
    // gift message) have lines and columns too, not only the errors that
    // are returned
    let config = deser_json::DeserializerConfig::new().track_locations(true);
    let validation = Validation::new().max_errors(100);
    let rv = deser_json::Deserializer::from_str_with_config(request, &config)
        .deserialize_with::<Order, _>(|driver| validation.setup(driver));
    let report = validation.finish(rv).into_result().unwrap_err();

    // what an API would answer with
    let problems: Vec<_> = report
        .iter()
        .map(|issue| {
            let code = issue.violation().map_or("invalid_type", |x| x.code());
            format!(
                "{} [{}]: {} (line {})",
                issue.path().unwrap_or(""),
                code,
                issue.message(),
                issue.line().map_or("?".into(), |x| x.to_string()),
            )
        })
        .collect();
    for problem in &problems {
        println!("{problem}");
    }
    assert_eq!(
        problems,
        [
            "customer [email]: invalid value: must be an email address (line 2)",
            "lines[1].sku [slug]: invalid value: may only contain lowercase letters, digits and dashes (line 5)",
            "lines[1].quantity [range]: invalid value: must be between 1 and 100 (line 5)",
            "lines[2].quantity [invalid_type]: unexpected string, expected u32 (line 6)",
            "shipping.street [non_empty]: invalid value: must not be empty (line 8)",
            "shipping.zip [length]: invalid value: length must be between 4 and 10 (line 8)",
            "shipping [invalid_type]: missing field `city` (line 8)",
            "gift_message [length]: invalid value: item 0: length must be at most 20 (line 10)",
        ]
    );

    // parts of an input can collect all of their errors on their own, for
    // instance to accept an order but report a broken address
    #[derive(Debug, Deserialize)]
    struct Lenient {
        shipping: Validated<Collect<Address>>,
    }
    let lenient: Lenient = deser_json::from_str(request).unwrap();
    let err = lenient.shipping.error().unwrap();
    println!("shipping has {} problems", err.error_count());
    assert_eq!(err.error_count(), 3);
}

fn main() {
    form();
    api();
}
