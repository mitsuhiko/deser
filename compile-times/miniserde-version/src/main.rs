use miniserde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct User {
    id: usize,
    #[serde(rename = "emailAddress")]
    email_address: String,
    kind: UserKind,
    #[serde(rename = "isSpecial")]
    is_special: bool,
    #[serde(rename = "isPowerful")]
    is_powerful: bool,
}

#[derive(Serialize, Deserialize)]
pub enum UserKind {
    #[serde(rename = "admin")]
    Admin,
    #[serde(rename = "user")]
    User,
}

fn main() {
    let user: User = miniserde::json::from_str(
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

    println!("{}", miniserde::json::to_string(&user));
}
