//! Reading and writing values from and to `std::io` streams.
//!
//! Single values are read with the `from_reader` and written with the
//! `to_writer` functions of the formats.  Streams of values (JSON Lines,
//! CBOR sequences, YAML documents) are read with a `deser::io::Reader` and
//! written with a `deser::io::Writer` using the configurations of a
//! format.  Only one value is buffered at a time.
use std::fs::File;
use std::io::{BufWriter, Write};

use deser::io::{Reader, Writer};
use deser::{Deserialize, Serialize};
use deser_json::{DeserializerConfig, SerializerConfig, Trailing};

const READ_LINES: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);
const WRITE_LINES: SerializerConfig = SerializerConfig::new().trailing(Trailing::Newline);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    name: String,
    workers: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Login { user: String },
    Upload { user: String, bytes: u64 },
    Logout { user: String },
}

fn main() -> Result<(), deser::Error> {
    let dir = std::env::temp_dir();

    // a single value: a config file
    let path = dir.join("deser-streams-config.toml");
    deser_toml::to_writer(
        File::create(&path)?,
        &Config {
            name: "uploads".into(),
            workers: 4,
        },
    )?;
    let config: Config = deser_toml::from_reader(File::open(&path)?)?;
    println!("config: {:?}", config);

    // a stream of values: a JSON Lines log (with a broken line)
    let path = dir.join("deser-streams-events.jsonl");
    {
        let file = BufWriter::new(File::create(&path)?);
        let mut log = Writer::new(file, WRITE_LINES);
        log.write(&Event::Login {
            user: "jane".into(),
        })?;
        log.write(&Event::Upload {
            user: "jane".into(),
            bytes: 1024,
        })?;
        log.get_mut()
            .write_all(b"{\"event\": \"upload\", \"user\": \"jane\"\n")?;
        log.write(&Event::Logout {
            user: "jane".into(),
        })?;
        log.flush()?;
    }

    // read it back line by line, errors only discard their line.  The
    // events are converted into a CBOR sequence on the way.
    let mut events = Reader::new(File::open(&path)?, READ_LINES);
    let mut cbor = Writer::new(Vec::new(), deser_cbor::SerializerConfig::new());
    while let Some(event) = events.read::<Event>().transpose() {
        match event {
            Ok(event) => cbor.write(&event)?,
            Err(err) => println!("skipped: {}", err),
        }
    }
    let cbor = cbor.into_inner();
    println!("{} bytes of CBOR", cbor.len());

    // and the CBOR sequence as YAML documents
    let mut yaml = Writer::new(Vec::new(), deser_yaml::SerializerConfig::new());
    for event in Reader::new(&cbor[..], deser_cbor::DeserializerConfig::new()).iter::<Event>() {
        yaml.write(&event?)?;
    }
    let yaml = String::from_utf8(yaml.into_inner()).unwrap();
    print!("{}", yaml);

    let mut documents = Reader::new(yaml.as_bytes(), deser_yaml::DeserializerConfig::new());
    let events = documents.iter::<Event>().collect::<Result<Vec<_>, _>>()?;
    assert_eq!(events.len(), 3);
    Ok(())
}
