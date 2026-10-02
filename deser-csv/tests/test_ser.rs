use std::collections::BTreeMap;

use deser::{Deserialize, ErrorKind, Serialize};
use deser_csv::{
    DeserializerConfig, Escape, Headers, Nulls, QuoteStyle, Serializer, SerializerConfig,
    Terminator, from_str, to_string,
};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Row {
    name: String,
    age: u32,
    #[deser(skip_serializing_if = Option::is_none)]
    note: Option<String>,
}

fn row(name: &str, age: u32, note: Option<&str>) -> Row {
    Row {
        name: name.into(),
        age,
        note: note.map(Into::into),
    }
}

#[test]
fn test_roundtrip() {
    let rows = vec![
        row("jane", 42, Some("likes \"quotes\", commas")),
        row("john\nsmith", 23, None),
        row("", 0, Some("")),
        row(" padded ", 1, Some("x\r\ny")),
    ];
    let csv = to_string(&rows).unwrap();
    assert_eq!(
        csv,
        "name,age,note\n\
         jane,42,\"likes \"\"quotes\"\", commas\"\n\
         \"john\nsmith\",23,\n\
         ,0,\n \
         padded ,1,\"x\r\ny\"\n"
    );
    let rows_back: Vec<Row> = from_str(&csv).unwrap();
    // the missing note is empty
    assert_eq!(rows_back[1].note.as_deref(), Some(""));
    assert_eq!(rows_back[0], rows[0]);
    assert_eq!(rows_back[2], rows[2]);
    assert_eq!(rows_back[3], rows[3]);
}

#[test]
fn test_null_and_empty() {
    // with `Nulls::Empty`, the empty string is quoted
    let rows = vec![(Some(""), None::<u32>)];
    let config = SerializerConfig::builder().nulls(Nulls::Empty).build();
    let csv = config.to_string(&rows).unwrap();
    assert_eq!(csv, "\"\",\n");
    let back: Vec<(Option<String>, Option<u32>)> = DeserializerConfig::builder()
        .headers(Headers::None)
        .nulls(Nulls::Empty)
        .build()
        .from_str(&csv)
        .unwrap();
    assert_eq!(back, [(Some("".into()), None)]);

    let config = SerializerConfig::builder()
        .nulls(Nulls::Text("NULL"))
        .build();
    let rows = vec![(Some("NULL"), None::<u32>)];
    assert_eq!(config.to_string(&rows).unwrap(), "\"NULL\",NULL\n");
}

#[test]
fn test_columns() {
    // the keys of the first record are the columns, later records are
    // written in their order
    let rows = vec![
        BTreeMap::from([("a", 1), ("b", 2)]),
        BTreeMap::from([("b", 4)]),
    ];
    assert_eq!(to_string(&rows).unwrap(), "a,b\n1,2\n,4\n");

    let rows = vec![BTreeMap::from([("a", 1)]), BTreeMap::from([("c", 4)])];
    let err = to_string(&rows).unwrap_err();
    assert_eq!(err.message(), "field `c` is not a column");

    let config = SerializerConfig::builder().headers(false).build();
    assert_eq!(
        config.to_string(&vec![BTreeMap::from([("a", 1)])]).unwrap(),
        "1\n"
    );
}

#[test]
fn test_sequences() {
    let rows = vec![vec![1, 2], vec![3, 4]];
    assert_eq!(to_string(&rows).unwrap(), "1,2\n3,4\n");
    let err = to_string(&vec![vec![1, 2], vec![3]]).unwrap_err();
    assert_eq!(err.message(), "record has 1 fields, expected 2");
    let config = SerializerConfig::builder().flexible(true).build();
    assert_eq!(
        config.to_string(&vec![vec![1, 2], vec![3]]).unwrap(),
        "1,2\n3\n"
    );
    // a single empty field is quoted, it would be a blank line otherwise
    assert_eq!(to_string(&vec![vec![""], vec!["a"]]).unwrap(), "\"\"\na\n");
    // no records
    assert_eq!(to_string(&Vec::<Row>::new()).unwrap(), "");
}

