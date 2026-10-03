use std::collections::BTreeMap;

use deser::ErrorKind;
use deser::ext::BigInt;
use deser_pickle::{
    Deserializer, DeserializerConfig, Form, Global, Kind, Object, Reference, from_slice,
};

use crate::common::Py;

/// `pickle.dumps(Point(1, 2), 4)` of a dataclass `Point` of `geometry`.
const POINT: &[u8] = b"\x80\x04\x95*\x00\x00\x00\x00\x00\x00\x00\x8c\x08geometry\x94\x8c\x05Point\x94\x93\x94)\x81\x94}\x94(\x8c\x01x\x94K\x01\x8c\x01y\x94K\x02ub.";

#[derive(Debug, PartialEq, deser::Deserialize)]
struct Point {
    x: i32,
    y: i32,
}

#[test]
fn test_scalars() {
    assert_eq!(from_slice::<Option<u32>>(b"N.").unwrap(), None);
    assert!(from_slice::<bool>(b"\x88.").unwrap());
    assert!(!from_slice::<bool>(b"I00\n.").unwrap());
    assert_eq!(from_slice::<i32>(b"J\xff\xff\xff\xff.").unwrap(), -1);
    assert_eq!(
        from_slice::<u64>(b"L18446744073709551615L\n.").unwrap(),
        u64::MAX
    );
    assert_eq!(
        from_slice::<f64>(b"G?\xf8\x00\x00\x00\x00\x00\x00.").unwrap(),
        1.5
    );
    assert_eq!(from_slice::<f64>(b"F-inf\n.").unwrap(), f64::NEG_INFINITY);
    assert_eq!(from_slice::<String>(b"V\\u00e9t\\u00e9\n.").unwrap(), "été");
    assert_eq!(from_slice::<Vec<u8>>(b"C\x02\x00\xff.").unwrap(), [0, 255]);
}

#[test]
fn test_big_integers() {
    // 2**64 with LONG1
    let value: u128 = from_slice(b"\x8a\x09\x00\x00\x00\x00\x00\x00\x00\x00\x01.").unwrap();
    assert_eq!(value, 1 << 64);
    let value: i128 = from_slice(b"L-170141183460469231731687303715884105728\n.").unwrap();
    assert_eq!(value, i128::MIN);
    // 2**200
    let mut input = b"\x8a\x1a".to_vec();
    input.extend_from_slice(&[0; 25]);
    input.extend_from_slice(b"\x01.");
    let value: BigInt = from_slice(&input).unwrap();
    assert_eq!(
        value.to_string(),
        "1606938044258990275541962092341162602522202993782792835301376"
    );
    let value: Py = from_slice(&input).unwrap();
    assert_eq!(value, Py::Int(value_text(&value)));
    assert_eq!(
        value_text(&value),
        "1606938044258990275541962092341162602522202993782792835301376"
    );
}

fn value_text(value: &Py) -> String {
    match value {
        Py::Int(text) => text.clone(),
        other => panic!("not an int: {:?}", other),
    }
}

#[test]
fn test_python2_strings() {
    // `str` of Python 2 is text if it's UTF-8 and bytes otherwise
    assert_eq!(from_slice::<String>(b"U\x03abc.").unwrap(), "abc");
    assert_eq!(from_slice::<String>(b"S'a\\nb'\n.").unwrap(), "a\nb");
    assert_eq!(
        from_slice::<Py>(b"T\x01\x00\x00\x00\xff.").unwrap(),
        Py::Bytes(vec![255])
    );
}

