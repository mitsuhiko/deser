//! Reading and writing JSON Lines (NDJSON).
//!
//! With `Trailing::Newline` a `deser_json::Deserializer` reads one value per
//! line.  An error (a syntax error or a value that does not fit the type)
//! only discards its line: the error reports the line and reading continues
//! with the next one.
use deser::{Deserialize, Serialize};
use deser_json::{Deserializer, DeserializerConfig, Trailing};

#[derive(Debug, Serialize, Deserialize)]
#[deser(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Login { user: String },
    Upload { user: String, bytes: u64 },
    Logout { user: String },
}

const LINES: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);

const INPUT: &str = r#"{"event": "login", "user": "jane"}
{"event": "upload", "user": "jane", "bytes": 1024}

{"event": "upload", "user": "jane", "bytes": -1}
{"event": "upload", "user": "jane", "bytes": 2048
{"event": "logout", "user": "jane"}
"#;

fn main() {
    let mut de = Deserializer::from_str_with_config(INPUT, &LINES);
    let mut events = Vec::new();
    let mut errors = Vec::new();
    while !de.is_end() {
        match de.deserialize::<Event>() {
            Ok(event) => events.push(event),
            Err(err) => {
                println!("skipped: {}", err);
                errors.push(err.line());
            }
        }
    }
    println!("{:#?}", events);
    assert_eq!(events.len(), 3);
    assert_eq!(errors, [Some(4), Some(5)]);

    // the serializer never writes line breaks, a newline after every value
    // produces JSON Lines
    let mut output = String::new();
    for event in &events {
        output.push_str(&deser_json::to_string(event).unwrap());
        output.push('\n');
    }
    print!("{}", output);
    assert_eq!(
        output,
        r#"{"event":"login","user":"jane"}
{"event":"upload","user":"jane","bytes":1024}
{"event":"logout","user":"jane"}
"#
    );
}
