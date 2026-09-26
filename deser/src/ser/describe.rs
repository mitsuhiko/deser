//! Describing the Rust shape of values.
/// Receives the description of the Rust shape of a value.
///
/// The data model of deser is small: a struct is a map with string keys, a
/// tuple is a sequence, `Some(42)` is just `42`.  Formats which want to
/// reflect the Rust shape of values (for instance a `Debug`-like formatter)
/// can ask a value to [`describe`](crate::ser::Serialize::describe) itself.
/// The description is pulled: values are only asked if a format wants to
/// know, which means that formats which do not care pay nothing for it.
///
/// The [`SerializeDriver`](crate::ser::SerializeDriver) passes the value an
/// event belongs to along with the event.  The value of the end event of a
/// map or sequence is the value that started it.  Keys of structs come with
/// a value that describes nothing (the description of the struct says that
/// the keys are field names).
///
/// Wrappers describe themselves and then delegate to the value they wrap,
/// which is how nested wrappers such as `Some(Meters(5.0))` are described:
/// the describer receives [`some`](Describe::some),
/// [`newtype`](Describe::newtype) and then nothing (as `f64` has no
/// description of its own).
///
/// ```
/// use deser::ser::{Describe, Serialize, SerializeDriver};
///
/// #[derive(Default)]
/// struct Names(Vec<String>);
///
/// impl Describe for Names {
///     fn structure(&mut self, name: &str) {
///         self.0.push(format!("struct {}", name));
///     }
///
///     fn some(&mut self) {
///         self.0.push("some".into());
///     }
/// }
///
/// let mut names = Names::default();
/// Some(Some(42)).describe(&mut names);
/// assert_eq!(names.0, ["some", "some"]);
/// ```
///
/// All methods ignore the call by default so that describers only need to
/// implement what they care about.  New methods can be added in the future.
pub trait Describe {
    /// The value is a struct with named fields.
    ///
    /// It's serialized as map with the names of the fields as keys.
    fn structure(&mut self, name: &str) {
        let _ = name;
    }

    /// The value is a newtype struct.
    ///
    /// It's serialized as the value it wraps, the description of that value
    /// follows.
    fn newtype(&mut self, name: &str) {
        let _ = name;
    }

    /// The value is a variant of an enum.
    ///
    /// How the variant is serialized depends on its [`VariantRepr`].  For
    /// externally tagged variants with content the value is a map with a
    /// single entry, the key is the name of the variant and the value the
    /// content.
    fn variant(&mut self, variant: &Variant<'_>) {
        let _ = variant;
    }

    /// The value is `Some` of an `Option`.
    ///
    /// It's serialized as the value it wraps, the description of that value
    /// follows.
    fn some(&mut self) {}

    /// The value is `None` of an `Option`.
    ///
    /// It's serialized as null.
    fn none(&mut self) {}

    /// The value is a tuple.
    ///
    /// It's serialized as sequence.
    fn tuple(&mut self) {}

    /// The value is a set.
    ///
    /// It's serialized as sequence.
    fn set(&mut self) {}
}

/// Describes a variant of an enum.
///
/// See [`Describe::variant`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Variant<'a> {
    /// The name of the enum.
    pub enum_name: &'a str,
    /// The name of the variant.
    pub name: &'a str,
    /// The kind of variant.
    pub kind: VariantKind,
    /// How the variant is represented.
    pub repr: VariantRepr<'a>,
}

impl<'a> Variant<'a> {
    /// Creates a variant description.
    pub const fn new(
        enum_name: &'a str,
        name: &'a str,
        kind: VariantKind,
        repr: VariantRepr<'a>,
    ) -> Variant<'a> {
        Variant {
            enum_name,
            name,
            kind,
            repr,
        }
    }
}

/// The kind of an enum variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VariantKind {
    /// A variant without content (`A`).
    Unit,
    /// A variant with a single unnamed field (`A(T)`).
    Newtype,
    /// A variant with multiple unnamed fields (`A(T, U)`).
    Tuple,
    /// A variant with named fields (`A { x: T }`).
    Struct,
}

/// How an enum variant is represented.
///
/// This corresponds to the representations of the derive (see
/// [`derive`](crate::derive)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VariantRepr<'a> {
    /// Externally tagged: unit variants are their name, others a map with
    /// the name as key and the content as value.
    External,
    /// Internally tagged: a map with the name in the given key and the
    /// fields of the content.
    Internal {
        /// The key of the name.
        tag: &'a str,
    },
    /// Adjacently tagged: a map with the name and the content in the given
    /// keys.
    Adjacent {
        /// The key of the name.
        tag: &'a str,
        /// The key of the content.
        content: &'a str,
    },
    /// Untagged: just the content.
    Untagged,
}
