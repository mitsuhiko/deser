//! Runs the test files of the [`plist` crate](https://github.com/ebarnard/rust-plist).
//!
//! Every file that Core Foundation reads is compared against its
//! conversion to XML by `plutil` (see `scripts/update-plist-test-data.sh`),
//! the files that Core Foundation rejects have to fail.  Some files have
//! additional checks that mirror the tests of the `plist` crate.
use std::fs;
use std::path::{Path, PathBuf};

use deser::ext::Timestamp;
use deser_plist::{Format, SerializerConfig};

use crate::common::{Value, parse};

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/rust-plist");

fn corpus() -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(DATA)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.is_file()
                && !matches!(
                    path.file_name().unwrap().to_str(),
                    Some("LICENSE" | "SOURCE")
                )
        })
        .collect();
    files.sort();
    files
}

fn read(name: &str) -> Vec<u8> {
    fs::read(Path::new(DATA).join(name)).unwrap()
}

fn errors() -> Vec<String> {
    fs::read_to_string(Path::new(DATA).join("expected/errors.txt"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn test_against_plutil() {
    let errors = errors();
    let mut checked = 0;
    for path in corpus() {
        let name = path.file_name().unwrap().to_str().unwrap();
        let input = fs::read(&path).unwrap();
        if errors.iter().any(|x| x == name) {
            assert!(
                deser_plist::from_slice::<Value>(&input).is_err(),
                "{name} should fail"
            );
            continue;
        }
        let value =
            deser_plist::from_slice::<Value>(&input).unwrap_or_else(|err| panic!("{name}: {err}"));
        let expected = parse(&read(&format!("expected/{name}.xml")));
        assert!(value.sorted() == expected, "{name} does not match plutil");
        checked += 1;
    }
    assert_eq!(checked, 10);
}

#[test]
fn test_formats() {
    for path in corpus() {
        let name = path.file_name().unwrap().to_str().unwrap();
        let expected = if name.starts_with("binary") || name.starts_with("utf16") {
            Format::Binary
        } else if name.starts_with("xml") || name == "book.plist" {
            Format::Xml
        } else {
            Format::Ascii
        };
        assert_eq!(
            Format::detect(&fs::read(&path).unwrap()),
            expected,
            "{name}"
        );
    }
}

/// Writes every file in all formats and reads it back.
#[test]
fn test_roundtrip() {
    let errors = errors();
    for path in corpus() {
        let name = path.file_name().unwrap().to_str().unwrap();
        if errors.iter().any(|x| x == name) {
            continue;
        }
        let value = parse(&fs::read(&path).unwrap());
        for format in [Format::Xml, Format::Binary, Format::Ascii] {
            let bytes = SerializerConfig::builder()
                .format(format)
                .build()
                .to_vec(&value)
                .unwrap();
            assert_eq!(Format::detect(&bytes), format, "{name}");
            let back = parse(&bytes);
            if format == Format::Ascii && !name.starts_with("ascii") && !name.ends_with("pbxproj") {
                // OpenStep has only strings, data, arrays and dictionaries
                continue;
            }
            assert!(back == value, "{name} as {format:?}");
        }
    }
}

/// Our XML output of the files matches what `plutil` writes.
#[test]
fn test_xml_output_matches_plutil() {
    for name in [
        "ascii-animals.plist",
        "ascii-sample.plist",
        "binary_NSKeyedArchiver.plist",
        "netnewswire.pbxproj",
        "utf16_bplist.plist",
        "xml-animals.plist",
    ] {
        let value = parse(&read(name)).sorted();
        let xml = SerializerConfig::new().to_string(&value).unwrap();
        let expected = String::from_utf8(read(&format!("expected/{name}.xml"))).unwrap();
        assert!(xml == expected, "{name}:\n{xml}");
    }
}

fn date(s: &str) -> Timestamp {
    s.parse().unwrap()
}

/// The contents of `binary.plist` and `xml.plist` in document order.
fn book(with_empty_containers: bool) -> Value {
    let mut entries = vec![
        ("Author", Value::from("William Shakespeare")),
        (
            "Lines",
            array![
                "It is a tale told by an idiot,     ",
                "Full of sound and fury, signifying nothing."
            ],
        ),
        ("Death", Value::from(1564u64)),
        ("Height", Value::from(1.6)),
        (
            "Data",
            Value::Bytes(vec![0, 0, 0, 190, 0, 0, 0, 3, 0, 0, 0, 30, 0, 0, 0]),
        ),
        ("Birthdate", Value::from(date("1981-05-16T11:32:06Z"))),
        ("Blank", Value::from("")),
        ("BiggestNumber", Value::from(u64::MAX)),
        ("SmallestNumber", Value::from(i64::MIN)),
    ];
    if with_empty_containers {
        entries.push(("EmptyArray", array![]));
        entries.push(("EmptyDictionary", map! {}));
    } else {
        entries.push(("HexademicalNumber", Value::from(0xdeadbeefu64)));
    }
    entries.push(("IsTrue", Value::from(true)));
    entries.push(("IsNotFalse", Value::from(false)));
    Value::Map(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

#[test]
fn test_binary() {
    // the order of the binary file differs, the plist crate checks it
    let value = parse(&read("binary.plist"));
    let Value::Map(ref entries) = value else {
        panic!()
    };
    let keys: Vec<_> = entries.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        [
            "Author",
            "Birthdate",
            "EmptyArray",
            "IsNotFalse",
            "SmallestNumber",
            "EmptyDictionary",
            "Height",
            "Lines",
            "Death",
            "Blank",
            "BiggestNumber",
            "IsTrue",
            "Data",
        ]
    );
    assert_eq!(value.sorted(), book(true).sorted());
}

#[test]
fn test_xml() {
    let mut value = parse(&read("xml.plist"));
    // the last entry is not in the binary file
    let Value::Map(ref mut entries) = value else {
        panic!()
    };
    assert_eq!(
        entries.pop().unwrap(),
        ("Pets".to_string(), Value::from("A cat & a dog."))
    );
    assert_eq!(value, book(false));
}

#[test]
fn test_utf16() {
    let value = parse(&read("utf16_bplist.plist"));
    let Value::Map(ref entries) = value else {
        panic!()
    };
    assert_eq!(entries[0].1, Value::from("\u{2605} or better"));
    let Value::Str(ref poem) = entries[1].1 else {
        panic!()
    };
    // the plist crate checks the length in bytes
    assert_eq!(poem.len(), 643);
    assert!(poem.ends_with('\u{2605}'));
}

#[test]
fn test_keyed_archive() {
    let value = parse(&read("binary_NSKeyedArchiver.plist"));
    let Value::Array(ref objects) = *value.get("$objects") else {
        panic!()
    };
    assert_eq!(objects[1].get("$class"), &Value::Uid(4));
    assert_eq!(objects[1].get("NSRangeData"), &Value::Uid(2));
    assert_eq!(value.get("$top").get("foundItems"), &Value::Uid(1));
}

#[test]
fn test_three_byte_offsets() {
    let value = parse(&read("binary_three_byte_integer_offset_table.plist"));
    let Value::Map(ref entries) = value else {
        panic!()
    };
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].0, "data");
    let Value::Map(ref data) = entries[0].1 else {
        panic!()
    };
    assert_eq!(data.len(), 2199);
    assert_eq!(data[0].0, "1838");
}

#[test]
fn test_ascii() {
    assert_eq!(
        parse(&read("ascii-sample.plist")),
        map! {
            "KeyName1" => "Value1",
            "AnotherKeyName" => "Value2",
            "Something" => array!["ArrayItem1", "ArrayItem2", "ArrayItem3"],
            "Key4" => "0.10",
            "KeyFive" => map! {
                "Dictionary2Key1" => "Something",
                "AnotherKey" => "Somethingelse",
            },
        }
    );
    let project = parse(&read("netnewswire.pbxproj"));
    assert_eq!(project.get("archiveVersion"), &Value::from("1"));
    assert_eq!(project.get("objectVersion"), &Value::from("46"));
}

#[test]
fn test_errors() {
    for (name, message) in [
        // the trailer is broken, see `test_de::test_binary_cycle` for cycles
        ("binary_circular_array.plist", "invalid number of objects"),
        (
            "binary_zero_offset_size.plist",
            "invalid integer sizes in trailer",
        ),
        ("xml_error.plist", "expected `>` at line 17 column 2"),
        (
            "xml_entity_error.plist",
            "unexpected content, expected a tag",
        ),
    ] {
        let err = deser_plist::from_slice::<Value>(&read(name)).unwrap_err();
        assert!(err.to_string().contains(message), "{name}: {err}");
    }
}
