//! CSV and TSV files with `deser-csv`.
//!
//! A CSV export of a shop: every row is an order, the columns depend on
//! the kind of the order (an internally tagged enum that is flattened
//! into the row), tags are a list in a single field and empty fields are
//! missing values.  Rows are read one at a time from a stream, rows with
//! errors are reported with their line and column and skipped.  Then the
//! orders are written as CSV and as TSV.
use deser::adapters::Separated;
use deser::{Deserialize, Serialize};
use deser_csv::{DeserializerConfig, Nulls, SerializerConfig};
use deser_path::{Path, PathLayer};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Order {
    id: u32,
    customer: String,
    #[deser(flatten)]
    item: Item,
    /// a list in a single field (an empty field is null)
    #[deser(as = Option<Separated<';'>>)]
    tags: Option<Vec<String>>,
    note: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "kind", rename_all = "lowercase")]
enum Item {
    Book { isbn: String, copies: u32 },
    Gift { value: f64 },
}

/// A file as spreadsheets write it: a byte order mark, `\r\n` and quotes
/// around fields with commas, quotes and line breaks.
const INPUT: &str = "\u{feff}id,customer,kind,isbn,copies,value,tags,note\r\n\
    1,\"Doe, Jane\",book,978-3-16-148410-0,2,,gift;express,\"wrap it, please\"\r\n\
    2,John,gift,,,25.5,,\r\n\
    3,Max,book,978-0-306-40615-7,many,,,\r\n\
    4,Anna,gift,,,10,express,\"first line\r\nsecond \"\"line\"\"\"\r\n";

fn main() {
    // empty fields are null, so `copies` of a gift is missing instead of
    // an empty number
    let config = DeserializerConfig::builder().nulls(Nulls::Empty).build();
    let mut reader = config.reader(INPUT.as_bytes());
    let mut orders = Vec::new();
    loop {
        match reader.read_with::<Order, _>(|driver| driver.push_layer(PathLayer::new())) {
            Ok(Some(order)) => orders.push(order),
            Ok(None) => break,
            Err(err) => {
                // the error only discards its row
                println!("skipped: {}", err);
                assert_eq!(err.attachment::<Path>().unwrap().to_string(), "copies");
                assert_eq!(err.line(), Some(4));
            }
        }
    }
    println!("columns: {:?}", reader.deserializer().headers().unwrap());
    for order in &orders {
        println!("{:?}", order);
    }
    assert_eq!(orders.len(), 3);
    assert_eq!(orders[0].customer, "Doe, Jane");
    assert_eq!(orders[0].tags.as_deref().unwrap(), ["gift", "express"]);
    assert_eq!(orders[1].tags, None);
    assert_eq!(orders[1].item, Item::Gift { value: 25.5 });
    assert_eq!(
        orders[2].note.as_deref(),
        Some("first line\r\nsecond \"line\"")
    );

    // written back as CSV.  The columns are given as the first order does
    // not have all of them (it's a book), missing fields are empty.
    const COLUMNS: &[&str] = &[
        "id", "customer", "kind", "isbn", "copies", "value", "tags", "note",
    ];
    let csv = SerializerConfig::builder()
        .columns(COLUMNS)
        .build()
        .to_string(&orders)
        .unwrap();
    println!("\n{}", csv);
    let back: Vec<Order> = config.from_str(&csv).unwrap();
    assert_eq!(back, orders);

    // TSV as databases write it: escapes instead of quotes and `\N` for
    // null
    let mut config = SerializerConfig::tsv();
    config.set_columns(COLUMNS);
    let tsv = config.to_string(&orders).unwrap();
    println!("{}", tsv);
    let back: Vec<Order> = DeserializerConfig::tsv().from_str(&tsv).unwrap();
    assert_eq!(back, orders);
}
