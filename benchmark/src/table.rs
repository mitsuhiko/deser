//! A table of 20,000 generated rows for CSV, compared with the `csv`
//! crate.
//!
//! The columns are what an export of a database table looks like: ids,
//! text (some of it quoted because it contains commas, quotes or line
//! breaks), floats, integers, booleans and empty optional fields (an
//! empty field is `None` for an `Option<u32>` with both libraries).  The
//! whole table is read into a `Vec` and written from it.
use deser::{Deserialize, Serialize};

/// The number of rows.
const ROWS: usize = 20_000;

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
pub struct Row {
    id: u64,
    created_at: String,
    name: String,
    email: String,
    country: String,
    score: f64,
    visits: u32,
    active: bool,
    referrer: Option<u32>,
    note: String,
}

pub fn rows() -> Vec<Row> {
    const COUNTRIES: [&str; 5] = ["AT", "DE", "US", "JP", "BR"];
    (0..ROWS)
        .map(|index| Row {
            id: 1_000_000 + index as u64,
            created_at: format!(
                "2024-{:02}-{:02}T{:02}:{:02}:00Z",
                index % 12 + 1,
                index % 28 + 1,
                index % 24,
                index % 60
            ),
            name: format!("User {}", index),
            email: format!("user{}@example.com", index),
            country: COUNTRIES[index % COUNTRIES.len()].to_string(),
            score: (index as f64 * 0.37) % 100.0,
            visits: (index * 7 % 1000) as u32,
            active: index % 3 != 0,
            referrer: (index % 4 == 0).then_some((index % 97) as u32),
            note: match index % 10 {
                0 => format!("said \"hello\", {}", index),
                1 => "line one\nline two".to_string(),
                _ => String::new(),
            },
        })
        .collect()
}

pub fn csv(rows: &[Row]) -> String {
    deser_csv::to_string(&rows).unwrap()
}

pub fn deser_de(input: &str) -> Vec<Row> {
    deser_csv::from_str(input).unwrap()
}

pub fn serde_de(input: &str) -> Vec<Row> {
    csv::Reader::from_reader(input.as_bytes())
        .deserialize()
        .collect::<Result<_, _>>()
        .unwrap()
}

pub fn deser_ser(rows: &[Row]) -> String {
    deser_csv::to_string(&rows).unwrap()
}

pub fn serde_ser(rows: &[Row]) -> Vec<u8> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for row in rows {
        writer.serialize(row).unwrap();
    }
    writer.into_inner().unwrap()
}
