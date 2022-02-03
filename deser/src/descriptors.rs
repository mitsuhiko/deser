use std::fmt::Debug;

/// The default null descriptor.
#[derive(Debug)]
pub(crate) struct NullDescriptor;

/// A primitive descriptor with just a name.
#[derive(Debug)]
pub(crate) struct NamedDescriptor {
    pub(crate) name: &'static str,
}

/// A number descriptor provides additional information about a number type.
#[derive(Debug)]
pub(crate) struct NumberDescriptor {
    pub(crate) name: &'static str,
    pub(crate) precision: usize,
}

/// A descriptor that is always unordered.
#[derive(Debug)]
pub(crate) struct UnorderedNamedDescriptor {
    pub(crate) name: &'static str,
}

/// A descriptor provides auxiliary type information.
///
/// Many types upon serialization coerce their value into a common atomic
/// value which is native to the `deser` data model.  This causes challenges
/// when a serializer needs to tell the difference between the original values.
/// For instance a serializer might be interested in being able to tell a
/// `u8` from a `u64` despite the fact that both are represented equally.
///
/// During serialization descriptors are generally created, for the deserialization
/// system descriptors are only used when entering into a nested structure
/// such as a map, struct or sequence.
pub trait Descriptor: Debug {
    /// Returns a descriptive name for a type if such a name is available.
    fn name(&self) -> Option<&str> {
        None
    }

    /// Returns the precision in bits of the value.
    ///
    /// This is normally set for numbers and returns the natural bit count of
    /// the source information.  For instancen a `u32` will return `Some(32)`
    /// from this method.
    fn precision(&self) -> Option<usize> {
        None
    }

    /// Returns information about this value's ordering characteristic.
    ///
    /// Things that are naturally unordered return `true` here.  For instance
    /// a `HashSet` returns `true` here.
    fn unordered(&self) -> bool {
        false
    }
}

impl Descriptor for NullDescriptor {}

impl Descriptor for NamedDescriptor {
    fn name(&self) -> Option<&str> {
        Some(self.name)
    }
}

impl Descriptor for NumberDescriptor {
    fn name(&self) -> Option<&str> {
        Some(self.name)
    }

    fn precision(&self) -> Option<usize> {
        if self.precision > 0 {
            Some(self.precision)
        } else {
            None
        }
    }
}

impl Descriptor for UnorderedNamedDescriptor {
    fn name(&self) -> Option<&str> {
        Some(self.name)
    }

    fn unordered(&self) -> bool {
        true
    }
}
