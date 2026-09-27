use std::io::Read;

use deser::Event;
use deser::de::Recording;
use deser::io::Reader;
use deser_json5::{Deserializer, DeserializerConfig, Trailing};

const STRICT: DeserializerConfig = DeserializerConfig::new();
const NEWLINE: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);
const STOP: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Stop);

/// A reader that returns the input in chunks of a fixed size.
struct Chunked<'a> {
    input: &'a [u8],
    size: usize,
}

impl Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.size.min(buf.len()).min(self.input.len());
        buf[..len].copy_from_slice(&self.input[..len]);
        self.input = &self.input[len..];
        Ok(len)
    }
}

fn events(value: Recording) -> Vec<Event<'static>> {
    value.events().cloned().collect()
}

fn read_in_memory(config: &DeserializerConfig, input: &str) -> Vec<Vec<Event<'static>>> {
    let mut de = Deserializer::from_str_with_config(input, config);
    let mut rv = Vec::new();
    while !de.is_end() {
        rv.push(events(de.deserialize::<Recording>().unwrap()));
    }
    rv
}

/// Reads the values while their input arrives.
fn read_fed(config: &DeserializerConfig, input: &str, size: usize) -> Vec<Vec<Event<'static>>> {
    let mut reader = Reader::new(
        Chunked {
            input: input.as_bytes(),
            size,
        },
        config,
    );
    let mut rv = Vec::new();
    while let Some(value) = reader.read::<Recording>().unwrap() {
        rv.push(events(value));
    }
    rv
}

/// Reads the values from their frames.
fn read_framed(config: &DeserializerConfig, input: &str, size: usize) -> Vec<Vec<Event<'static>>> {
    let mut reader = Reader::new(
        Chunked {
            input: input.as_bytes(),
            size,
        },
        config,
    );
    let mut rv = Vec::new();
    while let Some(value) = reader.read_borrowed::<Recording>().unwrap() {
        rv.push(events(value));
    }
    rv
}

fn check(config: &DeserializerConfig, input: &str, count: usize) {
    let expected = read_in_memory(config, input);
    assert_eq!(expected.len(), count, "{input:?}");
    for size in 1..=input.len() {
        assert_eq!(
            read_fed(config, input, size),
            expected,
            "fed {input:?} size {size}"
        );
        assert_eq!(
            read_framed(config, input, size),
            expected,
            "framed {input:?} size {size}"
        );
    }
}

#[test]
fn test_stop() {
    check(
        &STOP,
        r#"// values
        1 /* ] */ [1, /* ] */ 2,] {"a": "/*", /* } */ "b": [],} // "
        "x"/**/3 // end"#,
        5,
    );
}

#[test]
fn test_strict() {
    check(&STRICT, "/* a */ [1, // b\n 2,] // c", 1);
    check(&STRICT, "// only a comment", 0);
}

#[test]
fn test_newline() {
    check(
        &NEWLINE,
        "// header\n[1, 2,] // a\n\n{\"a\": 1} /* b */\n",
        2,
    );
}

#[test]
fn test_unterminated_comment() {
    for config in [&STRICT, &STOP] {
        for size in 1..=8 {
            let mut reader = Reader::new(
                Chunked {
                    input: b"[1] /* x",
                    size,
                },
                config,
            );
            let rv = (|| {
                while reader.read::<Recording>()?.is_some() {}
                Ok::<_, deser::Error>(())
            })();
            assert!(rv.is_err(), "size {size}");
        }
    }
}

#[test]
fn test_json5_stop() {
    check(
        &STOP,
        "{ünï: 'a]\"}', b: [.5, +1, 0xFF, Infinity,],} 'x\\'y' -Infinity\u{a0}'}' {c: \"'\"}",
        5,
    );
}

#[test]
fn test_json5_strict() {
    check(
        &STRICT,
        "\u{feff}// c\n{key: 'val\\\nue', n: -.5e1,}\u{2028}",
        1,
    );
}
