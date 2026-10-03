use std::collections::BTreeMap;

use deser::ErrorKind;
use deser::ext::BigInt;
use deser_pickle::{
    Form, Global, Kind, Object, Reference, Serializer, SerializerConfig, from_slice, to_vec,
};

use crate::common::Py;

fn with_protocol<T: deser::Serialize>(protocol: u8, value: &T) -> Vec<u8> {
    SerializerConfig::builder()
        .protocol(protocol)
        .build()
        .to_vec(value)
        .unwrap()
}

fn int(value: i64) -> Py {
    Py::Int(value.to_string())
}

#[test]
fn test_scalars() {
    assert_eq!(to_vec(&()).unwrap(), b"\x80\x04N.");
    assert_eq!(to_vec(&true).unwrap(), b"\x80\x04\x88.");
    assert_eq!(to_vec(&255u8).unwrap(), b"\x80\x04K\xff.");
    assert_eq!(to_vec(&65535u16).unwrap(), b"\x80\x04M\xff\xff.");
    assert_eq!(to_vec(&-1i32).unwrap(), b"\x80\x04J\xff\xff\xff\xff.");
    assert_eq!(
        to_vec(&(1u64 << 40)).unwrap(),
        b"\x80\x04\x8a\x06\x00\x00\x00\x00\x00\x01."
    );
    assert_eq!(
        to_vec(&i64::MIN).unwrap(),
        b"\x80\x04\x8a\x08\x00\x00\x00\x00\x00\x00\x00\x80."
    );
    assert_eq!(
        to_vec(&u64::MAX).unwrap(),
        b"\x80\x04\x8a\x09\xff\xff\xff\xff\xff\xff\xff\xff\x00."
    );
    assert_eq!(
        to_vec(&1.5f64).unwrap(),
        b"\x80\x04G?\xf8\x00\x00\x00\x00\x00\x00."
    );
    assert_eq!(to_vec(&"hé").unwrap(), b"\x80\x04\x8c\x03h\xc3\xa9.");
    assert_eq!(
        with_protocol(2, &"hé"),
        b"\x80\x02X\x03\x00\x00\x00h\xc3\xa9."
    );
}

#[test]
fn test_big_integers() {
    let value: BigInt = "-1606938044258990275541962092341162602522202993782792835301376"
        .parse()
        .unwrap();
    let output = to_vec(&value).unwrap();
    let mut expected = b"\x80\x04\x8a\x1a".to_vec();
    expected.extend_from_slice(&[0; 25]);
    expected.extend_from_slice(b"\xff.");
    assert_eq!(output, expected);
    assert_eq!(from_slice::<BigInt>(&output).unwrap(), value);
    assert_eq!(
        from_slice::<i128>(&to_vec(&i128::MIN).unwrap()).unwrap(),
        i128::MIN
    );
    assert_eq!(
        from_slice::<u128>(&to_vec(&u128::MAX).unwrap()).unwrap(),
        u128::MAX
    );
}

#[test]
fn test_bytes() {
    let value = Py::Bytes(b"a\xff".to_vec());
    assert_eq!(to_vec(&value).unwrap(), b"\x80\x04C\x02a\xff.");
    // protocol 2 has no bytes, Python 3 writes `_codecs.encode`
    assert_eq!(
        with_protocol(2, &value),
        b"\x80\x02c_codecs\nencode\nX\x03\x00\x00\x00a\xc3\xbfX\x06\x00\x00\x00latin1\x86R."
    );
    assert_eq!(
        with_protocol(2, &Py::Bytes(vec![])),
        b"\x80\x02c__builtin__\nbytes\n)R."
    );
    let value = Py::ByteArray(b"a".to_vec());
    assert_eq!(
        with_protocol(5, &value),
        b"\x80\x05\x96\x01\x00\x00\x00\x00\x00\x00\x00a."
    );
    assert_eq!(
        with_protocol(4, &value),
        b"\x80\x04\x8c\x08builtins\x8c\tbytearray\x93C\x01a\x85R."
    );
    assert_eq!(
        with_protocol(2, &value),
        b"\x80\x02c__builtin__\nbytearray\nX\x01\x00\x00\x00aX\x07\x00\x00\x00latin-1\x86R."
    );
    for protocol in 2..=5 {
        let output = with_protocol(protocol, &value);
        assert_eq!(from_slice::<Py>(&output).unwrap(), value);
    }
}