#[test]
fn test_unsupported() {
    let err = to_string(&42).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);
    assert_eq!(err.message(), "CSV documents are sequences of records");

    let err = to_string(&vec![1, 2]).unwrap_err();
    assert_eq!(err.message(), "CSV records must be maps or sequences");

    let err = to_string(&vec![vec![vec![1]]]).unwrap_err();
    assert_eq!(err.message(), "CSV fields cannot hold maps or sequences");
}

#[test]
fn test_values() {
    #[derive(Serialize)]
    struct Values {
        yes: bool,
        char: char,
        small: f32,
        large: f64,
        negative: i64,
        big: u128,
        bytes: Vec<u8>,
        unit: (),
    }
    let csv = to_string(&[Values {
        yes: true,
        char: 'x',
        small: 0.1,
        large: 1e100,
        negative: -1,
        big: u128::MAX,
        bytes: b"hi".to_vec(),
        unit: (),
    }])
    .unwrap();
    assert_eq!(
        csv,
        "yes,char,small,large,negative,big,bytes,unit\n\
         true,x,0.1,1e+100,-1,340282366920938463463374607431768211455,aGk=,\n"
    );
}

#[test]
fn test_dialects() {
    let rows = vec![("a;b", "c\td"), ("e", "f\"g")];
    let config = SerializerConfig::builder()
        .delimiter(b';')
        .terminator(Terminator::CrLf)
        .build();
    assert_eq!(
        config.to_string(&rows).unwrap(),
        "\"a;b\";c\td\r\ne;\"f\"\"g\"\r\n"
    );

    let config = SerializerConfig::builder()
        .escape(Escape::Backslash)
        .double_quote(false)
        .quote_style(QuoteStyle::NonNumeric)
        .build();
    assert_eq!(
        config.to_string(&vec![("a\"b\\", 1)]).unwrap(),
        "\"a\\\"b\\\\\",1\n"
    );

    let config = SerializerConfig::builder()
        .escape(Escape::Char(b'!'))
        .build();
    assert_eq!(
        config.to_string(&vec![("a,b!", "c")]).unwrap(),
        "a!,b!!,c\n"
    );

    let config = SerializerConfig::builder()
        .delimiter(0x1f)
        .terminator(Terminator::Byte(0x1e))
        .build();
    assert_eq!(
        config.to_string(&vec![("a", "b\nc")]).unwrap(),
        "a\x1fb\nc\x1e"
    );
}

#[test]
fn test_never_quote() {
    let config = SerializerConfig::builder()
        .quote_style(QuoteStyle::Never)
        .build();
    assert_eq!(config.to_string(&vec![("a", "b")]).unwrap(), "a,b\n");
    let err = config.to_string(&vec![("a,b", "c")]).unwrap_err();
    assert_eq!(err.message(), "field \"a,b\" needs to be quoted");

    let config = SerializerConfig::builder().quote(None).build();
    assert!(config.to_string(&vec![("a\nb", "c")]).is_err());
}

#[test]
fn test_tsv() {
    let rows = vec![
        ("tab\there", Some("line\nbreak")),
        ("back\\slash", None),
        ("\\N", Some("")),
    ];
    let tsv = SerializerConfig::tsv().to_string(&rows).unwrap();
    assert_eq!(
        tsv,
        "tab\\there\tline\\nbreak\nback\\\\slash\t\\N\n\\\\N\t\n"
    );
    let back: Vec<(String, Option<String>)> = DeserializerConfig::tsv()
        .into_builder()
        .headers(Headers::None)
        .build()
        .from_str(&tsv)
        .unwrap();
    let expected: Vec<(String, Option<String>)> = rows
        .iter()
        .map(|(a, b)| (a.to_string(), b.map(Into::into)))
        .collect();
    assert_eq!(back, expected);

    // text that reads as null is escaped
    let config = SerializerConfig::tsv()
        .into_builder()
        .nulls(Nulls::Text("NULL"))
        .build();
    let tsv = config.to_string(&vec![("NULL", None::<u32>)]).unwrap();
    assert_eq!(tsv, "\\NULL\tNULL\n");
}

