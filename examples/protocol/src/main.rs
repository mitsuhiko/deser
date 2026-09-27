//! A wire protocol with integer tags, versioning and lossless forwarding.
//!
//! This is a relay of a chat protocol.  It shows:
//!
//! * variants named by integers (`"op": 1`) instead of strings, and enums
//!   named by their discriminants with `repr`,
//! * `tag_alias` to accept the tag under the name older clients used,
//! * an `other` variant that keeps the tag of unknown messages together
//!   with their content (a `Recording`), so that messages of newer clients
//!   are forwarded unchanged instead of being dropped,
//! * messages that borrow their strings from the input without copying.
//!
//! The same types work with JSON and CBOR.
use std::borrow::Cow;

use deser::adapters::Borrowed;
use deser::de::Recording;
use deser::{Deserialize, Serialize};

/// A message of the protocol, tagged by the integer `op`.
#[derive(Debug, Serialize, Deserialize)]
#[deser(tag = "op", tag_alias = "type")]
pub enum Message<'a> {
    #[deser(rename = 1)]
    Hello {
        client: &'a str,
        #[deser(default = 1)]
        version: u32,
    },
    #[deser(rename = 2)]
    Say {
        room: &'a str,
        // borrowed unless the string had escape sequences
        #[deser(as = Borrowed)]
        text: Cow<'a, str>,
    },
    #[deser(rename = 3)]
    Error { code: ErrorCode },
    /// Messages this relay does not know (yet).  The op code and the
    /// content are kept, they are serialized as they came in.
    #[deser(other)]
    Unknown(#[deser(tag)] u64, Recording),
}

/// Error codes are written as their numbers.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(repr)]
#[repr(u16)]
pub enum ErrorCode {
    BadRequest = 400,
    Forbidden = 403,
    RateLimited = 429,
}

/// Returns `true` if `part` points into `whole`.
fn is_within(part: &str, whole: &str) -> bool {
    whole.as_bytes().as_ptr_range().contains(&part.as_ptr())
}

/// Receives a message and forwards it.
fn relay(input: &str) -> String {
    let message: Message = deser_json::from_str(input).unwrap();
    match message {
        Message::Say { room, ref text } => {
            println!("received: {:?}", message);
            // nothing was copied
            assert!(is_within(room, input));
            assert!(matches!(text, Cow::Borrowed(_)));
        }
        Message::Unknown(op, ref content) => {
            // a recording can be serialized like any other value
            let content = deser_json::to_string(content).unwrap();
            println!("received: unknown op {} with {}", op, content);
        }
        _ => println!("received: {:?}", message),
    }
    let output = deser_json::to_string(&message).unwrap();
    println!("sent:     {}\n", output);
    output
}

fn main() {
    relay(r#"{"op": 1, "client": "cli"}"#);
    relay(r#"{"op": 2, "room": "general", "text": "hi everyone"}"#);
    assert_eq!(relay(r#"{"op": 3, "code": 429}"#), r#"{"op":3,"code":429}"#);

    // old clients sent the op as `type`
    let output = relay(r#"{"type": 1, "client": "legacy", "version": 0}"#);
    assert_eq!(output, r#"{"op":1,"client":"legacy","version":0}"#);

    // a message of a newer client goes through unchanged, including values
    // that this relay knows nothing about
    let newer = r#"{"op":7,"room":"general","reaction":{"emoji":"🎉","count":3}}"#;
    assert_eq!(relay(newer), newer);

    // unknown error codes are rejected with the known ones
    let err = deser_json::from_str::<Message>(r#"{"op": 3, "code": 500}"#).unwrap_err();
    println!("error: {}", err);
    assert!(err.message().contains("unknown variant `500` of ErrorCode"));

    // CBOR works the same, with integers as integers on the wire
    let message = Message::Error {
        code: ErrorCode::Forbidden,
    };
    let cbor = deser_cbor::to_vec(&message).unwrap();
    let back: Message = deser_cbor::from_slice(&cbor).unwrap();
    println!("CBOR: {} bytes, {:?}", cbor.len(), back);
    assert!(matches!(
        back,
        Message::Error {
            code: ErrorCode::Forbidden
        }
    ));
}