#[test]
fn test_containers() {
    assert_eq!(to_vec(&vec![1, 2]).unwrap(), b"\x80\x04](K\x01K\x02e.");
    let mut map = BTreeMap::new();
    map.insert("a", 1);
    assert_eq!(to_vec(&map).unwrap(), b"\x80\x04}(\x8c\x01aK\x01u.");
    let tuple = Py::Seq(Some(Kind::Tuple), vec![int(1), int(2)]);
    assert_eq!(to_vec(&tuple).unwrap(), b"\x80\x04(K\x01K\x02t.");
    let set = Py::Seq(Some(Kind::Set), vec![int(1)]);
    assert_eq!(to_vec(&set).unwrap(), b"\x80\x04\x8f(K\x01\x90.");
    assert_eq!(
        with_protocol(3, &set),
        b"\x80\x03cbuiltins\nset\n(K\x01l\x85R."
    );
    assert_eq!(
        with_protocol(2, &set),
        b"\x80\x02c__builtin__\nset\n(K\x01l\x85R."
    );
    let frozenset = Py::Seq(Some(Kind::FrozenSet), vec![int(1)]);
    assert_eq!(to_vec(&frozenset).unwrap(), b"\x80\x04(K\x01\x91.");
    for value in [tuple, set, frozenset] {
        for protocol in 2..=5 {
            let output = with_protocol(protocol, &value);
            assert_eq!(from_slice::<Py>(&output).unwrap(), value);
        }
    }
}

#[test]
fn test_keys() {
    // sequences in keys are tuples, sets frozensets
    let value = Py::Map(vec![(
        Py::Seq(None, vec![int(1), Py::Seq(Some(Kind::Set), vec![])]),
        Py::Null,
    )]);
    assert_eq!(to_vec(&value).unwrap(), b"\x80\x04}((K\x01(\x91tNu.");
    let value = Py::Map(vec![(Py::Map(vec![]), Py::Null)]);
    let err = to_vec(&value).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);
}

#[derive(deser::Serialize, deser::Deserialize, Debug, PartialEq)]
struct Point {
    x: i32,
    y: i32,
}

#[test]
fn test_objects() {
    let point = Object::new(Global::new("geometry", "Point"), Point { x: 1, y: 2 });
    let output = to_vec(&point).unwrap();
    assert_eq!(
        output,
        b"\x80\x04\x8c\x08geometry\x8c\x05Point\x93)\x81}(\x8c\x01xK\x01\x8c\x01yK\x02ub."
    );
    let again: Object<Point> = from_slice(&output).unwrap();
    assert_eq!(again.class, point.class);
    assert_eq!(again.form, Some(Form::State));
    assert_eq!(again.value, point.value);
    assert_eq!(
        with_protocol(2, &point),
        b"\x80\x02cgeometry\nPoint\n)\x81}(X\x01\x00\x00\x00xK\x01X\x01\x00\x00\x00yK\x02ub."
    );

    let class = Global::new("decimal", "Decimal");
    let value = Object::new(class.clone(), "1.5");
    assert_eq!(
        to_vec(&value).unwrap(),
        b"\x80\x04\x8c\x07decimal\x8c\x07Decimal\x93\x8c\x031.5\x85R."
    );

    // the forms
    let class = Global::new("m", "C");
    let items = Object::with_form(class.clone(), Form::Items, BTreeMap::from([("a", 1)]));
    assert_eq!(
        to_vec(&items).unwrap(),
        b"\x80\x04\x8c\x01m\x8c\x01C\x93)\x81(\x8c\x01aK\x01u."
    );
    let slots = Object::with_form(class.clone(), Form::Slots, BTreeMap::from([("a", 1)]));
    assert_eq!(
        to_vec(&slots).unwrap(),
        b"\x80\x04\x8c\x01m\x8c\x01C\x93)\x81N}(\x8c\x01aK\x01u\x86b."
    );
    let kwargs = Object::with_form(class.clone(), Form::Arguments, BTreeMap::from([("a", 1)]));
    assert_eq!(
        to_vec(&kwargs).unwrap(),
        b"\x80\x04\x8c\x01m\x8c\x01C\x93)}(\x8c\x01aK\x01u\x92."
    );
    let err = SerializerConfig::builder()
        .protocol(3)
        .build()
        .to_vec(&kwargs)
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);
    let args = Object::with_form(class.clone(), Form::Arguments, (1, 2));
    assert_eq!(
        to_vec(&args).unwrap(),
        b"\x80\x04\x8c\x01m\x8c\x01C\x93(K\x01K\x02tR."
    );
    let arg = Object::with_form(class.clone(), Form::Argument, vec![1]);
    assert_eq!(
        to_vec(&arg).unwrap(),
        b"\x80\x04\x8c\x01m\x8c\x01C\x93](K\x01e\x85R."
    );
    let list = Object::new(class.clone(), vec![1]);
    assert_eq!(
        to_vec(&list).unwrap(),
        b"\x80\x04\x8c\x01m\x8c\x01C\x93)\x81(K\x01e."
    );
    for (value, form) in [
        (to_vec(&items).unwrap(), Form::Items),
        (to_vec(&slots).unwrap(), Form::Slots),
        (to_vec(&kwargs).unwrap(), Form::Arguments),
        (to_vec(&args).unwrap(), Form::Arguments),
        (to_vec(&arg).unwrap(), Form::Argument),
        (to_vec(&list).unwrap(), Form::Items),
    ] {
        let again: Object<Py> = from_slice(&value).unwrap();
        assert_eq!(again.form, Some(form));
    }
}

