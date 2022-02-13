use deser::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct User {
    id: usize,
    email_address: String,
    kind: UserKind,
    is_special: bool,
    is_powerful: bool,
}

#[derive(Serialize, Deserialize)]
#[deser(rename_all = "snake_case")]
pub enum UserKind {
    Admin,
    User,
}

fn main() {
    let user: User = deser_json::from_str(
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

    println!("{}", deser_json::to_string(&user).unwrap());
}
