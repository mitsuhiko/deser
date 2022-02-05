use deser::Serialize;
use deser_debug::ToDebug;

#[derive(Serialize)]
pub struct User {
    id: usize,
    email: String,
}

fn main() {
    println!(
        "{:#?}",
        ToDebug::new(&User {
            id: 42,
            email: "john@example.com".into(),
        })
    )
}
