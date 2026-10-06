use deser::de::Recording;
use deser::{ContainerShape, ErrorKind, Event};

use super::common::{
    Blocking, Chunked, NEWLINE, STOP, STRICT, check_stream, chunk_sizes, events, read_chunked,
};
use super::{DIALECT, dialect};

const VALUES: &str = if DIALECT.hjson {
    // numbers and literals end at the end of the line
    " 1\n-2.5e3\n\"a\\\"b\\\\\" true\nnull\n[] {} [1, [2, [3]]]
{\"a\": {\"b\": \"}]\"}, \"c\": [false]} \"\\u00e4ä\" 42"
} else {
    r#" 1 -2.5e3 "a\"b\\" true null [] {} [1, [2, [3]]]
{"a": {"b": "}]"}, "c": [false]} "\u00e4ä" 42"#
};

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_stop_in_chunks() {
    check_stream(&STOP, VALUES, 11);
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_numbers_in_chunks() {
    // numbers whose start is parsed by the parser continue across chunks
    check_stream(
        &STOP,
        "1\n-2\n12.5\n123456789\n-1234567890\n3e2\n0\n-0.5\n12345678.875e-3\n7",
        10,
    );
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_values_without_whitespace() {
    check_stream(&STOP, r#"[1]{"a":2}"x"3"#, 4);
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_scalars_without_whitespace() {
    // scalars end where the parser stops, not at the next whitespace
    if DIALECT.hjson {
        // quoteless strings and numbers end at the end of the line
        check_stream(
            &STOP, "1-2
3", 2,
        );
    } else if DIALECT.json5 {
        check_stream(&STOP, "1-2 3+4 .5.5 true-1null", 9);
    } else {
        check_stream(&STOP, "1-2 3-4e1 truefalse-0null", 8);
    }
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_multiline_strings_in_chunks() {
    if !DIALECT.hjson {
        return;
    }
    // the indentation is relative to the column of the quotes, also if
    // the frame of the value starts at them
    check_stream(&STOP, "'''\n  a\n  '''\n   '''\n   b\n    c\n   '''", 2);
    check_stream(&STRICT, "\n  '''\n  a\n   b\n  '''\n", 1);
    let values = read_chunked(&STOP, "  '''\n  a\n   b\n  '''", 1);
    assert_eq!(values, [vec![Event::from("a\n b")]]);
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_newline_in_chunks() {
    check_stream(&NEWLINE, "[1, 2]\n\n  {\"a\": \"b\"}  \r\n\"x\"\n   \n3", 4);
}

#[test]
#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]
fn test_strict_in_chunks() {
    check_stream(&STRICT, " [1, {\"a\": [true]}] \n", 1);
    assert_eq!(read_chunked(&STRICT, "  \n", 1), Vec::<Vec<Event>>::new());
}

#[test]
fn test_no_read_while_a_value_is_complete() {
    // everything after the complete values is only read when needed
    let mut reader = NEWLINE.reader(Blocking(b"1\n\n[2]\n"));
    assert_eq!(reader.read::<u32>().unwrap(), Some(1));
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![2]));

    let mut reader = STOP.reader(Blocking(b"[1] \"x\" {} 2"));
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1]));
    assert_eq!(reader.read::<String>().unwrap().as_deref(), Some("x"));
    assert_eq!(
        reader.read::<Recording>().unwrap().map(events),
        Some(vec![
            Event::MapStart(ContainerShape::with_len(0)),
            Event::MapEnd
        ])
    );
}

#[test]
fn test_from_reader() {
    let value: Vec<u32> = dialect::from_reader(Chunked {
        input: b" [1, 2,\n 3] ",
        size: 2,
    })
    .unwrap();
    assert_eq!(value, [1, 2, 3]);

    let err = dialect::from_reader::<Vec<u32>, _>(&b"[1, 2]\n [3]"[..]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "Syntax: garbage after input at line 2 column 2"
    );
    let err = dialect::from_reader::<Vec<u32>, _>(&b"  "[..]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);

    // `Trailing::Stop` reads the first value, but it must be the only one
    let value: u32 = STOP.from_reader(&b" 1 "[..]).unwrap();
    assert_eq!(value, 1);
    // in Hjson numbers end at the end of the line
    let (input, column): (&[u8], _) = if DIALECT.hjson {
        (b"1\n2", "line 2 column 1")
    } else {
        (b"1 2", "line 1 column 3")
    };
    let err = STOP.from_reader::<u32, _>(input).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("Syntax: unexpected value after the end at {column}")
    );
}

#[test]
fn test_borrowed() {
    let mut reader = NEWLINE.reader(&b"{\"name\": \"Peter\"}\n"[..]);
    let value: std::collections::BTreeMap<&str, &str> = reader.read_borrowed().unwrap().unwrap();
    assert_eq!(value["name"], "Peter");
}

#[test]
fn test_errors() {
    // lines continue after errors, positions refer to the stream
    for size in [1, 3, 100] {
        let input = b"[1]\n[\"x\"]\n  [2, x]\n[3]\n";
        let mut reader = NEWLINE.reader(Chunked { input, size });
        let mut results = Vec::new();
        while let Some(result) = reader.read::<Vec<u32>>().transpose() {
            results.push(result.map_err(|err| err.to_string()));
        }
        assert_eq!(
            results,
            [
                Ok(vec![1]),
                Err("InvalidType: unexpected string, expected u32 at line 2 column 2".into()),
                Err(if DIALECT.hjson {
                    // `x]` is a string without quotes
                    "InvalidType: unexpected string, expected u32 at line 3 column 7".into()
                } else {
                    "Syntax: unexpected character at line 3 column 7".into()
                }),
                Ok(vec![3]),
            ]
        );
    }

    // with `Trailing::Stop` values that do not match the type are skipped
    // (also while they are fed)
    for size in [1, 3, 100] {
        let input = b"[1] [\"x\", [{}]] {\"a\": 1}\n[3]";
        let mut reader = STOP.reader(Chunked { input, size });
        assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1]));
        let err = reader.read::<Vec<u32>>().unwrap_err();
        assert_eq!(
            err.to_string(),
            "InvalidType: unexpected string, expected u32 at line 1 column 6"
        );
        let err = reader.read::<Vec<u32>>().unwrap_err();
        assert_eq!(err.offset(), Some(16));
        assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![3]));
        assert_eq!(reader.read::<Vec<u32>>().unwrap(), None);
    }

    // values that are not valid end the stream when they are fed
    let mut reader = STOP.reader(&b"[1] {] [3]"[..]);
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1]));
    let err = reader.read::<Vec<u32>>().unwrap_err();
    assert_eq!(err.offset(), Some(4));
    let err = reader.read::<Vec<u32>>().unwrap_err();
    assert_eq!(
        err.to_string(),
        "Syntax: expected map key at line 1 column 6"
    );
    let err = reader.read::<Vec<u32>>().unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidState: cannot continue after an error"
    );

    // values which are read from their frames (borrowing) are skipped
    let mut reader = STOP.reader(&b"[1] {] [3]"[..]);
    assert_eq!(reader.read_borrowed::<Vec<u32>>().unwrap(), Some(vec![1]));
    assert!(reader.read_borrowed::<Vec<u32>>().is_err());
    assert_eq!(reader.read_borrowed::<Vec<u32>>().unwrap(), Some(vec![3]));

    // incomplete values at the end
    let mut reader = STOP.reader(&b"[1] [2"[..]);
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), Some(vec![1]));
    let err = reader.read::<Vec<u32>>().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
    assert_eq!(reader.read::<Vec<u32>>().unwrap(), None);
}

