//! Raw values (see `deser::ext::Raw`).
//!
//! The statuses of the Twitter dump and the log events are held as raw
//! values of the format they are read from.  This measures passing on the
//! input of values (the requests of the sinks and validating the values)
//! and writing it out again as it is.  Only JSON, CBOR and MessagePack have
//! raw values.
use std::hint::black_box;

use deser::{Deserialize, Serialize};
use deser_cbor::RawCbor;
use deser_json::RawJson;
use deser_msgpack::RawMsgpack;

use crate::formats::{self, Format, Input};
use crate::logs::LogEvent;
use crate::twitter::Twitter;
use crate::{Benches, Dataset, Documents};

macro_rules! statuses {
    ($name:ident, $raw:ty) => {
        /// The Twitter dump with raw statuses.
        #[derive(Serialize, Deserialize)]
        pub struct $name {
            statuses: Vec<$raw>,
            search_metadata: $raw,
        }

        impl $name {
            fn first(&self) -> &[u8] {
                self.statuses[0].as_bytes()
            }
        }
    };
}

statuses!(JsonStatuses, RawJson<'static>);
statuses!(CborStatuses, RawCbor<'static>);
statuses!(MsgpackStatuses, RawMsgpack<'static>);

macro_rules! add_format {
    ($benches:expr, $twitter:expr, $logs:expr, $format:expr, $statuses:ty, $raw:ty) => {{
        let format = $format;
        let name = format!("raw/{}", format.name());
        let twitter = $twitter.input(format);
        let statuses: $statuses = formats::deser_de(format, twitter).unwrap();
        // the input is passed on, not encoded again
        assert!(contains(bytes(twitter), statuses.first()), "{}", name);
        let docs = &$logs.inputs.iter().find(|(f, _)| *f == format).unwrap().1;
        let values = docs
            .iter()
            .map(|doc| {
                let raw: $raw = formats::deser_de(format, Input::new(format, doc)).unwrap();
                assert_eq!(raw.as_bytes(), &doc[..], "{}", name);
                raw
            })
            .collect::<Vec<_>>();
        $benches.add(format!("{}/de-items", name), move || {
            black_box(formats::deser_de::<$statuses>(format, twitter).unwrap());
        });
        $benches.add(format!("{}/ser-items", name), move || {
            black_box(formats::deser_ser(format, &statuses).unwrap());
        });
        $benches.add(format!("{}/de-top", name), move || {
            for doc in docs {
                black_box(formats::deser_de::<$raw>(format, Input::new(format, doc)).unwrap());
            }
        });
        $benches.add(format!("{}/ser-top", name), move || {
            for value in &values {
                black_box(formats::deser_ser(format, value).unwrap());
            }
        });
    }};
}

/// Adds the benchmarks of raw values.
pub fn add_benches<'a>(
    benches: &mut Benches<'a>,
    twitter: &'a Dataset<Twitter>,
    logs: &'a Documents<LogEvent>,
) {
    add_format!(
        benches,
        twitter,
        logs,
        Format::Json,
        JsonStatuses,
        RawJson<'static>
    );
    add_format!(
        benches,
        twitter,
        logs,
        Format::Cbor,
        CborStatuses,
        RawCbor<'static>
    );
    add_format!(
        benches,
        twitter,
        logs,
        Format::Msgpack,
        MsgpackStatuses,
        RawMsgpack<'static>
    );
}

fn bytes(input: Input<'_>) -> &[u8] {
    match input {
        Input::Text(text) => text.as_bytes(),
        Input::Binary(bytes) => bytes,
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
