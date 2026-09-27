//! Generic types: bounds, lifetimes and const parameters.
//!
//! By default the derive requires every type parameter to implement the
//! derived trait.  That's wrong for parameters which are not serialized
//! themselves, like a storage backend of which only the associated `Id`
//! type is serialized.  The bounds can be replaced:
//!
//! * `bound(...)` on the type replaces all inferred bounds (for both
//!   derives, `DeserializeOwned` stands in for `Deserialize<'de>`),
//! * `serialize_bound(...)` and `deserialize_bound(...)` replace them for
//!   one derive (with `'de` for the lifetime of the data),
//! * placed on a field, they only replace the bounds inferred from that
//!   field and other fields keep theirs.
//!
//! `#[derive(Debug)]` has the same problem (it would require the backends
//! to be `Debug`), so the values are formatted with `deser-debug` which
//! goes through `Serialize` and its bounds.
//!
//! Enums can have lifetime and const parameters like structs: they borrow
//! from the data and can be generic over the size of arrays.
use deser::de::DeserializeOwned;
use deser::{Deserialize, Serialize};
use deser_debug::ToDebug;

/// A storage backend with its own type of ids.  Backends are markers and
/// never serialized.
pub trait Backend {
    type Id;
}

pub struct Postgres;

impl Backend for Postgres {
    type Id = i64;
}

pub struct Couch;

impl Backend for Couch {
    type Id = String;
}

/// A record of a backend.  Only the bounds inferred from `id` (which would
/// be `B: Serialize`) are replaced, `T: Serialize` is still inferred from
/// `data`.
#[derive(Serialize, Deserialize)]
pub struct Record<B: Backend, T> {
    #[deser(
        serialize_bound(B::Id: Serialize),
        deserialize_bound(B::Id: Deserialize<'de>)
    )]
    id: B::Id,
    data: T,
}

/// Where to continue listing records.  One bound for both derives replaces
/// the inferred ones.
#[derive(Serialize, Deserialize)]
#[deser(bound(B::Id: Serialize + DeserializeOwned))]
pub struct Cursor<B: Backend> {
    after: B::Id,
    limit: u32,
}

#[derive(Serialize, Deserialize)]
pub struct User {
    name: String,
}

/// A change to a table, the name of the table is borrowed from the data.
#[derive(Serialize, Deserialize)]
#[deser(
    tag = "op",
    rename_all = "snake_case",
    serialize_bound(B::Id: Serialize),
    deserialize_bound(B::Id: Deserialize<'de>)
)]
pub enum Change<'a, B: Backend> {
    Insert { table: &'a str, id: B::Id },
    Delete { table: &'a str, id: B::Id },
    Truncate { table: &'a str },
}

/// A shape in any number of dimensions.
#[derive(Serialize, Deserialize)]
#[deser(rename_all = "snake_case")]
pub enum Shape<const D: usize> {
    Point([f64; D]),
    Sphere { center: [f64; D], radius: f64 },
}

fn main() {
    // the same record type with the ids of different backends
    let json = r#"{"id": 42, "data": {"name": "Jane"}}"#;
    let record: Record<Postgres, User> = deser_json::from_str(json).unwrap();
    println!("{:?}", ToDebug::new(&record));
    assert_eq!(record.id, 42);

    let json = r#"{"id": "user:jane", "data": {"name": "Jane"}}"#;
    let record: Record<Couch, User> = deser_json::from_str(json).unwrap();
    println!("{:?}", ToDebug::new(&record));
    assert_eq!(record.id, "user:jane");

    // the id has to fit the backend
    let err = deser_json::from_str::<Record<Postgres, User>>(json)
        .err()
        .unwrap();
    println!("error: {}\n", err);

    let cursor: Cursor<Couch> = Cursor {
        after: "user:jane".into(),
        limit: 50,
    };
    let json = deser_json::to_string(&cursor).unwrap();
    println!("{}", json);
    let cursor: Cursor<Couch> = deser_json::from_str(&json).unwrap();
    assert_eq!(cursor.limit, 50);

    // enums borrow from the data
    let input = r#"[
        {"op": "insert", "table": "users", "id": 1},
        {"op": "delete", "table": "users", "id": 1},
        {"op": "truncate", "table": "sessions"}
    ]"#;
    let changes: Vec<Change<Postgres>> = deser_json::from_str(input).unwrap();
    println!("\n{:?}", ToDebug::new(&changes));
    let Change::Insert { table, .. } = changes[0] else {
        unreachable!()
    };
    assert!(input.as_bytes().as_ptr_range().contains(&table.as_ptr()));

    // and the size of arrays is a parameter
    let flat: Vec<Shape<2>> =
        deser_json::from_str(r#"[{"point": [1, 2]}, {"sphere": {"center": [0, 0], "radius": 1}}]"#)
            .unwrap();
    println!("\n{:?}", ToDebug::new(&flat));
    let solid: Shape<3> =
        deser_json::from_str(r#"{"sphere": {"center": [0, 0, 0], "radius": 1}}"#).unwrap();
    println!("{:?}", ToDebug::new(&solid));

    let err = deser_json::from_str::<Shape<2>>(r#"{"point": [1, 2, 3]}"#)
        .err()
        .unwrap();
    println!("error: {}", err);
}