#[test]
fn test_generic_formats() {
    use deser::de::{DeserializeOwned, Deserializer as _};
    use deser::ser::{Serialize, StreamSerializer};

    /// Roundtrips a value through any format.
    fn roundtrip<T, S>(ser: &mut S, de: impl FnOnce(&[u8]) -> T, value: &T) -> T
    where
        T: Serialize + DeserializeOwned,
        S: StreamSerializer,
    {
        ser.serialize(value).unwrap();
        de(ser.output())
    }

    let value = vec![(1u32, "a".to_string())];
    let mut ser = dialect::Serializer::new();
    let rv = roundtrip(&mut ser, |bytes| STRICT.from_slice(bytes).unwrap(), &value);
    assert_eq!(rv, value);

    // readers are deserializers
    // in Hjson numbers end at the end of the line
    let input: &[u8] = if DIALECT.hjson { b"1\n2" } else { b"1 2" };
    let mut reader = STOP.reader(input);
    let mut values = Vec::<u32>::new();
    while !reader.is_end().unwrap() {
        values.push(reader.deserialize().unwrap());
    }
    assert_eq!(values, [1, 2]);
    let value: Vec<&str> = STRICT.from_slice(br#"["a", "b"]"#).unwrap();
    assert_eq!(value, ["a", "b"]);
    assert!(STRICT.from_slice::<u32>(input).is_err());
}

#[test]
fn test_stream_deserializer_without_io() {
    use deser::stream::{InputBuffer, Status};
    use dialect::StreamDeserializer;

    // values are fed in, the deserializer frames or feeds them
    let mut buffer = InputBuffer::new(StreamDeserializer::with_config(STOP));
    buffer.extend_from_slice(b"[1]\n[2");
    assert_eq!(buffer.peek().unwrap(), Status::Ready);
    let mut out = None::<Vec<u32>>;
    let mut driver = deser::de::DeserializeDriver::new(&mut out);
    assert_eq!(buffer.drive_partial(&mut driver).unwrap(), Status::Ready);
    drop(driver);
    assert_eq!(out, Some(vec![1]));
    assert_eq!(buffer.peek().unwrap(), Status::Ready);
    buffer.extend_from_slice(b"]  ");
    buffer.set_eof();
    assert_eq!(buffer.poll().unwrap(), Status::Ready);
    assert_eq!(buffer.deserialize::<Vec<u32>>().unwrap(), [2]);
    assert_eq!(buffer.peek().unwrap(), Status::End);
}

#[test]
fn test_feeding_bounds_the_buffer() {
    use deser::de::DeserializeDriver;
    use deser::stream::{InputBuffer, Status};

    // a large value that arrives in chunks is deserialized while it
    // arrives, only incomplete tokens are buffered
    let long = "x".repeat(50);
    let mut input = String::from("[");
    // many chunks, fewer under miri which is slow
    let count = if cfg!(miri) { 150 } else { 10_000 };
    for idx in 0..count {
        if idx > 0 {
            input.push(',');
        }
        input.push_str(&format!("{{\"id\": {idx}, \"name\": \"{long}\"}}"));
    }
    input.push(']');

    let mut buffer = InputBuffer::new(dialect::StreamDeserializer::with_config(STRICT));
    let mut out = None::<Vec<std::collections::BTreeMap<String, Recording>>>;
    let mut max_buffered = 0;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        let chunks = input.as_bytes().chunks(1024).collect::<Vec<_>>();
        for (idx, chunk) in chunks.iter().enumerate() {
            buffer.extend_from_slice(chunk);
            let status = buffer.drive_partial(&mut driver).unwrap();
            // the value is complete with the last chunk
            if idx == chunks.len() - 1 {
                assert_eq!(status, Status::Ready);
            } else {
                assert_eq!(status, Status::NeedInput);
            }
            max_buffered = max_buffered.max(buffer.buffered());
        }
    }
    buffer.set_eof();
    assert_eq!(out.unwrap().len(), count);
    assert!(max_buffered < 100, "{max_buffered} bytes buffered");
    assert_eq!(
        buffer
            .drive_partial(&mut DeserializeDriver::new(&mut None::<u32>))
            .unwrap(),
        Status::End
    );
}