#[test]
fn test_kinds() {
    // `({1}, frozenset(), bytearray(b"a"))`
    let value: Py =
        from_slice(b"\x80\x04\x8f(K\x01\x90(\x91\x96\x01\x00\x00\x00\x00\x00\x00\x00a\x87.")
            .unwrap();
    assert_eq!(
        value,
        Py::Seq(
            Some(Kind::Tuple),
            vec![
                Py::Seq(Some(Kind::Set), vec![Py::Int("1".into())]),
                Py::Seq(Some(Kind::FrozenSet), vec![]),
                Py::ByteArray(b"a".to_vec()),
            ]
        )
    );
    let value: Py = from_slice(b"\x96\x01\x00\x00\x00\x00\x00\x00\x00a.").unwrap();
    assert_eq!(value, Py::ByteArray(b"a".to_vec()));
    // types that do not care see sequences and bytes
    let value: (u32, u32) = from_slice(b"K\x01K\x02\x86.").unwrap();
    assert_eq!(value, (1, 2));
}

#[test]
fn test_empty_containers_are_both() {
    // an empty dict is an empty sequence and an empty list a struct
    let value: Vec<u32> = from_slice(b"}.").unwrap();
    assert!(value.is_empty());
    #[derive(Debug, Default, PartialEq, deser::Deserialize)]
    struct Empty {
        #[deser(default)]
        x: u32,
    }
    assert_eq!(from_slice::<Empty>(b"].").unwrap(), Empty::default());
}

#[test]
fn test_dict_keys() {
    // `{1: "a", (1, 2): "b"}`
    let value: Py =
        from_slice(b"}(K\x01X\x01\x00\x00\x00aK\x01K\x02\x86X\x01\x00\x00\x00bu.").unwrap();
    let Py::Map(entries) = value else {
        panic!("not a map");
    };
    assert_eq!(
        entries[1].0,
        Py::Seq(
            Some(Kind::Tuple),
            vec![Py::Int("1".into()), Py::Int("2".into())]
        )
    );
    let value: BTreeMap<u32, String> = from_slice(b"}(K\x01X\x01\x00\x00\x00au.").unwrap();
    assert_eq!(value[&1], "a");
}

#[test]
fn test_objects() {
    assert_eq!(from_slice::<Point>(POINT).unwrap(), Point { x: 1, y: 2 });
    let point: Object<BTreeMap<String, i32>> = from_slice(POINT).unwrap();
    assert_eq!(point.class, Some(Global::new("geometry", "Point")));
    assert_eq!(point.form, Some(Form::State));
    assert_eq!(point.value["x"], 1);

    // `Decimal("1.50")` is its argument
    let input = b"\x80\x04\x95\"\x00\x00\x00\x00\x00\x00\x00\x8c\x07decimal\x94\x8c\x07Decimal\x94\x93\x94\x8c\x041.50\x94\x85\x94R\x94.";
    let value: Object<String> = from_slice(input).unwrap();
    assert_eq!(value.class, Some(Global::new("decimal", "Decimal")));
    assert_eq!(value.form, Some(Form::Argument));
    assert_eq!(value.value, "1.50");

    // `OrderedDict([("b", 1)])` is its items
    let input = b"\x80\x04\x95)\x00\x00\x00\x00\x00\x00\x00\x8c\x0bcollections\x94\x8c\x0bOrderedDict\x94\x93\x94)R\x94\x8c\x01b\x94K\x01s.";
    let value: Object<BTreeMap<String, u32>> = from_slice(input).unwrap();
    assert_eq!(value.form, Some(Form::Items));
    assert_eq!(value.value["b"], 1);

    // objects are never instantiated: protocol 0 with `copy_reg`
    let input = b"ccopy_reg\n_reconstructor\n(cgeometry\nPoint\nc__builtin__\nobject\nNtR(dS'x'\nI1\nsS'y'\nI2\nsb.";
    assert_eq!(from_slice::<Point>(input).unwrap(), Point { x: 1, y: 2 });
}

#[test]
fn test_python2_names() {
    // like Python, the names of Python 2 are read as the ones of Python 3
    // before protocol 3
    let global: Global = from_slice(b"c__builtin__\nunicode\n.").unwrap();
    assert_eq!(global, Global::new("builtins", "str"));
    let global: Global = from_slice(b"\x80\x03c__builtin__\nunicode\n.").unwrap();
    assert_eq!(global, Global::new("__builtin__", "unicode"));
}

