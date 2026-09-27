//! `Serialize` and `Deserialize` for the collections and byte buffers of
//! other crates.
//!
//! Every crate has a feature of the same name.  The types behave like their
//! counterparts in the standard library, including the container adapters
//! (for instance `IndexMap<KA, VA>` is an adapter for `IndexMap<K, V>`):
//!
//! * `indexmap`: `IndexMap` and `IndexSet` like `HashMap` and `HashSet`,
//!   except that their entries are emitted in their order.
//! * `hashbrown`: `HashMap` and `HashSet` like the ones of the standard
//!   library.
//! * `smallvec`: `SmallVec` like `Vec`.
//! * `arrayvec`: `ArrayVec` like `Vec` (more elements than its capacity
//!   are an error) and `ArrayString` like `String`.
//! * `bytes`: `Bytes` and `BytesMut` like `Vec<u8>`.
//! * `bstr`: `BString`, `Box<BStr>` and `&BStr` as strings if they are
//!   valid UTF-8 and as bytes otherwise.  Strings are deserialized as their
//!   UTF-8 bytes, bytes and sequences of integers are accepted too.
//!
//! The byte buffers (and `SmallVec` and `ArrayVec` of `u8`) also support
//! the bytes adapters (see [`BytesBuf`](crate::adapters::BytesBuf)), which
//! represent the raw bytes.

#[cfg(feature = "arrayvec")]
mod arrayvec;
#[cfg(feature = "bstr")]
mod bstr;
#[cfg(feature = "bytes")]
mod bytes;
#[cfg(feature = "hashbrown")]
mod hashbrown;
#[cfg(feature = "indexmap")]
mod indexmap;
#[cfg(feature = "smallvec")]
mod smallvec;
