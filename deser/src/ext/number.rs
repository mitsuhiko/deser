use std::borrow::Cow;
use std::fmt;

use crate::State;
use crate::de::{Deserialize, Sink, SinkHandle};
use crate::descriptors::{Descriptor, NamedDescriptor};
use crate::error::Error;
use crate::event::Atom;
use crate::ext::known::invalid;
use crate::ext::{BorrowedExtension, ExtValue};
use crate::ser::{Chunk, Serialize};

/// A number literal from a text format.
///
/// This is a well-known borrowing extension (see [`ext`](crate::ext)) for
/// numbers that text formats parsed as floats (or integers too large for
/// the data model).  It holds the text of the literal in the syntax of JSON
/// numbers together with its value as `f64`.  The fallback is the `f64`, so
/// consumers that do not know about exact numbers are not affected.
/// Consumers that need the exact value (like [`Decimal`](crate::ext::Decimal)
/// and [`BigInt`](crate::ext::BigInt) and the types of `rust_decimal`,
/// `bigdecimal` and `num-bigint`) use the text.
///
/// ```
/// use deser::ext::{Decimal, Number};
///
/// let number = Number::parse("0.10").unwrap();
/// assert_eq!(number.as_str(), "0.10");
/// assert_eq!(number.value(), 0.1);
/// ```
///
/// The text borrows from the input for formats that support it, extension
/// values of it are created with
/// [`ExtValue::borrowed_value::<Number>`](ExtValue::borrowed_value) and
/// looked up with
/// [`ExtValue::downcast_value_ref::<Number>`](ExtValue::downcast_value_ref).
#[derive(Clone, PartialEq)]
pub struct Number<'a> {
    text: Cow<'a, str>,
    value: f64,
}

impl<'a> Number<'a> {
    /// Creates a number from its text and value.
    ///
    /// The text has to follow the syntax of JSON numbers and the value has
    /// to be the (possibly approximated) value of the text.  This is not
    /// checked in release builds.  Use [`parse`](Self::parse) to validate the
    /// text and compute the value.
    pub fn new<T: Into<Cow<'a, str>>>(text: T, value: f64) -> Number<'a> {
        let text = text.into();
        debug_assert!(is_json_number(&text), "invalid number text {:?}", text);
        Number { text, value }
    }

    /// Parses a number from its text.
    ///
    /// The text has to follow the syntax of JSON numbers.
    pub fn parse<T: Into<Cow<'a, str>>>(text: T) -> Result<Number<'a>, Error> {
        let text = text.into();
        if !is_json_number(&text) {
            return Err(invalid("invalid number"));
        }
        let value = text.parse().map_err(|_| invalid("invalid number"))?;
        Ok(Number { text, value })
    }

    /// Returns the text of the number.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Returns the value of the number as `f64`.
    ///
    /// This might be an approximation of the text.
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Returns `true` if the text is an integer (no fraction or exponent).
    pub fn is_integer(&self) -> bool {
        !self.text.contains(['.', 'e', 'E'])
    }

    /// Detaches the number from the data it borrows.
    pub fn into_static(self) -> Number<'static> {
        Number {
            text: Cow::Owned(self.text.into_owned()),
            value: self.value,
        }
    }
}

/// Checks the syntax of JSON numbers.
pub(crate) fn is_json_number(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut pos = 0;
    let digits = |pos: &mut usize| {
        let start = *pos;
        while bytes.get(*pos).is_some_and(u8::is_ascii_digit) {
            *pos += 1;
        }
        *pos - start
    };
    if bytes.first() == Some(&b'-') {
        pos += 1;
    }
    let int_start = pos;
    let int_len = digits(&mut pos);
    if int_len == 0 || (int_len > 1 && bytes[int_start] == b'0') {
        return false;
    }
    if bytes.get(pos) == Some(&b'.') {
        pos += 1;
        if digits(&mut pos) == 0 {
            return false;
        }
    }
    if let Some(b'e' | b'E') = bytes.get(pos) {
        pos += 1;
        if let Some(b'+' | b'-') = bytes.get(pos) {
            pos += 1;
        }
        if digits(&mut pos) == 0 {
            return false;
        }
    }
    pos == bytes.len()
}

impl<'a> fmt::Display for Number<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl<'a> fmt::Debug for Number<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Number({})", self.text)
    }
}

impl BorrowedExtension for Number<'static> {
    type Value<'a> = Number<'a>;

    fn name<'v>(_value: &'v Number<'_>) -> &'v str {
        "number"
    }

    fn fallback<'v>(value: &'v Number<'_>) -> Atom<'v> {
        Atom::F64(value.value)
    }

    fn to_static(value: &Number<'_>) -> Number<'static> {
        value.clone().into_static()
    }

    fn shorten<'s, 'l: 's>(value: &'s Number<'l>) -> &'s Number<'s> {
        value
    }
}

static NUMBER_DESCRIPTOR: NamedDescriptor = NamedDescriptor { name: "Number" };

impl<'a> Serialize for Number<'a> {
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed_value::<Number>(
            self,
        ))))
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        &NUMBER_DESCRIPTOR
    }
}

/// Numbers are deserialized from numbers (and number extension values) and
/// strings with the syntax of JSON numbers.  The text is always owned.
impl<'de, 'a> Deserialize<'de> for Number<'a> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(NumberSink(out))
    }
}

struct NumberSink<'a, 'n>(&'a mut Option<Number<'n>>);

impl<'a, 'n, 'de> Sink<'de> for NumberSink<'a, 'n> {
    fn descriptor(&self) -> &'static dyn Descriptor {
        &NUMBER_DESCRIPTOR
    }

    fn expecting(&self) -> Cow<'_, str> {
        "number".into()
    }

    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        let number = match atom {
            Atom::Ext(ref ext) => match ext.downcast_value_ref::<Number>() {
                Some(value) => value.clone().into_static(),
                None => return self.unexpected_atom(atom, state),
            },
            Atom::U64(value) => Number::new(value.to_string(), value as f64),
            Atom::I64(value) => Number::new(value.to_string(), value as f64),
            Atom::F64(value) if value.is_finite() => Number::new(format!("{:?}", value), value),
            Atom::Str(ref value) => Number::parse(value.to_string())?,
            other => return self.unexpected_atom(other, state),
        };
        *self.0 = Some(number);
        Ok(())
    }
}

#[test]
fn test_number() {
    let number = Number::parse("-12.50e3").unwrap();
    assert_eq!(number.as_str(), "-12.50e3");
    assert_eq!(number.value(), -12500.0);
    assert!(!number.is_integer());
    assert!(
        Number::parse("12345678901234567890123456789")
            .unwrap()
            .is_integer()
    );
    for invalid in ["", "1.", ".1", "+1", "01", "1e", "NaN", "inf", "0x10"] {
        assert!(Number::parse(invalid).is_err(), "{}", invalid);
    }

    let ext = ExtValue::borrowed_value::<Number>(&number);
    assert_eq!(ext.fallback(), Atom::F64(-12500.0));
    assert_eq!(ext.name(), "number");
    assert_eq!(
        ext.downcast_value_ref::<Number>().unwrap().as_str(),
        "-12.50e3"
    );
}
