//! Parse and serialize YAML compatible with deser.
//!
//! **This crate is work in progress and cannot parse YAML yet.**
//!
//! The implementation is layered like the processing model of the YAML
//! specification:
//!
//! 1. the parser turns text into YAML events (checked against the official
//!    [YAML test suite](https://github.com/yaml/yaml-test-suite)),
//! 2. the composer resolves aliases and tags,
//! 3. the schema resolves plain scalars to typed values (failsafe, JSON,
//!    core or YAML 1.1),
//! 4. the result is fed into deser.
//!
//! Only the last layer is public.  A YAML stream with multiple documents is
//! read like a CBOR sequence in `deser-cbor`: every document is one item.
// TODO: remove once the parser produces all events
#[allow(dead_code)]
mod event;
mod parser;

#[doc(hidden)]
#[path = "private.rs"]
pub mod __private;
