use deser::io::{Reader, Writer};
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
        let mut writer = Writer::new(Vec::new(), config.clone());
        writer.write(&1u32).unwrap();
        assert!(writer.write(&2u32).is_err());
        let bytes = writer.into_inner();
        let mut reader = Reader::new(&bytes[..], DeserializerConfig::new());
        assert_eq!(reader.read::<u32>().unwrap(), Some(1));
        assert_eq!(reader.read::<u32>().unwrap(), None);
    }
}

#[test]
fn test_reader_errors() {
    let err = deser_plist::from_reader::<Value, _>(&b"<plist>\n<foo/>"[..]).unwrap_err();
    assert_eq!((err.line(), err.column()), (Some(2), Some(1)));
}
