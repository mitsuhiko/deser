//! The formats and the libraries that implement them.
//!
//! Every format is implemented by a deser crate and by a serde library:
//!
//! | format | deser        | serde          |
//! |--------|--------------|----------------|
//! | JSON   | `deser-json` | `serde_json`   |
//! | CBOR   | `deser-cbor` | `ciborium`     |
//! | YAML   | `deser-yaml` | `serde-saphyr` |
//! | TOML   | `deser-toml` | `toml`         |
use deser::{Deserialize, Serialize};
use serde::de::DeserializeOwned;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Cbor,
    Yaml,
    Toml,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Json, Format::Cbor, Format::Yaml, Format::Toml];

    pub fn name(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Cbor => "cbor",
            Format::Yaml => "yaml",
            Format::Toml => "toml",
        }
    }
}

/// A serialized value.  Text formats are deserialized from a `&str` (which
/// is the common API and skips the UTF-8 validation).
#[derive(Clone, Copy)]
pub enum Input<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
}

impl<'a> Input<'a> {
    pub fn new(format: Format, bytes: &'a [u8]) -> Input<'a> {
        match format {
            Format::Cbor => Input::Binary(bytes),
            _ => Input::Text(std::str::from_utf8(bytes).expect("text format is not UTF-8")),
        }
    }

    fn text(self) -> &'a str {
        match self {
            Input::Text(text) => text,
            Input::Binary(_) => unreachable!("binary input for a text format"),
        }
    }

    fn bytes(self) -> &'a [u8] {
        match self {
            Input::Text(text) => text.as_bytes(),
            Input::Binary(bytes) => bytes,
        }
    }
}

pub type Error = Box<dyn std::error::Error>;

/// Serializes with the deser crate of the format.
pub fn deser_ser<T: Serialize>(format: Format, value: &T) -> Result<Vec<u8>, Error> {
    Ok(match format {
        Format::Json => deser_json::to_string(value)?.into_bytes(),
        Format::Cbor => deser_cbor::to_vec(value)?,
        Format::Yaml => deser_yaml::to_string(value)?.into_bytes(),
        Format::Toml => deser_toml::to_string(value)?.into_bytes(),
    })
}

/// Deserializes with the deser crate of the format.
pub fn deser_de<T: for<'de> Deserialize<'de>>(format: Format, input: Input) -> Result<T, Error> {
    Ok(match format {
        Format::Json => deser_json::from_str(input.text())?,
        Format::Cbor => deser_cbor::from_slice(input.bytes())?,
        Format::Yaml => deser_yaml::from_str(input.text())?,
        Format::Toml => deser_toml::from_str(input.text())?,
    })
}

/// Serializes with the serde library of the format.
pub fn serde_ser<T: serde::Serialize>(format: Format, value: &T) -> Result<Vec<u8>, Error> {
    Ok(match format {
        Format::Json => serde_json::to_string(value)?.into_bytes(),
        Format::Cbor => {
            let mut out = Vec::new();
            ciborium::into_writer(value, &mut out)?;
            out
        }
        Format::Yaml => serde_saphyr::to_string(value)?.into_bytes(),
        Format::Toml => toml::to_string(value)?.into_bytes(),
    })
}

/// Deserializes with the serde library of the format.
pub fn serde_de<T: DeserializeOwned>(format: Format, input: Input) -> Result<T, Error> {
    Ok(match format {
        Format::Json => serde_json::from_str(input.text())?,
        Format::Cbor => ciborium::from_reader(input.bytes())?,
        // The default budget of serde-saphyr rejects the larger documents
        // (such as canada with more than 250,000 nodes).  deser-yaml does
        // not limit the input by default, so neither does serde-saphyr.
        Format::Yaml => serde_saphyr::from_str_with_options(
            input.text(),
            serde_saphyr::options! { budget: None },
        )?,
        Format::Toml => toml::from_str(input.text())?,
    })
}
