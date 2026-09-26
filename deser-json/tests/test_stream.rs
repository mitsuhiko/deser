use deser::de::Recording;
use deser::{Deserialize, ErrorKind};
use deser_json::{Deserializer, DeserializerConfig, Trailing};

const STRICT: DeserializerConfig = DeserializerConfig::new();
const NEWLINE: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Newline);
const STOP: DeserializerConfig = DeserializerConfig::new().trailing(Trailing::Stop);

#[derive(Deserialize, Debug, PartialEq)]
struct Row {
    id: u32,
    name: String,
}

fn collect<'a, T: Deserialize<'a>>(de: &mut Deserializer<'a>) -> Result<Vec<T>, String> {
    de.iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())
}

fn stream<'a, T: Deserialize<'a>>(
    config: &DeserializerConfig,
    input: &'a str,
) -> Result<Vec<T>, String> {
    collect(&mut Deserializer::from_str_with_config(input, config))
}

#[test]
fn test_default_is_strict() {
    assert_eq!(DeserializerConfig::default(), STRICT);
    assert_eq!(deser_json::from_str::<u32>(" 1 \n").unwrap(), 1);
    for (input, column) in [
        ("1\n2", "2 column 1"),
        ("[1] x", "1 column 5"),
        ("1-2", "1 column 2"),
        ("truefalse", "1 column 5"),
        ("{}{}", "1 column 3"),
    ] {
        assert_eq!(
            deser_json::from_str::<Recording>(input)
                .unwrap_err()
                .to_string(),
            format!("Unexpected: garbage after input at line {column}")
        );
        // the deserializer checks it too
        assert!(Deserializer::from_str(input)
            .deserialize::<Recording>()
            .is_err());
    }
    assert!(deser_json::from_slice::<u32>(b"1 x").is_err());

    // there is only one value
    let mut de = Deserializer::from_str("[1]\n");
    assert_eq!(collect::<Vec<u32>>(&mut de).unwrap(), [vec![1]]);
    assert!(de.is_end());
}

#[test]
fn test_newline() {
    let input = "{\"id\": 1, \"name\": \"a\"}\n{\"id\": 2, \"name\": \"b\"}\n";
    let mut de = Deserializer::from_str_with_config(input, &NEWLINE);
    assert_eq!(
        collect::<Row>(&mut de).unwrap(),
        [
            Row {
                id: 1,
                name: "a".into()
            },
            Row {
                id: 2,
                name: "b".into()
            }
        ]
    );
    assert!(de.is_end());
    assert!(de.end().is_ok());
    let err = de.deserialize::<Row>().unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);

    // blank lines are skipped, CRLF works and the last newline is optional
    assert_eq!(
        stream::<u32>(&NEWLINE, "1\n\n  \r\n2\r\n3  \n4").unwrap(),
        [1, 2, 3, 4]
    );

    // a value per line
    assert_eq!(
        stream::<u32>(&NEWLINE, "1 2\n").unwrap_err(),
        "Unexpected: expected end of line after value at line 1 column 3"
    );
    assert_eq!(
        stream::<Vec<u32>>(&NEWLINE, "[1,\n2]\n").unwrap_err(),
        "EndOfFile: unexpected end of file at line 1 column 4"
    );

    // from_str reads the first line
    assert_eq!(NEWLINE.from_str::<u32>("1\n2").unwrap(), 1);
    assert!(NEWLINE.from_str::<u32>("1 2\n3").is_err());
}

#[test]
fn test_newline_recover() {
    let input = concat!(
        "{\"id\": 1, \"name\": \"a\"}\n",
        "{\"id\": \"x\", \"name\": \"b\"}\n", // type error
        "{\"id\": 3, \"name\": \n",           // syntax error
        "\n",
        "{\"id\": 4, \"name\": \"d\"} x\n", // garbage
        "{\"id\": 5, \"name\": \"e\"}",
    );
    let mut de = Deserializer::from_str_with_config(input, &NEWLINE);
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    while !de.is_end() {
        match de.deserialize::<Row>() {
            Ok(row) => rows.push(row.id),
            Err(err) => errors.push(err.to_string()),
        }
    }
    assert_eq!(rows, [1, 5]);
    assert_eq!(
        errors,
        [
            "Unexpected: unexpected string, expected u32 at line 2 column 8",
            "EndOfFile: unexpected end of file at line 3 column 19",
            "Unexpected: expected end of line after value at line 5 column 24",
        ]
    );
}

