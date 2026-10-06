//! Environment variables are not read from bytes, the input is split into
//! lines of `NAME=VALUE`.
#![no_main]

use deser::de::Recording;
use deser_env::{Case, DeserializerConfig, SerializerConfig};
use deser_fuzz::typed::Typed;
use deser_fuzz::{Input, context};
use deser_value::Value;
use libfuzzer_sys::fuzz_target;

const PREFIX: &str = "APP_";

fn separator(flags: u32) -> &'static str {
    ["__", "_", ".", "::"][(flags & 3) as usize]
}

fn case(flags: u32) -> Case {
    if flags & 4 != 0 {
        Case::Preserve
    } else {
        Case::Upper
    }
}

fn config(flags: u32, context: deser::Context) -> DeserializerConfig {
    DeserializerConfig::builder()
        .separator(separator(flags))
        .case(case(flags))
        .max_depth([16, 0, 1, 1000][(flags >> 3 & 3) as usize])
        .context(context)
        .build()
}

fn vars(data: &[u8]) -> Vec<(&str, &str)> {
    data.split(|&b| b == b'\n')
        .filter_map(|line| std::str::from_utf8(line).ok())
        .map(|line| line.split_once('=').unwrap_or((line, "")))
        .collect()
}

/// Serializes a value, the variables are sorted by name (like the
/// deserializer sorts them).
fn to_vars(ser: &SerializerConfig, value: &Value) -> Result<Vec<(String, String)>, deser::Error> {
    let mut vars = ser.to_vars(PREFIX, value)?;
    vars.sort();
    Ok(vars)
}

fn from_vars(de: &DeserializerConfig, vars: &[(String, String)]) -> Value {
    de.from_vars(PREFIX, vars.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .unwrap_or_else(|err| panic!("cannot deserialize the variables: {err}\nvars: {vars:?}"))
}

/// Checks that variables can be deserialized again and that values
/// survive round trips (like `deser_fuzz::check_roundtrip`).
fn check_roundtrip(ser_flags: u32, value: &Value) {
    let ser = SerializerConfig::builder()
        .separator(separator(ser_flags))
        .case(case(ser_flags))
        .build();
    // the serializer does not limit the depth of values
    let de = DeserializerConfig::builder()
        .separator(separator(ser_flags))
        .case(case(ser_flags))
        .max_depth(1_000_000)
        .build();
    let Ok(vars) = to_vars(&ser, value) else {
        return;
    };
    let first = from_vars(&de, &vars);
    // values with names that contain the separator cannot be serialized
    let Ok(vars) = to_vars(&ser, &first) else {
        return;
    };
    let second = from_vars(&de, &vars);
    assert_eq!(
        first, second,
        "the value changed after a round trip\nvars: {vars:?}"
    );
    let vars2 = to_vars(&ser, &second).unwrap_or_else(|err| {
        panic!("cannot serialize a value deserialized from variables: {err}\nvars: {vars:?}")
    });
    assert_eq!(vars, vars2, "the variables changed after a round trip");
}

fuzz_target!(|data: &[u8]| {
    let Some(input) = Input::parse(data) else {
        return;
    };
    let vars = vars(input.data);
    let config = config(input.flags, context(input.flags));
    let ser = SerializerConfig::builder()
        .separator(separator(input.ser_flags))
        .case(case(input.ser_flags))
        .build();

    if let Ok(typed) = config.from_vars::<Typed, _, _, _>(PREFIX, vars.iter().copied()) {
        let _ = ser.to_vars(PREFIX, &typed);
    }
    if let Ok(recording) = config.from_vars::<Recording, _, _, _>(PREFIX, vars.iter().copied()) {
        let _ = ser.to_vars(PREFIX, &recording);
    }
    if let Ok(value) = config.from_vars::<Value, _, _, _>(PREFIX, vars.iter().copied()) {
        check_roundtrip(input.ser_flags, &value);
    }
});
