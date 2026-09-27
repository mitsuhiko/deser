//! Writing JSON streams (reading is tested by deser-private-jsontemplate).
use deser::Event;
use deser::io::{Reader, Writer};
use deser_json::{DeserializerConfig, SerializerConfig, Trailing};

const STOP: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Stop);

#[test]
fn test_writer() {
    let mut writer = Writer::new(Vec::new(), SerializerConfig::new().trailing(Trailing::Stop));
    writer.write(&1).unwrap();
    writer.write(&vec![2, 3]).unwrap();
    let out = writer.into_inner();
    assert_eq!(out, b"1\n[2,3]");

    // the output can be read again
    let mut reader = Reader::new(&out[..], STOP);
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![2, 3]));

    let mut writer = Writer::new(
        Vec::new(),
        SerializerConfig::new().trailing(Trailing::Newline),
    );
    writer.write(&"a").unwrap();
    writer.write(&"b").unwrap();
    assert_eq!(writer.into_inner(), b"\"a\"\n\"b\"\n");

    let mut out = Vec::new();
    deser_json::to_writer(&mut out, &vec!["x"]).unwrap();
    assert_eq!(out, b"[\"x\"]");
}

#[test]
fn test_writer_strict_and_layers() {
    use deser::ser::{Layer, Next};
    use deser::{Atom, Error};

    // a strict stream holds a single value
    let mut writer = Writer::new(Vec::new(), SerializerConfig::new());
    writer.write(&1).unwrap();
    assert!(writer.write(&2).is_err());
    assert_eq!(writer.into_inner(), b"1");

    /// Writes all numbers as strings.
    struct NumbersAsStrings;

    impl Layer for NumbersAsStrings {
        fn event(&mut self, event: Event<'_>, next: &mut Next<'_>) -> Result<(), Error> {
            match event {
                Event::Atom(Atom::U64(value)) => next.emit(value.to_string().into()),
                event => next.emit(event),
            }
        }
    }

    let mut writer = Writer::new(
        Vec::new(),
        SerializerConfig::new().trailing(Trailing::Newline),
    );
    writer
        .write_with(&vec![1u64, 2], |driver| driver.push_layer(NumbersAsStrings))
        .unwrap();
    assert_eq!(writer.into_inner(), b"[\"1\",\"2\"]\n");
}
