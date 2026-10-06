use alloc::borrow::Cow;

use deser_core::State;
use deser_core::de::{Deserialize, Slot, default_atom};
use deser_core::ext::{ExtValue, Extension};
use deser_core::ser::{Emit, Serialize};
use deser_core::{Atom, Error};

/// The kind of a [`Reference`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReferenceKind {
    /// The same object again (`r:`).
    ///
    /// PHP writes this when an object appears more than once.  It can
    /// only refer to objects.
    Object,
    /// A PHP reference (`R:`), a value that was assigned by reference
    /// (`$b = &$a`).
    ///
    /// It can refer to any value.
    Value,
}

/// A reference to another value of the input (`r:` and `R:`).
///
/// PHP numbers the values of the input in the order they appear (starting
/// with 1 for the top-level value) and a reference repeats the value of a
/// number.  The deserializer does not resolve references: it passes them
/// on as extension atoms of this type, the serializer writes them as they
/// are.  Like when deserializing, references have to refer to a value
/// before them (`r:` to an object), otherwise they are an error.
///
/// **This is basically a marker only.**  The number refers to a position
/// in the whole input which the value that holds the reference cannot
/// know, and what it refers to is gone once the input was deserialized:
/// the values that the types skipped have a number too, and the numbering
/// rules (keys and `R:` have no number, `r:` has one) are those of PHP.
/// Short of reimplementing the deserializer there is no way to look up the
/// value.  What this type is good for is to detect references and to
/// write the input back unchanged:
///
/// ```
/// use deser_php::{Reference, ReferenceKind};
///
/// // `[$a, &$a]` with `$a = 5`, the second entry refers to the first
/// let input = b"a:2:{i:0;i:5;i:1;R:2;}";
/// let value: (u32, Reference) = deser_php::from_slice(input).unwrap();
/// assert_eq!(value.1, Reference::new(ReferenceKind::Value, 2));
/// assert_eq!(deser_php::to_vec(&value).unwrap(), input);
///
/// // the fallback is the number: this is not the value it refers to
/// let value: Vec<u32> = deser_php::from_slice(input).unwrap();
/// assert_eq!(value, [5, 2]);
/// ```
///
/// The fallback of the extension is the number, other types than this one
/// receive an integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reference {
    kind: ReferenceKind,
    number: u64,
}

impl Reference {
    /// Creates a reference.
    pub const fn new(kind: ReferenceKind, number: u64) -> Reference {
        Reference { kind, number }
    }

    /// Returns the kind of the reference.
    pub const fn kind(self) -> ReferenceKind {
        self.kind
    }

    /// Returns the number of the value it refers to.
    ///
    /// The top-level value is 1.
    pub const fn number(self) -> u64 {
        self.number
    }
}

impl Extension for Reference {
    fn name(&self) -> &str {
        "php reference"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::U64(self.number)
    }
}

impl Serialize for Reference {
    fn serialize<'a>(value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::Atom(Atom::Ext(ExtValue::borrowed(value))))
    }
}

impl<'de> Deserialize<'de> for Reference {
    fn deserialize_atom(slot: &mut Slot<Self>, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Ext(ref ext) => match ext.downcast_ref::<Reference>() {
                Some(&value) => {
                    slot.set(value);
                    Ok(())
                }
                None => default_atom(slot, atom, state),
            },
            other => default_atom(slot, other, state),
        }
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("php reference")
    }
}
