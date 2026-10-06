//! The context of the configuration.
use std::collections::BTreeMap;

use deser::Context;
use deser::de::{DuplicateKeys, Limits};

use super::dialect::{self, Deserializer, DeserializerConfig};

type Map = BTreeMap<String, u32>;

const INPUT: &str = r#"{"a": 1, "a": 2}"#;

fn last() -> DeserializerConfig {
    DeserializerConfig::builder()
        .context(Context::with(DuplicateKeys::Last))
        .build()
}

#[test]
fn test_config_context() {
    // repeated keys are errors by default
    assert!(dialect::from_str::<Map>(INPUT).is_err());

    let config = last();
    assert_eq!(config.from_str::<Map>(INPUT).unwrap()["a"], 2);
    assert_eq!(config.from_slice::<Map>(INPUT.as_bytes()).unwrap()["a"], 2);

    // the deserializers created with the configuration have its context
    let mut de = Deserializer::from_str_with_config(INPUT, config.clone());
    assert_eq!(de.config().context(), config.context());
    assert_eq!(de.deserialize::<Map>().unwrap()["a"], 2);

    // a context on the driver takes precedence
    let mut de = Deserializer::from_str_with_config(INPUT, config);
    let err = de
        .deserialize_with::<Map, _>(|driver| {
            driver.set_context(Context::with(DuplicateKeys::Error));
        })
        .unwrap_err();
    assert!(err.message().contains("duplicate"), "{}", err);
}

#[test]
fn test_driver_context_extends_config() {
    // the values of the configuration's context that the context of the
    // driver does not have are added to it
    let limits = Context::with(Limits::builder().max_items(2).build());
    let mut de = Deserializer::from_str_with_config(INPUT, last());
    let map = de
        .deserialize_with::<Map, _>(|driver| driver.set_context(limits.clone()))
        .unwrap();
    assert_eq!(map["a"], 2);

    let mut de = Deserializer::from_str_with_config(r#"{"a": 1, "a": 2, "b": 3}"#, last());
    let err = de
        .deserialize_with::<Map, _>(|driver| driver.set_context(limits.clone()))
        .unwrap_err();
    assert_eq!(err.kind(), deser::ErrorKind::LimitExceeded);
}

#[test]
fn test_config_setter() {
    let mut config = DeserializerConfig::new();
    assert!(config.context().is_empty());
    config.set_context(Context::with(DuplicateKeys::Last));
    assert_eq!(config.from_str::<Map>(INPUT).unwrap()["a"], 2);

    // configurations are equal if they share the context
    assert_eq!(config, config.clone());
    assert_ne!(config, last());
    assert_eq!(DeserializerConfig::new(), DeserializerConfig::default());
}

#[test]
#[cfg(feature = "io")]
fn test_reader_context() {
    let config = last();
    let map: Map = config.from_reader(INPUT.as_bytes()).unwrap();
    assert_eq!(map["a"], 2);

    let mut reader = config.reader(INPUT.as_bytes());
    assert_eq!(reader.context(), config.context());
    assert_eq!(reader.read::<Map>().unwrap().unwrap()["a"], 2);

    let mut reader = config.reader(INPUT.as_bytes());
    reader.set_context(Context::new());
    assert!(reader.read::<Map>().is_err());
    // also for values read from their frames
    let mut reader = config.reader(INPUT.as_bytes());
    reader.set_context(Context::new());
    assert!(reader.read_borrowed::<Map>().is_err());
}

#[test]
fn test_drive_context() {
    // the deserializer gives the context of the configuration to drivers
    let mut de = Deserializer::from_str_with_config(INPUT, last());
    let mut out = None::<Map>;
    {
        let mut driver = deser::de::DeserializeDriver::new(&mut out);
        de.drive(&mut driver).unwrap();
    }
    assert_eq!(out.unwrap()["a"], 2);
}