#[test]
fn test_globals() {
    let input = b"\x80\x04\x8c\x0bcollections\x8c\x0bOrderedDict\x93.";
    let global: Global = from_slice(input).unwrap();
    assert_eq!(global.module(), "collections");
    assert_eq!(global.name(), "OrderedDict");
    assert_eq!(
        from_slice::<String>(input).unwrap(),
        "collections.OrderedDict"
    );
}

#[test]
fn test_cycles() {
    // `x = []; x.append(x)`
    let input = b"\x80\x04]\x94h\x00a.";
    let value: Py = from_slice(input).unwrap();
    let Py::Shared(id, inner) = value else {
        panic!("not shared");
    };
    assert_eq!(*inner, Py::Seq(None, vec![Py::Ref(id)]));
    let value: Vec<Reference> = from_slice(input).unwrap();
    assert_eq!(value, [Reference::new(id)]);
    // types that do not understand references see null
    let value: Vec<Option<Vec<u32>>> = from_slice(input).unwrap();
    assert_eq!(value, [None]);
    let err = from_slice::<Vec<Vec<u32>>>(input).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidType);
}

#[test]
fn test_shared_values() {
    // `x = [1]; [x, x]`
    let input = b"\x80\x04](]\x94K\x01ah\x00e.";
    let value: Vec<Vec<u32>> = from_slice(input).unwrap();
    assert_eq!(value, [[1], [1]]);
    let value: Py = from_slice(input).unwrap();
    let Py::Seq(None, items) = value else {
        panic!("not a list");
    };
    assert_eq!(items[0], items[1]);
    assert!(matches!(items[0], Py::Shared(..)));
}

/// A list of lists that hold the list of the level below twice.
fn doubling(levels: u8) -> Vec<u8> {
    let mut input = b"\x80\x04]\x94".to_vec();
    for idx in 0..levels {
        input.extend_from_slice(&[b']', b'(', b'h', idx, b'h', idx, b'e', 0x94]);
    }
    input.push(b'.');
    input
}

#[test]
fn test_max_shared_events() {
    // 2**30 lists in the end
    let err = from_slice::<Py>(&doubling(30)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::LimitExceeded);
    // 2**10 lists fit
    assert!(from_slice::<Py>(&doubling(10)).is_ok());
    let config = DeserializerConfig::builder().max_shared_events(100).build();
    let err = config.from_slice::<Py>(&doubling(10)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::LimitExceeded);
}

#[test]
fn test_errors() {
    let err = from_slice::<Py>(b"").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
    let err = from_slice::<Py>(b"N").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::EndOfFile);
    let err = from_slice::<Py>(b"K\x01z.").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Syntax);
    assert_eq!(err.offset(), Some(2));
    let err = from_slice::<Py>(b"N.N.").unwrap_err();
    assert_eq!(err.offset(), Some(2));
    let err = from_slice::<Py>(b"Pid\n.").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);
    let err = from_slice::<Py>(b"\x80\x06N.").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);
    // strings with surrogates
    assert!(from_slice::<Py>(b"V\\ud800\n.").is_err());
}

#[test]
fn test_concatenated() {
    let mut de = Deserializer::from_slice(b"K\x01.\x80\x04\x8c\x02hi.N.");
    assert_eq!(de.deserialize::<u32>().unwrap(), 1);
    assert_eq!(de.offset(), 3);
    assert_eq!(de.deserialize::<String>().unwrap(), "hi");
    assert_eq!(de.deserialize::<Option<u32>>().unwrap(), None);
    assert!(de.is_end());
    de.end().unwrap();
}

#[test]
fn test_borrowed_strings() {
    #[derive(deser::Deserialize)]
    struct Borrowed<'a> {
        name: &'a str,
        data: &'a [u8],
    }
    let input = b"\x80\x04}(\x8c\x04name\x8c\x04Jane\x8c\x04dataC\x02ab\x75.";
    let value: Borrowed = from_slice(input).unwrap();
    assert_eq!(value.name, "Jane");
    assert_eq!(value.data, b"ab");
}