#[test]
fn test_feeding_with_layers() {
    use deser::de::Limits;

    let mut reader = STRICT.reader(Chunked {
        input: b"[1, 2, 3]",
        size: 2,
    });
    let err = reader
        .read_with::<Vec<u32>, _>(|driver| {
            driver.set_context(deser::Context::with(Limits::builder().max_items(2).build()))
        })
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "LimitExceeded: too many items at line 1 column 8"
    );
}

mod streamed {
    use deser::io::Reader;
    use deser::stream::{Part, Streamed};
    use deser::{Deserialize, Serialize};

    use super::dialect::{self, DeserializerConfig, Trailing};
    use super::{Blocking, Chunked, STRICT, chunk_sizes};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Item {
        id: u32,
        name: String,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Page {
        total: u32,
        items: Streamed<Item>,
        next: Option<String>,
    }

    fn item(id: u32) -> Item {
        Item {
            id,
            name: format!("item {id}"),
        }
    }

    fn read_all(
        reader: &mut Reader<impl std::io::Read, dialect::StreamDeserializer>,
    ) -> Vec<Part<Item, Page>> {
        let mut rv = Vec::new();
        while let Some(next) = reader.read_next::<Page, Item>().unwrap() {
            rv.push(next);
        }
        rv
    }

    #[test]
    fn test_elements_are_handed_out() {
        let page = Page {
            total: 3,
            items: (0..3).map(item).collect(),
            next: Some("cursor".into()),
        };
        let json = dialect::to_string(&page).unwrap();
        let expected = vec![
            Part::Element(item(0)),
            Part::Element(item(1)),
            Part::Element(item(2)),
            Part::Done(Page {
                total: 3,
                items: Streamed::new(),
                next: Some("cursor".into()),
            }),
        ];
        for size in chunk_sizes(json.len()) {
            let mut reader = STRICT.reader(Chunked {
                input: json.as_bytes(),
                size,
            });
            assert_eq!(read_all(&mut reader), expected, "size {size}");
        }
    }

    #[test]
    fn test_elements_are_handed_out_as_they_arrive() {
        // the stream stays open after the second element
        let input = b"{\"total\": 2, \"items\": [{\"id\": 0, \"name\": \"item 0\"}, {\"id\": 1, \"name\": \"item 1\"}";
        let mut reader = STRICT.reader(Blocking(input));
        assert_eq!(
            reader.read_next::<Page, Item>().unwrap(),
            Some(Part::Element(item(0)))
        );
        assert_eq!(
            reader.read_next::<Page, Item>().unwrap(),
            Some(Part::Element(item(1)))
        );
    }

    #[test]
    fn test_collected_like_a_vec() {
        let page: Page = dialect::from_str(
            r#"{"total": 1, "items": [{"id": 0, "name": "item 0"}], "next": null}"#,
        )
        .unwrap();
        assert_eq!(page.items.as_slice(), [item(0)]);
        assert_eq!(
            dialect::to_string(&page).unwrap(),
            r#"{"total":1,"items":[{"id":0,"name":"item 0"}],"next":null}"#
        );

        // without read_next the elements are collected
        let input = br#"{"total": 1, "items": [{"id": 0, "name": "item 0"}], "next": null}"#;
        let mut reader = STRICT.reader(&input[..]);
        assert_eq!(reader.read::<Page>().unwrap().unwrap().items.len(), 1);
    }

    #[test]
    fn test_framed_values() {
        // JSON Lines are read from frames, the elements are handed out once
        // the line is complete
        let lines = DeserializerConfig::builder()
            .trailing(Trailing::Newline)
            .build();
        let input = b"{\"total\": 1, \"items\": [{\"id\": 0, \"name\": \"item 0\"}], \"next\": null}\n{\"total\": 0, \"items\": [], \"next\": \"x\"}\n";
        let mut reader = lines.reader(&input[..]);
        assert_eq!(
            read_all(&mut reader),
            [
                Part::Element(item(0)),
                Part::Done(Page {
                    total: 1,
                    items: Streamed::new(),
                    next: None
                }),
                Part::Done(Page {
                    total: 0,
                    items: Streamed::new(),
                    next: Some("x".into())
                }),
            ]
        );
    }

    #[test]
    fn test_nested_and_atoms() {
        #[derive(Debug, PartialEq, Deserialize)]
        struct Outer {
            inner: Inner,
        }

        #[derive(Debug, PartialEq, Deserialize)]
        struct Inner {
            values: Streamed<u32>,
        }

        let input = br#"{"inner": {"values": [1, 2, 3]}} {"inner": {"values": []}}"#;
        let stop = DeserializerConfig::builder()
            .trailing(Trailing::Stop)
            .build();
        let mut reader = stop.reader(Chunked { input, size: 3 });
        let mut rv = Vec::new();
        while let Some(next) = reader.read_next::<Outer, u32>().unwrap() {
            rv.push(match next {
                Part::Element(value) => Some(value),
                Part::Done(_) => None,
            });
        }
        assert_eq!(rv, [Some(1), Some(2), Some(3), None, None]);
    }

    #[test]
    fn test_other_reads_while_reading_elements() {
        let input = br#"{"total": 1, "items": [{"id": 0, "name": "item 0"}], "next": null}"#;
        let mut reader = STRICT.reader(&input[..]);
        assert!(matches!(
            reader.read_next::<Page, Item>().unwrap(),
            Some(Part::Element(_))
        ));
        assert!(reader.read::<Page>().is_err());
        assert!(reader.read_next::<Page, u32>().is_err());
        assert!(matches!(
            reader.read_next::<Page, Item>().unwrap(),
            Some(Part::Done(_))
        ));
        assert!(reader.read_next::<Page, Item>().unwrap().is_none());
    }

    #[test]
    fn test_elements_bound_the_buffer() {
        use deser::stream::{ElementReader, ElementStatus, InputBuffer};

        // many chunks, fewer under miri which is slow
        let total = if cfg!(miri) { 150 } else { 10_000 };
        let page = Page {
            total,
            items: (0..total).map(item).collect(),
            next: None,
        };
        let json = dialect::to_string(&page).unwrap();
        let mut buffer = InputBuffer::new(dialect::StreamDeserializer::with_config(STRICT));
        let mut reader = ElementReader::<Page, Item>::new();
        let mut chunks = json.as_bytes().chunks(1024);
        let mut count = 0;
        let mut max_buffered = 0;
        loop {
            match reader.poll(&mut buffer).unwrap() {
                ElementStatus::Ready(Part::Element(element)) => {
                    assert_eq!(element, item(count));
                    count += 1;
                }
                ElementStatus::Ready(Part::Done(page)) => {
                    assert_eq!(page.total, total);
                    assert!(page.items.is_empty());
                    break;
                }
                ElementStatus::NeedInput => {
                    // only an incomplete token is left
                    max_buffered = max_buffered.max(buffer.buffered());
                    match chunks.next() {
                        Some(chunk) => buffer.extend_from_slice(chunk),
                        None => buffer.set_eof(),
                    }
                }
                ElementStatus::End => unreachable!(),
            }
        }
        assert_eq!(count, total);
        assert!(max_buffered < 100, "{max_buffered} bytes buffered");
    }
}
