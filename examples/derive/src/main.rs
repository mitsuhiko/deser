use deser::de::Driver;
use deser::{Deserialize, Event, Serialize};
use deser_debug::ToDebug;

#[derive(Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct User {
    id: usize,
    email_address: String,
    kind: UserKind,
}

#[derive(Serialize, Deserialize)]
#[deser(rename_all = "snake_case")]
pub enum UserKind {
    Admin,
    User,
}

fn main() {
    let mut user = None::<User>;
    {
        let mut driver = Driver::new(&mut user);
        driver.emit(Event::MapStart).unwrap();
        driver.emit("id").unwrap();
        driver.emit(23u64).unwrap();
        driver.emit("emailAddress").unwrap();
        driver.emit("jane@example.com").unwrap();
        driver.emit("kind").unwrap();
        driver.emit("admin").unwrap();
        driver.emit(Event::MapEnd).unwrap();
    }
    println!("{:#?}", ToDebug::new(&user.unwrap()));
}
