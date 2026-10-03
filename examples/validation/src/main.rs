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
//!
//! Validators are types.  `validator!` turns a function (or a condition)
//! into one.
use deser::{Context, Deserialize, TrackLocations};
use deser_validate::{
    Check, Each, Email, Len, MaxLen, NonEmpty, Range, Validated, Validation, validator,
};

/// Checks a lowercase identifier like `jane-doe`.
///
/// This is a plain function, `validator!` below turns it into the
/// validator type `Slug`.  It returns a message if the value is invalid,
/// the code of the violation is the name of the validator (`slug`).
fn check_slug(value: &str) -> Result<(), &'static str> {
    let valid = value
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if valid && !value.is_empty() {
        Ok(())
    } else {
        Err("may only contain lowercase letters, digits and dashes")
    }
}

// A validator of `str` validates `String`s (and everything else that
// borrows as `str`) too.
validator!(pub Slug(value: &str) = check_slug);

// Simple rules are a condition and a message.
validator!(pub Quantity(quantity: &u32) => (1..=100).contains(quantity), "must be between 1 and 100");

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
    #[deser(as = Check<NonEmpty>)]
    street: String,
    city: String,
    #[deser(as = Check<Len<4, 10>>)]
    zip: String,
}

/// Validators work as adapters (`Check`), the fields keep their types.
#[derive(Debug, Deserialize)]
pub struct OrderLine {
    #[deser(as = Check<Slug>)]
    sku: String,
    #[deser(as = Check<Quantity>)]
    quantity: u32,
}

/// An order submitted to a JSON API.
#[derive(Debug, Deserialize)]
pub struct Order {
    #[deser(as = Check<Email>)]
    customer: String,
    #[deser(as = Check<(NonEmpty, MaxLen<50>)>)]
    lines: Vec<OrderLine>,
    shipping: Address,
    #[deser(as = Check<Each<MaxLen<200>>>)]
    notes: Vec<String>,
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
    let config = deser_json::DeserializerConfig::builder()
        .context(Context::with(TrackLocations(true)))
        .build();
    let mut de = deser_json::Deserializer::from_str_with_config(request, config);
    let mut validation = Validation::new();
    validation.set_max_errors(100);
    let rv = de.deserialize_with::<Order, _>(|driver| validation.setup(driver));
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
            "lines[1].quantity [quantity]: invalid value: must be between 1 and 100 (line 5)",
            "lines[2].quantity [invalid_type]: unexpected string, expected u32 (line 6)",
            "shipping.street [non_empty]: invalid value: must not be empty (line 8)",
            "shipping.zip [len]: invalid value: length must be between 4 and 10 (line 8)",
            "shipping [invalid_type]: missing field `city` (line 8)",
            "gift_message [max_len]: invalid value: item 0: length must be at most 20 (line 10)",
        ]
    );

    // `Validated` keeps all errors of its value, for instance to accept an
    // order but report a broken address
    #[derive(Debug, Deserialize)]
    struct Lenient {
        shipping: Validated<Address>,
    }
    let lenient: Lenient = deser_json::from_str(request).unwrap();
    let err = lenient.shipping.error().unwrap();
    println!("shipping has {} problems", err.errors().count());
    assert_eq!(err.errors().count(), 3);
}

fn main() {
    form();
    api();
}
