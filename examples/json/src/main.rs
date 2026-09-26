//! The basics: deriving `Serialize` and `Deserialize` and using them with
//! JSON.
//!
//! * `rename_all` renames the fields and variants,
//! * `flatten` merges the fields of another struct into the parent (without
//!   buffering),
//! * `Option` fields are optional and `skip_serializing_if` leaves out
//!   values when serializing.
//!
//! The types do not derive `Debug`: `deser-debug` formats any value that
//! can be serialized like `#[derive(Debug)]` would.
use deser::{Deserialize, Serialize};
use deser_debug::ToDebug;

#[derive(Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct User {
    id: u64,
    email_address: String,
    kind: UserKind,
    #[deser(skip_serializing_if = Vec::is_empty, default)]
    tags: Vec<String>,
    #[deser(flatten)]
    attributes: UserAttributes,
}

#[derive(Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct UserAttributes {
    is_special: bool,
    display_name: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[deser(rename_all = "snake_case")]
pub enum UserKind {
    Admin,
    RegularUser,
}

fn main() {
    let user: User = deser_json::from_str(
        r#"{
            "id": 23,
            "emailAddress": "jane@example.com",
            "kind": "regular_user",
            "isSpecial": true
        }"#,
    )
    .unwrap();

    println!("{:#?}", ToDebug::new(&user));

    let json = deser_json::to_string(&user).unwrap();
    println!("{}", json);
    assert_eq!(
        json,
        r#"{"id":23,"emailAddress":"jane@example.com","kind":"regular_user","isSpecial":true,"displayName":null}"#
    );
}
