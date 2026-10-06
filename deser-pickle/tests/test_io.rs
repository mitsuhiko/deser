use deser_pickle::{DeserializerConfig, SerializerConfig, from_reader, to_writer};

#[test]
fn test_from_reader() {
    let value: Vec<String> = from_reader(&b"\x80\x04](\x8c\x01ae."[..]).unwrap();
    assert_eq!(value, ["a"]);
    // a single value
    assert!(from_reader::<u32, _>(&b"K\x01.K\x02."[..]).is_err());
    assert!(from_reader::<u32, _>(&b""[..]).is_err());
    assert!(from_reader::<u32, _>(&b"K\x01"[..]).is_err());
}

#[test]
fn test_reader() {
    let input = b"K\x01.\x80\x04\x95\x06\x00\x00\x00\x00\x00\x00\x00\x8c\x03two\x94.](X\x02\x00\x00\x00.\ne.";
    let mut reader = DeserializerConfig::new().reader(&input[..]);
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("two"));
    // a `STOP` in a string is not the end
    assert_eq!(
        reader.read::<Vec<String>>().unwrap(),
        Some(vec![".\n".into()])
    );
    assert_eq!(reader.read::<u32>().unwrap(), None);
}

#[test]
fn test_reader_invalid() {
    let mut reader = DeserializerConfig::new().reader(&b"K\x01.\xff."[..]);
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    assert!(reader.read::<u32>().is_err());
}

#[test]
fn test_writer() {
    let mut out = Vec::new();
    to_writer(&mut out, &vec![1, 2]).unwrap();
    assert_eq!(out, b"\x80\x04](K\x01K\x02e.");

    let mut writer = SerializerConfig::new().writer(Vec::new());
    writer.write(&1).unwrap();
    writer.write(&"x").unwrap();
    assert_eq!(writer.into_inner(), b"\x80\x04K\x01.\x80\x04\x8c\x01x.");
}

#[test]
fn test_stop_in_a_frame() {
    // a pickle that stops within its frame ends with the frame (like when
    // Python reads pickles from a file), the rest of the frame is skipped
    let mut input = b"\x95".to_vec();
    input.extend_from_slice(&6u64.to_le_bytes());
    input.extend_from_slice(b"K\x01.K\x02.K\x03.");

    let mut de = deser_pickle::Deserializer::from_slice(&input);
    let values = de.iter::<u32>().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(values, [1, 3]);

    for size in 1..=input.len() {
        let mut reader = DeserializerConfig::new().reader(Chunked {
            input: &input,
            size,
        });
        let values = reader.iter::<u32>().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(values, [1, 3], "size {size}");
    }

    // the frame has to be complete
    let mut de = deser_pickle::Deserializer::from_slice(&input[..12]);
    assert!(de.deserialize::<u32>().is_err());
    let mut reader = DeserializerConfig::new().reader(&input[..12]);
    assert!(reader.read::<u32>().is_err());
}

/// A reader that returns the input in chunks of a fixed size.
struct Chunked<'a> {
    input: &'a [u8],
    size: usize,
}

impl std::io::Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.size.min(buf.len()).min(self.input.len());
        buf[..len].copy_from_slice(&self.input[..len]);
        self.input = &self.input[len..];
        Ok(len)
    }
}
