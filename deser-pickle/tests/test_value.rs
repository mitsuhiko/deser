//! Values that are buffered keep what pickles have beyond the data model:
//! classes, kinds, shared values and cycles.
use deser::de::Recording;
use deser_value::Value;

use crate::common::Py;

/// `pickle.dumps(root, 4)` of a `tree.Node` with a child whose parent is
/// the root, a tuple and a set.
const INPUT: &[u8] = b"\x80\x04\x95[\x00\x00\x00\x00\x00\x00\x00\x8c\x04tree\x94\x8c\x04Node\x94\x93\x94)\x81\x94}\x94(\x8c\x04name\x94\x8c\x04root\x94\x8c\x06parent\x94N\x8c\x08children\x94]\x94h\x02)\x81\x94}\x94(h\x05\x8c\x05child\x94h\x07h\x03h\x08]\x94ubaub.";

/// What the serializer writes for `INPUT`.
const OUTPUT: &[u8] = b"\x80\x04\x8c\x04tree\x8c\x04Node\x93)\x81\x94}(\x8c\x04name\x8c\x04root\x8c\x06parentN\x8c\x08children](\x8c\x04tree\x8c\x04Node\x93)\x81}(\x8c\x04name\x8c\x05child\x8c\x06parenth\x00\x8c\x08children](eubeub.";

#[test]
fn test_value_round_trip() {
    let value: Value = deser_pickle::from_slice(INPUT).unwrap();
    let output = deser_pickle::to_vec(&value).unwrap();
    assert_eq!(output, OUTPUT);
    // the same as read from the input
    let expected: Py = deser_pickle::from_slice(INPUT).unwrap();
    let again: Py = deser_pickle::from_slice(&output).unwrap();
    assert_eq!(again.normalized(), expected.normalized());
}

#[test]
fn test_recording_round_trip() {
    let recording: Recording = deser_pickle::from_slice(INPUT).unwrap();
    assert_eq!(deser_pickle::to_vec(&recording).unwrap(), OUTPUT);
}

#[test]
fn test_kinds_round_trip() {
    // `((1, 2), {3}, frozenset({4}), bytearray(b"a"))`
    let input = b"\x80\x05\x95 \x00\x00\x00\x00\x00\x00\x00(K\x01K\x02\x86\x94\x8f\x94(K\x03\x90(K\x04\x91\x94\x96\x01\x00\x00\x00\x00\x00\x00\x00a\x94t\x94.";
    let value: Value = deser_pickle::from_slice(input).unwrap();
    let output = deser_pickle::SerializerConfig::builder()
        .protocol(5)
        .build()
        .to_vec(&value)
        .unwrap();
    assert_eq!(
        output,
        b"\x80\x05((K\x01K\x02t\x8f(K\x03\x90(K\x04\x91\x96\x01\x00\x00\x00\x00\x00\x00\x00at."
    );
}

#[test]
fn test_to_json() {
    // classes are dropped and the cycle is null
    let value: Value = deser_pickle::from_slice(INPUT).unwrap();
    assert_eq!(
        deser_json::to_string(&value).unwrap(),
        r#"{"name":"root","parent":null,"children":[{"name":"child","parent":null,"children":[]}]}"#
    );
}

#[test]
fn test_from_json() {
    let value: Value = deser_json::from_str(r#"{"a": [1, 2.5, null], "7": {"b": true}}"#).unwrap();
    let output = deser_pickle::to_vec(&value).unwrap();
    assert_eq!(
        output,
        b"\x80\x04}(\x8c\x01a](K\x01G@\x04\x00\x00\x00\x00\x00\x00Ne\x8c\x017}(\x8c\x01b\x88uu."
    );
}
