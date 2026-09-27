//! The messages of a sensor that reports its readings as CBOR.
//!
//! This library does not use the standard library, it only needs `alloc`.
//! It builds for targets without an operating system, such as
//! `thumbv7em-none-eabihf`.
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use deser::de::Recording;
use deser::{Deserialize, Error, Serialize};

/// A message the sensor sends.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "type", rename_all = "snake_case")]
pub enum Message<'a> {
    Reading(Reading<'a>),
    Status {
        uptime: u64,
        #[deser(default)]
        errors: Vec<String>,
    },
    /// Messages of newer firmware are kept and can be forwarded.
    #[deser(other)]
    Unknown(#[deser(tag)] String, Recording),
}

/// A reading of the sensor.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
pub struct Reading<'a> {
    pub sensor_id: u32,
    /// Borrowed from the input.
    pub unit: &'a str,
    pub values: Vec<f32>,
    #[deser(default = 1.0)]
    pub scale: f32,
    /// Raw register contents, bytes in CBOR.
    pub raw: [u8; 4],
}

/// Encodes a message as CBOR.
pub fn encode(message: &Message<'_>) -> Result<Vec<u8>, Error> {
    deser_cbor::to_vec(message)
}

/// Decodes a message from CBOR.
pub fn decode(bytes: &[u8]) -> Result<Message<'_>, Error> {
    deser_cbor::from_slice(bytes)
}

/// Parses a message from JSON (for instance from a configuration tool).
pub fn from_json(json: &str) -> Result<Message<'_>, Error> {
    deser_json::from_str(json)
}

/// Writes a message as JSON.
pub fn to_json(message: &Message<'_>) -> Result<String, Error> {
    deser_json::to_string(message)
}
