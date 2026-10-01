//! Reading and writing streams of values without doing IO.
//!
//! Data formats parse complete inputs (slices) and serialize into complete
//! outputs.  For streams (such as files, sockets or pipes) the formats
//! have stream serializers and stream deserializers which do not do IO
//! themselves (sans-io).  This makes them usable with any kind of IO, and
//! without the standard library:
//!
//! * A [`StreamSerializer`](crate::ser::StreamSerializer) (the serializer
//!   of a format, for instance `deser_json::Serializer`) holds the state
//!   of a stream and its output.  Whoever writes the output to the stream
//!   takes it and clears it.  Large values can be serialized in parts (see
//!   [`drive_partial`](crate::ser::StreamSerializer::drive_partial)), so
//!   the memory used does not depend on their size.
//! * A [`StreamDeserializer`](crate::de::StreamDeserializer) (for instance
//!   `deser_json::StreamDeserializer`) splits the input of a stream into
//!   values or deserializes them while their input arrives.  The
//!   [`InputBuffer`] of this module holds the input that was read and
//!   invokes the stream deserializer.
//!
//! The readers and writers of `deser::io` (which need the `io` feature)
//! connect them to [`std::io`](https://doc.rust-lang.org/std/io/), and
//! crates like `deser-tokio` to other kinds of IO.
//!
//! # Frames and Partial Deserialization
//!
//! A stream deserializer splits the input into frames: it finds the bytes
//! of the next value in the input that was read so far (see
//! [`StreamDeserializer::frame`](crate::de::StreamDeserializer::frame)),
//! for instance a line with JSON Lines.  Once a value is complete it's
//! deserialized from its frame with the format's regular parser.  Types
//! can borrow from the frame (see [`InputBuffer::deserialize`]).
//!
//! Formats which can be parsed while the input arrives (like JSON and CBOR)
//! can also deserialize values while the input is fed to them (see
//! [`InputBuffer::drive_partial`]): the parts of a value are deserialized
//! as they are read and only incomplete tokens are buffered, which means
//! that the memory used does not depend on the size of the values.
//!
//! # Large Sequences
//!
//! Values which contain a large (or unbounded) sequence can be processed
//! while they are read: a [`Streamed`] sequence hands out
//! its elements as they are read with an [`ElementReader`] (and behaves
//! like a `Vec` otherwise).
//!
//! # Errors
//!
//! Errors refer to positions in the stream: the offsets, lines and columns
//! of errors are relative to the start of the stream, not to the start of
//! the frame.
//!
//! The input ranges formats publish into the [`State`](crate::State) (and
//! the locations derived from them, for instance by `deser-location`)
//! refer to the frame of the value.
mod buffer;
pub(crate) mod elements;
mod streamed;

pub use self::buffer::{InputBuffer, Status};
pub use self::elements::{ElementReader, ElementStatus, Part};
pub use self::streamed::Streamed;

/// The default for how much output of a value writers buffer before it's
/// written (see `Writer::set_buffer_limit` of `deser::io`).
///
/// Writers pass this as the limit to
/// [`StreamSerializer::drive_partial`](crate::ser::StreamSerializer::drive_partial).
pub const DEFAULT_BUFFER_LIMIT: usize = 8 * 1024;