#[test]
fn test_globals() {
    let global = Global::new("collections", "OrderedDict");
    assert_eq!(
        to_vec(&global).unwrap(),
        b"\x80\x04\x8c\x0bcollections\x8c\x0bOrderedDict\x93."
    );
    assert_eq!(
        with_protocol(3, &global),
        b"\x80\x03ccollections\nOrderedDict\n."
    );
    // protocol 2 writes the names of Python 2
    assert_eq!(
        with_protocol(2, &Global::new("builtins", "str")),
        b"\x80\x02c__builtin__\nunicode\n."
    );
    let err = SerializerConfig::builder()
        .protocol(3)
        .build()
        .to_vec(&Global::new("a\nb", "c"))
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidValue);
}

#[test]
fn test_shared_and_cycles() {
    // `x = [1]; [x, x]`
    let shared = Py::Shared(7, Box::new(Py::Seq(None, vec![int(1)])));
    let value = Py::Seq(None, vec![shared.clone(), shared]);
    let output = to_vec(&value).unwrap();
    assert_eq!(output, b"\x80\x04](]\x94(K\x01eh\x00e.");
    assert_eq!(with_protocol(3, &value), b"\x80\x03](]q\x00(K\x01eh\x00e.");
    assert_eq!(
        from_slice::<Py>(&output).unwrap().normalized(),
        value.normalized()
    );

    // `x = []; x.append(x)`
    let value = Py::Shared(3, Box::new(Py::Seq(None, vec![Py::Ref(3)])));
    let output = to_vec(&value).unwrap();
    assert_eq!(output, b"\x80\x04]\x94(h\x00e.");
    assert_eq!(
        from_slice::<Py>(&output).unwrap().normalized(),
        value.normalized()
    );

    // tuples cannot contain themselves
    let value = Py::Shared(
        3,
        Box::new(Py::Seq(
            Some(Kind::Tuple),
            vec![Py::Seq(None, vec![Py::Ref(3)])],
        )),
    );
    assert_eq!(to_vec(&value).unwrap_err().kind(), ErrorKind::InvalidValue);
    let value = Py::Seq(None, vec![Py::Ref(1)]);
    assert_eq!(to_vec(&value).unwrap_err().kind(), ErrorKind::InvalidValue);
    let value = Reference::new(1);
    assert_eq!(to_vec(&value).unwrap_err().kind(), ErrorKind::InvalidValue);
}

#[test]
fn test_protocols() {
    for protocol in [0, 1, 6] {
        let err = SerializerConfig::builder()
            .protocol(protocol)
            .build()
            .to_vec(&1)
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Configuration);
    }
}

#[test]
fn test_serializer() {
    let mut serializer = Serializer::new();
    serializer.serialize(&1).unwrap();
    serializer.serialize(&"a").unwrap();
    let output = serializer.finish();
    assert_eq!(output, b"\x80\x04K\x01.\x80\x04\x8c\x01a.");
    let mut de = deser_pickle::Deserializer::from_slice(&output);
    assert_eq!(de.deserialize::<u32>().unwrap(), 1);
    assert_eq!(de.deserialize::<String>().unwrap(), "a");
}
