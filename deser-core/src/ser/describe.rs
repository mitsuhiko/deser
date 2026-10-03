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
/// use deser::ser::{Describe, SerializeRef};
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
/// SerializeRef::new(&Some(Some(42))).describe(&mut names);
/// assert_eq!(names.0, ["some", "some"]);
/// ```
///
/// The names passed to the describer are the names used when serializing
/// (after renames), variants that are named by integers or booleans are
/// described with their name as text.
///
/// All methods ignore the call by default so that describers only need to
/// implement what they care about.  New methods can be added in the future.
///
/// Types describe themselves with
/// [`Serialize::describe`](crate::ser::Serialize::describe) (a value) and
/// [`Deserialize::describe_type`](crate::de::Deserialize::describe_type)
/// (what is known without a value).  Descriptions are mostly informational
/// (for formats like a `Debug`-like formatter), but some facts change how
/// values are represented, so the descriptions have to be accurate:
///
/// * [`unit_struct`](Self::unit_struct): newtype variants of internally
///   tagged enums whose content is a unit struct are the tag alone.  The
///   serializer checks the description of the value, the deserializer the
///   one of the type, so both have to describe the unit struct.
pub trait Describe {
    /// The value is a struct with named fields.
    ///
    /// It's serialized as map with the names of the fields as keys.
    fn structure(&mut self, name: &str) {
        let _ = name;
    }

    /// The names of the fields of the struct that was just described with
    /// [`structure`](Self::structure), in the order they are serialized.
    ///
    /// The keys of the map the struct is serialized as are a subsequence
    /// of these names: fields can be skipped but no other keys are
    /// emitted.  This lets formats know which keys can still come, for
    /// instance to write parts of the output before the struct ends.
    /// Structs whose keys are not known upfront (like derived structs with
    /// flattened fields) do not describe their fields.
    ///
    /// ```
    /// use deser::ser::Describe;
    ///
    /// #[derive(deser::Serialize)]
    /// struct Link {
    ///     #[deser(rename = "@href")]
    ///     href: String,
    ///     #[deser(skip_serializing)]
    ///     cache: u32,
    ///     title: String,
    /// }
    ///
    /// #[derive(Default)]
    /// struct Fields(&'static [&'static str]);
    ///
    /// impl Describe for Fields {
    ///     fn fields(&mut self, names: &'static [&'static str]) {
    ///         self.0 = names;
    ///     }
    /// }
    ///
    /// let mut fields = Fields::default();
    /// let link = Link { href: "/".into(), cache: 0, title: "x".into() };
    /// deser::ser::SerializeRef::new(&link).describe(&mut fields);
    /// assert_eq!(fields.0, ["@href", "title"]);
    /// ```
    fn fields(&mut self, names: &'static [&'static str]) {
        let _ = names;
    }

    /// The value is a newtype struct.
    ///
    /// It's serialized as the value it wraps, the description of that value
    /// follows.
    fn newtype(&mut self, name: &str) {
        let _ = name;
    }

    /// The value is a tuple struct (a struct with more than one unnamed
    /// field).
    ///
    /// It's serialized as sequence.
    fn tuple_struct(&mut self, name: &str) {
        let _ = name;
    }

    /// The value is a unit struct (a struct without fields).
    ///
    /// It's serialized as null and deserialized from null.  This is not
    /// only informational: newtype variants of internally tagged enums with
    /// a unit struct as content are the tag alone (`{"type": "A"}`), other
    /// content that is null is not.  Only unit
    /// structs themselves and wrappers that are serialized and deserialized
    /// as the value they wrap (like `Box`) describe this, not newtypes or
    /// options of unit structs.
    fn unit_struct(&mut self, name: &str) {
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

/// Returns `true` if a description is the one of a unit struct.
///
/// The description is given by a function that describes a value or type
/// (like `|d| value.describe(d)` or `T::describe_type`).  It's the
/// description of a unit struct if it says
/// [`unit_struct`](Describe::unit_struct) and does not wrap it (newtypes
/// and options of unit structs are not unit structs).  The serializer and
/// the deserializer decide with this whether newtype variants of internally
/// tagged enums are the tag alone, so they agree for all types whose
/// descriptions agree.
pub(crate) fn is_unit_struct(describe: impl FnOnce(&mut dyn Describe)) -> bool {
    #[derive(Default)]
    struct IsUnitStruct {
        unit_struct: bool,
        wrapped: bool,
    }

    impl Describe for IsUnitStruct {
        fn unit_struct(&mut self, _name: &str) {
            self.unit_struct = true;
        }

        fn newtype(&mut self, _name: &str) {
            self.wrapped = true;
        }

        fn some(&mut self) {
            self.wrapped = true;
        }

        fn none(&mut self) {
            self.wrapped = true;
        }
    }

    let mut d = IsUnitStruct::default();
    describe(&mut d);
    d.unit_struct && !d.wrapped
}
