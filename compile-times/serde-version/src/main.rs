use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    id: usize,
    email_address: String,
    kind: UserKind,
    is_special: bool,
    is_powerful: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserKind {
    Admin,
    User,
}

fn main() {
    let user: User = serde_json::from_str(
        r#"
        {
            "id": 23,
            "emailAddress": "jane@example.com",
            "kind": "admin",
            "isPowerful": true,
            "isSpecial": true
        }
    "#,
    )
    .unwrap();

    println!("{}", serde_json::to_string(&user).unwrap());
}
