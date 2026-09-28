use deser_plist::{DeserializerConfig, Format, SerializerConfig};

use crate::common::Value;

#[test]
fn test_reader_and_writer() {
    for format in [Format::Xml, Format::Binary, Format::Ascii] {
        let config = SerializerConfig::new().format(format);
        let mut out = Vec::new();
        config.to_writer(&mut out, &vec!["a", "b"]).unwrap();
        assert_eq!(out, config.to_vec(&vec!["a", "b"]).unwrap());

        let value: Vec<String> = deser_plist::from_reader(&out[..]).unwrap();
        assert_eq!(value, ["a", "b"]);

        // a stream holds a single property list
        let mut writer = config.clone().writer(Vec::new());
        writer.write(&1u32).unwrap();
        assert!(writer.write(&2u32).is_err());
        let bytes = writer.into_inner();
        let mut reader = DeserializerConfig::new().reader(&bytes[..]);
        assert_eq!(reader.read::<u32>().unwrap(), Some(1));
        assert_eq!(reader.read::<u32>().unwrap(), None);
    }
}

#[test]
fn test_reader_errors() {
    let err = deser_plist::from_reader::<Value, _>(&b"<plist>\n<foo/>"[..]).unwrap_err();
    assert_eq!((err.line(), err.column()), (Some(2), Some(1)));
}

#[test]
fn test_partial_writer() {
    use std::collections::BTreeMap;

    #[derive(deser::Serialize)]
    struct Entry {
        name: String,
        tags: Vec<String>,
        empty: Vec<u32>,
        nothing: Option<u32>,
        data: Value,
        nested: BTreeMap<String, Vec<BTreeMap<String, bool>>>,
    }

    let entries: Vec<Entry> = (0..30)
        .map(|idx| Entry {
            name: format!("entry <{idx}> & more"),
            tags: (0..idx % 4).map(|x| format!("tag {x}")).collect(),
            empty: vec![],
            nothing: None,
            data: Value::Bytes((0..idx * 20).map(|x| x as u8).collect()),
            nested: BTreeMap::from([
                (
                    "a".into(),
                    vec![BTreeMap::new(), BTreeMap::from([("x".into(), true)])],
                ),
                ("b".into(), vec![]),
            ]),
        })
        .collect();
    let values: [&dyn deser::Serialize; 3] = [&entries, &"scalar", &Vec::<u32>::new()];
    for format in [Format::Xml, Format::Ascii, Format::Binary] {
        let config = SerializerConfig::new().format(format);
        for value in values {
            let expected = config.to_vec(value).unwrap();
            for limit in [1, 13, 500, usize::MAX] {
                let mut writer = config.writer(Vec::new());
                writer.set_buffer_limit(limit);
                writer.write(value).unwrap();
                // a stream holds a single value
                assert!(writer.write(value).is_err());
                assert_eq!(writer.into_inner(), expected, "{format:?} {limit}");
            }
        }
    }

    // errors are the same
    let config = SerializerConfig::new();
    let nulls = vec![Some(1), None];
    let err = config.to_vec(&nulls).unwrap_err();
    let mut writer = config.writer(Vec::new());
    writer.set_buffer_limit(1);
    assert_eq!(writer.write(&nulls).unwrap_err().message(), err.message());
}
