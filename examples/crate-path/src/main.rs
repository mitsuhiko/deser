//! This example shows how to use the derive when deser is not available as
//! `deser`, for instance because it is renamed in `Cargo.toml` (as done here)
//! or re-exported by another crate.  `#[deser(crate = path)]` tells the
//! derive where to find it.

/// A facade module re-exporting deser, as a framework crate might do.
pub mod framework {
    pub use serialization as ser;
}

/// Uses the renamed crate directly.
#[derive(Debug, PartialEq, serialization::Serialize, serialization::Deserialize)]
#[deser(crate = serialization)]
pub struct User {
    name: String,
    #[deser(default = 42)]
    age: u32,
}

/// Uses the re-export.  Enums with struct variants go through helper
/// structs, which also need to find the crate.
#[derive(Debug, PartialEq, framework::ser::Serialize, framework::ser::Deserialize)]
#[deser(crate = crate::framework::ser, tag = "type")]
pub enum Event<T> {
    Login { user: User, extra: T },
    Logout,
}

fn main() {
    let event: Event<bool> =
        deser_json::from_str(r#"{"user": {"name": "Peter"}, "extra": true, "type": "Login"}"#)
            .unwrap();
    println!("{:?}", event);
    println!("{}", deser_json::to_string(&event).unwrap());
}

#[test]
fn test_roundtrip() {
    let user: User = deser_json::from_str(r#"{"name": "Peter"}"#).unwrap();
    assert_eq!(
        user,
        User {
            name: "Peter".into(),
            age: 42
        }
    );

    let event = Event::Login {
        user,
        extra: vec![1u32, 2],
    };
    let json = deser_json::to_string(&event).unwrap();
    assert_eq!(
        json,
        r#"{"type":"Login","user":{"name":"Peter","age":42},"extra":[1,2]}"#
    );
    assert_eq!(
        deser_json::from_str::<Event<Vec<u32>>>(&json).unwrap(),
        event
    );
    assert_eq!(
        deser_json::from_str::<Event<()>>(r#"{"type": "Logout"}"#).unwrap(),
        Event::Logout
    );
}
