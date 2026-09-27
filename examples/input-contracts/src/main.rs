//! What input is accepted, and what errors say when it is not.
//!
//! These are the requests of an issue tracker's API.  The types spell out
//! the contract of the API:
//!
//! * `transparent` makes a struct with named fields look like its only
//!   field on the wire (an email address is a string), other fields are
//!   skipped and cannot be set by clients,
//! * `expecting` describes a value in errors in the words of the API,
//!   instead of the name of the Rust type,
//! * `required` makes an `Option` field required: the key has to be given,
//!   but it can be null, so that "no parent" is a decision and not a
//!   forgotten key,
//! * `deny_unknown_fields` on a single variant rejects typos where they
//!   would lose data, while the other variants ignore unknown keys.
//!
//! Errors carry the path of the value with `deser-path`.
use deser::{Deserialize, Error, Serialize};
use deser_path::PathLayer;

/// An email address, written as a string.  Whether it's verified is known
/// by the server, a client cannot claim it.
#[derive(Debug, Serialize, Deserialize)]
#[deser(transparent, validate = check_email)]
pub struct Email {
    address: String,
    #[deser(skip)]
    verified: bool,
}

fn check_email(email: &Email) -> Result<(), String> {
    match email.address.split_once('@') {
        Some((user, domain)) if !user.is_empty() && domain.contains('.') => Ok(()),
        _ => Err(format!("`{}` is not an email address", email.address)),
    }
}

/// Errors call this "a label", not `Label`.
#[derive(Debug, Serialize, Deserialize)]
#[deser(expecting = "a label")]
pub struct Label {
    name: String,
    color: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[deser(tag = "action", rename_all = "snake_case", expecting = "a request")]
pub enum Request {
    /// A typo in a key of a new issue would silently drop what the user
    /// entered, so unknown keys are rejected.
    #[deser(deny_unknown_fields)]
    Create {
        title: String,
        /// The key is required, but null is fine.
        #[deser(required)]
        parent: Option<u64>,
        assignee: Option<Email>,
        #[deser(default)]
        labels: Vec<Label>,
    },
    /// Searches ignore unknown keys (clients add tracking parameters).
    Search {
        query: String,
        #[deser(default)]
        labels: Vec<Label>,
    },
}

fn parse(json: &str) -> Result<Request, Error> {
    deser_json::Deserializer::from_str(json)
        .deserialize_with(|driver| driver.push_layer(PathLayer::new()))
}

fn main() {
    let request = parse(
        r#"{
            "action": "create",
            "title": "Crash on start",
            "parent": null,
            "assignee": "jane@example.com",
            "labels": [{"name": "bug"}, {"name": "p1", "color": "red"}]
        }"#,
    )
    .unwrap();
    println!("{:#?}", request);
    let Request::Create { assignee, .. } = &request else {
        unreachable!()
    };
    // the email address is a string on the wire, and not verified
    assert!(!assignee.as_ref().unwrap().verified);
    let json = deser_json::to_string(&request).unwrap();
    println!("{}\n", json);
    assert!(json.contains(r#""assignee":"jane@example.com""#));
    assert!(json.contains(r#""parent":null"#));

    // unknown keys are fine in searches
    let search = parse(r#"{"action": "search", "query": "crash", "utm_source": "mail"}"#);
    println!("{:?}\n", search.unwrap());

    for (what, json) in [
        (
            "the parent is required, even if it's an option",
            r#"{"action": "create", "title": "Crash"}"#,
        ),
        (
            "typos are errors when creating issues",
            r#"{"action": "create", "title": "Crash", "parent": null, "lables": ["bug"]}"#,
        ),
        (
            "the email address is deserialized like a string",
            r#"{"action": "create", "title": "Crash", "parent": 1, "assignee": 42}"#,
        ),
        (
            "and then validated",
            r#"{"action": "create", "title": "Crash", "parent": 1, "assignee": "jane"}"#,
        ),
        (
            "labels are described in the words of the API",
            r#"{"action": "search", "query": "crash", "labels": ["bug"]}"#,
        ),
        ("and so are requests", r#"["search", "crash"]"#),
        (
            "and the fields that the server sets are not taken from the client",
            r#"{"action": "create", "title": "Crash", "parent": null,
                "assignee": {"address": "jane@example.com", "verified": true}}"#,
        ),
    ] {
        let err = parse(json).unwrap_err();
        println!("{}:\n  {}", what, err);
    }
}