#[test]
fn test_newline_from_slice() {
    // invalid UTF-8 only fails its line
    let mut de = Deserializer::from_slice_with_config(b"\"a\"\n\"\xff\"\n\"c\"\n", &NEWLINE);
    assert_eq!(de.deserialize::<String>().unwrap(), "a");
    assert!(de.deserialize::<String>().is_err());
    assert_eq!(de.deserialize::<String>().unwrap(), "c");
    assert!(de.is_end());
}

#[test]
fn test_stop() {
    // stops right after the value
    let mut de = Deserializer::from_str_with_config("[1] trash", &STOP);
    assert_eq!(de.deserialize::<Vec<u32>>().unwrap(), [1]);
    assert_eq!(de.offset(), 3);
    assert_eq!(
        de.end().unwrap_err().to_string(),
        "Unexpected: garbage after input at line 1 column 5"
    );
    assert_eq!(STOP.from_str::<u32>("1 trash").unwrap(), 1);
    assert_eq!(STOP.from_slice::<u32>(b"1 \xff").unwrap(), 1);

    // the next value continues after it, which reads concatenated JSON
    assert_eq!(
        stream::<Recording>(&STOP, "[1]{\"a\":2}\"x\" 3\n4")
            .unwrap()
            .len(),
        5
    );
    assert_eq!(stream::<i32>(&STOP, "1-2").unwrap(), [1, -2]);
    assert_eq!(stream::<bool>(&STOP, "truefalse").unwrap(), [true, false]);

    // after an error the stream cannot continue
    let mut de = Deserializer::from_str_with_config("1 \"x\" 3", &STOP);
    assert_eq!(de.deserialize::<u32>().unwrap(), 1);
    assert_eq!(
        de.deserialize::<u32>().unwrap_err().to_string(),
        "Unexpected: unexpected string, expected u32 at line 1 column 3"
    );
    assert!(de.is_end());
    assert_eq!(
        de.deserialize::<u32>().unwrap_err().to_string(),
        "Unexpected: cannot continue after an error"
    );
}

#[test]
fn test_empty() {
    for input in ["", "  \n\n \r\n"] {
        for config in [STRICT, NEWLINE, STOP] {
            let mut de = Deserializer::from_str_with_config(input, &config);
            assert!(de.is_end());
            assert_eq!(collect::<u32>(&mut de).unwrap(), Vec::<u32>::new());
            assert_eq!(
                de.deserialize::<u32>().unwrap_err().kind(),
                ErrorKind::EndOfFile
            );
        }
    }
}

#[test]
fn test_borrowed() {
    #[derive(Deserialize)]
    struct User<'a> {
        name: &'a str,
    }

    let users = stream::<User>(&NEWLINE, "{\"name\": \"a\"}\n{\"name\": \"b\"}\n").unwrap();
    assert_eq!(users.iter().map(|u| u.name).collect::<Vec<_>>(), ["a", "b"]);
}

#[test]
fn test_locations() {
    use deser_location::Spanned;

    let config = NEWLINE.track_locations(true);
    let items = stream::<Spanned<Vec<Spanned<u32>>>>(&config, "[1]\n\n  [2, 3]\n").unwrap();
    let spans = items
        .iter()
        .flat_map(|item| item.value.iter().map(|x| format!("{:?}", x.span.unwrap())))
        .collect::<Vec<_>>();
    assert_eq!(spans, ["1:2-1:3", "3:4-3:5", "3:7-3:8"]);
}
