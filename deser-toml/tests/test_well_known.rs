use std::collections::BTreeMap;
use std::time::{Duration, UNIX_EPOCH};

use deser::ext::{Decimal, Timestamp, Uuid};
use deser::{Deserialize, Serialize};
use deser_toml::{from_str, to_string};

#[derive(Deserialize, Serialize, Debug, PartialEq)]
struct Event {
    start: jiff::Timestamp,
    zoned: jiff::Zoned,
    local: jiff::civil::DateTime,
    day: jiff::civil::Date,
    at: jiff::civil::Time,
    reminder: Option<jiff::civil::Time>,
    length: jiff::SignedDuration,
    days: Vec<jiff::civil::Date>,
}

#[test]
fn test_jiff() {
    let event: Event = from_str(
        "
start = 1979-05-27 00:32:00.999999-07:00
zoned = 1979-05-27T00:32:00+05:30
local = 1979-05-27T07:32
day = 1979-05-27
at = 07:32:00.5
length = 'PT1H30M'
days = [2024-02-29, 0001-01-01]
",
    )
    .unwrap();
    assert_eq!(event.start.to_string(), "1979-05-27T07:32:00.999999Z");
    assert_eq!(event.zoned.to_string(), "1979-05-27T00:32:00+05:30[+05:30]");
    assert_eq!(event.day, jiff::civil::date(1979, 5, 27));
    assert_eq!(event.reminder, None);
    assert_eq!(event.length, jiff::SignedDuration::from_secs(5400));

    let toml = to_string(&event).unwrap();
    assert_eq!(
        toml,
        "start = 1979-05-27T07:32:00.999999Z
zoned = 1979-05-27T00:32:00+05:30
local = 1979-05-27T07:32:00
day = 1979-05-27
at = 07:32:00.5
length = \"PT1H30M\"
days = [2024-02-29, 0001-01-01]
"
    );
    assert_eq!(from_str::<Event>(&toml).unwrap(), event);

    // the kinds of date-times are checked
    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    struct Stamp {
        value: jiff::Timestamp,
    }
    let err = from_str::<Stamp>("value = 1979-05-27T07:32:00").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Unexpected: unexpected local date-time, expected offset date-time"
    );
}

#[test]
fn test_other_well_known_types() {
    #[derive(Deserialize, Serialize, Debug, PartialEq)]
    struct Doc {
        id: Uuid,
        price: Decimal,
        created: std::time::SystemTime,
        timeout: Duration,
        stamp: Timestamp,
    }
    let doc = Doc {
        id: "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap(),
        price: "12.50".parse().unwrap(),
        created: UNIX_EPOCH + Duration::from_secs(296638320),
        timeout: Duration::from_millis(1500),
        stamp: Timestamp {
            seconds: 253402300800,
            nanosecond: 0,
        },
    };
    let toml = to_string(&doc).unwrap();
    assert_eq!(
        toml,
        r#"id = "67e55044-10b1-426f-9247-bb680e5fe0c8"
price = "12.50"
created = 1979-05-27T07:32:00Z
timeout = "PT1.5S"
stamp = "+010000-01-01T00:00:00Z"
"#
    );
    assert_eq!(from_str::<Doc>(&toml).unwrap(), doc);

    // numbers are accepted for decimals
    let value: BTreeMap<String, Decimal> = from_str("a = 1.5\nb = 42").unwrap();
    assert_eq!(value["a"].as_str(), "1.5");
    assert_eq!(value["b"].as_str(), "42");
}
