use std::collections::BTreeMap;

use deser_ini::{DeserializerConfig, SerializerConfig};

#[test]
fn test_reader_and_writer() {
    let value: BTreeMap<String, BTreeMap<String, u32>> =
        deser_ini::from_reader(&b"[a]\nb = 1\n"[..]).unwrap();
    assert_eq!(value["a"]["b"], 1);

    let mut reader = DeserializerConfig::git().reader(&b"[A]\n\tB = 2\n"[..]);
    let value: BTreeMap<String, BTreeMap<String, u32>> = reader.read().unwrap().unwrap();
    assert_eq!(value["a"]["b"], 2);
    assert!(reader.read::<BTreeMap<String, u32>>().unwrap().is_none());

    let mut out = Vec::new();
    deser_ini::to_writer(&mut out, &value).unwrap();
    assert_eq!(out, b"[a]\nb = 2\n");

    let mut out = Vec::new();
    let mut writer = SerializerConfig::git().writer(&mut out);
    writer.write(&value).unwrap();
    assert!(writer.write(&value).is_err());
    drop(writer);
    assert_eq!(out, b"[a]\n\tb = 2\n");
}