#[test]
fn test_escape_formulas() {
    let config = SerializerConfig::builder().escape_formulas(true).build();
    let rows = vec![("=HYPERLINK(\"x\")", -1.5), ("+1", 2.0), ("a", 3.0)];
    assert_eq!(
        config.to_string(&rows).unwrap(),
        "\"'=HYPERLINK(\"\"x\"\")\",-1.5\n\"'+1\",2.0\na,3.0\n"
    );
}

#[test]
fn test_serializer() {
    let mut serializer = Serializer::new();
    serializer.serialize(&row("jane", 42, Some("x"))).unwrap();
    // failed records write nothing and do not change the columns
    assert!(serializer.serialize(&vec![vec![1]]).is_err());
    serializer.serialize(&row("john", 23, None)).unwrap();
    assert_eq!(serializer.as_str(), "name,age,note\njane,42,x\njohn,23,\n");

    // the columns are the keys of the first record, a field that is
    // skipped there cannot be written later
    let mut serializer = Serializer::new();
    serializer.serialize(&row("jane", 42, None)).unwrap();
    let err = serializer
        .serialize(&row("john", 23, Some("x")))
        .unwrap_err();
    assert_eq!(err.message(), "field `note` is not a column");
}

#[test]
fn test_serializer_first_record_fails() {
    // the names are only written with the first record that is written
    let mut serializer = Serializer::new();
    assert!(
        serializer
            .serialize(&BTreeMap::from([("a", vec![1])]))
            .is_err()
    );
    serializer.serialize(&BTreeMap::from([("b", 1)])).unwrap();
    assert_eq!(serializer.finish(), "b\n1\n");
}

#[test]
fn test_roundtrip_through_deserializer() {
    let rows = vec![row("a", 1, Some("x,y")), row("b\"c", 2, Some("\n"))];
    let csv = to_string(&rows).unwrap();
    let back: Vec<Row> = from_str(&csv).unwrap();
    assert_eq!(back, rows);
}

#[test]
fn test_field_order() {
    #[derive(Serialize)]
    struct Abc {
        a: u32,
        b: u32,
        c: u32,
    }

    #[derive(Serialize)]
    struct Acb {
        a: u32,
        c: u32,
        b: u32,
    }

    #[derive(Serialize)]
    struct Ab {
        a: u32,
        b: u32,
    }

    #[derive(Serialize)]
    struct Bc {
        b: u32,
        c: u32,
    }

    let mut serializer = Serializer::new();
    serializer.serialize(&Abc { a: 1, b: 2, c: 3 }).unwrap();
    serializer.serialize(&Acb { a: 4, c: 6, b: 5 }).unwrap();
    serializer.serialize(&Ab { a: 7, b: 8 }).unwrap();
    serializer.serialize(&Bc { b: 9, c: 10 }).unwrap();
    // a field after the one that is not a column
    assert!(
        serializer
            .serialize(&BTreeMap::from([("a", 1), ("d", 2)]))
            .is_err()
    );
    assert_eq!(serializer.finish(), "a,b,c\n1,2,3\n4,5,6\n7,8,\n,9,10\n");
}

#[test]
fn test_implicit() {
    use deser::ser::Emit;
    use deser::{Atom, Error, Implicit, ImplicitValue, State};

    // like the plain scalars of YAML
    struct Plain(&'static str, ImplicitValue);

    impl Serialize for Plain {
        fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
            Ok(Emit::Atom(Atom::Implicit(Implicit::new(value.0, value.1))))
        }
    }

    // values whose type was inferred from text are written as value
    let rows = vec![BTreeMap::from([
        ("a", Plain("0x1F", ImplicitValue::U64(31))),
        ("b", Plain("~", ImplicitValue::Null)),
        ("c", Plain("yes", ImplicitValue::Bool(true))),
        ("d", Plain("-1.50", ImplicitValue::F64(-1.5))),
    ])];
    assert_eq!(to_string(&rows).unwrap(), "a,b,c,d\n31,,true,-1.5\n");
}
