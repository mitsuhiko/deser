use deser::Serialize;
use deser_debug::ToDebug;

#[derive(Serialize)]
#[deser(rename_all = "camelCase")]
pub struct User {
    id: usize,
    email_address: String,
}

fn main() {
    let user = User {
        id: 42,
        email_address: "john@example.com".into(),
    };
    println!("{:#?}", ToDebug::new(&user))
}
