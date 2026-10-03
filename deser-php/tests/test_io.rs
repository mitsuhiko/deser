use deser_php::{DeserializerConfig, SerializerConfig, from_reader, to_writer};

#[test]
fn test_from_reader() {
    let value: Vec<String> = from_reader(&b"a:1:{i:0;s:1:\"a\";}"[..]).unwrap();
    assert_eq!(value, ["a"]);
    // a single value
    assert!(from_reader::<u32, _>(&b"i:1;i:2;"[..]).is_err());
    assert!(from_reader::<u32, _>(&b""[..]).is_err());
    assert!(from_reader::<u32, _>(&b"i:1"[..]).is_err());
}

#[test]
fn test_reader() {
    let mut reader = DeserializerConfig::new().reader(&b"i:1;s:3:\"two\";a:0:{}"[..]);
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("two"));
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![]));
    assert_eq!(reader.read::<u32>().unwrap(), None);
}

#[test]
fn test_writer() {
    let mut out = Vec::new();
    to_writer(&mut out, &vec![1, 2]).unwrap();
    assert_eq!(out, b"a:2:{i:0;i:1;i:1;i:2;}");

    let mut writer = SerializerConfig::new().writer(Vec::new());
    writer.write(&1).unwrap();
    writer.write(&"x").unwrap();
    assert_eq!(writer.into_inner(), b"i:1;s:1:\"x\";");
}
