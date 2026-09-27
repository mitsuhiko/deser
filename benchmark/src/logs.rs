//! Structured log events as a service would write them (one JSON object
//! per line), generated.
//!
//! Every event is a document of its own of a few hundred bytes, the
//! benchmarks deserialize them one by one.  This measures what it costs to
//! deserialize a document (which the large documents of the other datasets
//! hide) and covers an internally tagged enum, an untagged enum in a map
//! and optional values.
use std::collections::BTreeMap;

use deser::{Deserialize, Serialize};

/// The number of events.
const EVENTS: usize = 5_000;

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
pub struct LogEvent {
    timestamp: String,
    level: Level,
    target: String,
    message: String,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    span: Option<Span>,
    #[deser(default, skip_serializing_if = BTreeMap::is_empty)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    fields: BTreeMap<String, FieldValue>,
    event: Event,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Span {
    name: String,
    id: u64,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<u64>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(untagged)]
#[serde(untagged)]
enum FieldValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(tag = "type", rename_all = "snake_case")]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    Request {
        method: Method,
        path: String,
        status: u16,
        duration_ms: f64,
        #[deser(default, skip_serializing_if = Option::is_none)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_agent: Option<String>,
    },
    Query {
        statement: String,
        rows: u64,
        duration_ms: f64,
    },
    Cache {
        key: String,
        hit: bool,
    },
    Error {
        kind: String,
        message: String,
        backtrace: Vec<String>,
    },
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
enum Method {
    Get,
    Post,
    Put,
    Delete,
}

/// A small deterministic pseudo random number generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// Returns a float with a fractional part (so that it stays a float in
    /// the untagged field values).
    fn millis(&mut self) -> f64 {
        self.below(500_000) as f64 / 1000.0 + 0.0005
    }
}

pub fn events() -> Vec<LogEvent> {
    const PATHS: [&str; 6] = [
        "/api/v1/users",
        "/api/v1/users/42/settings",
        "/api/v1/projects",
        "/api/v1/projects/7/issues",
        "/healthz",
        "/api/v1/search",
    ];
    const TARGETS: [&str; 4] = [
        "server::http",
        "server::db",
        "server::cache",
        "server::auth",
    ];
    let mut rng = Rng(0x5eed_1e55);
    (0..EVENTS)
        .map(|index| {
            let event = match rng.below(10) {
                0..=4 => Event::Request {
                    method: match rng.below(4) {
                        0 => Method::Get,
                        1 => Method::Post,
                        2 => Method::Put,
                        _ => Method::Delete,
                    },
                    path: PATHS[rng.below(PATHS.len() as u64) as usize].to_string(),
                    status: [200, 200, 200, 201, 204, 404, 500][rng.below(7) as usize],
                    duration_ms: rng.millis(),
                    user_agent: (rng.below(3) != 0)
                        .then(|| "Mozilla/5.0 (X11; Linux x86_64) Firefox/128.0".to_string()),
                },
                5..=7 => Event::Query {
                    statement: "SELECT id, name, email FROM users WHERE project_id = $1".into(),
                    rows: rng.below(1000),
                    duration_ms: rng.millis(),
                },
                8 => Event::Cache {
                    key: format!("session:{:x}", rng.next()),
                    hit: rng.below(2) == 0,
                },
                _ => Event::Error {
                    kind: "ConnectionReset".into(),
                    message: "connection reset by peer".into(),
                    backtrace: vec![
                        "server::db::pool::Pool::get".into(),
                        "server::http::handler".into(),
                    ],
                },
            };
            let mut fields = BTreeMap::new();
            fields.insert(
                "request_id".to_string(),
                FieldValue::Str(format!("{:016x}", rng.next())),
            );
            if rng.below(2) == 0 {
                fields.insert(
                    "user_id".to_string(),
                    FieldValue::Int(rng.below(100_000) as i64),
                );
            }
            if rng.below(3) == 0 {
                fields.insert("retry".to_string(), FieldValue::Bool(rng.below(2) == 0));
                fields.insert("load".to_string(), FieldValue::Float(rng.millis()));
            }
            let seconds = index as u64 / 10;
            LogEvent {
                timestamp: format!(
                    "2024-05-01T{:02}:{:02}:{:02}.{:06}Z",
                    seconds / 3600 % 24,
                    seconds / 60 % 60,
                    seconds % 60,
                    rng.below(1_000_000)
                ),
                level: match rng.below(20) {
                    0 => Level::Error,
                    1..=2 => Level::Warn,
                    3..=5 => Level::Debug,
                    6 => Level::Trace,
                    _ => Level::Info,
                },
                target: TARGETS[rng.below(TARGETS.len() as u64) as usize].to_string(),
                message: "request finished".into(),
                span: (rng.below(2) == 0).then(|| Span {
                    name: "request".into(),
                    id: rng.below(1 << 40),
                    parent: (rng.below(2) == 0).then(|| rng.below(1 << 40)),
                }),
                fields,
                event,
            }
        })
        .collect()
}
