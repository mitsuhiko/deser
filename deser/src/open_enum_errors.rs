//! Compile tests for open enums that are rejected.  These are only
//! compiled as doctests.

/// Open enums need `Send` and `Sync` as supertraits.
///
/// ```compile_fail
/// #[deser::open_enum]
/// trait Shape {}
/// ```
///
/// Open enums cannot have generic parameters.
///
/// ```compile_fail
/// #[deser::open_enum]
/// trait Shape<T>: Send + Sync {}
/// ```
///
/// Untagged open enums have no tag.
///
/// ```compile_fail
/// #[deser::open_enum(untagged, tag = "type")]
/// trait Shape: Send + Sync {}
/// ```
///
/// `content` requires `tag`.
///
/// ```compile_fail
/// #[deser::open_enum(content = "c")]
/// trait Shape: Send + Sync {}
/// ```
///
/// Every implementation needs `#[deser::variant]`.
///
/// ```compile_fail,E0046
/// #[deser::open_enum]
/// trait Shape: Send + Sync {}
///
/// #[derive(deser::Serialize, deser::Deserialize)]
/// struct Circle;
///
/// impl Shape for Circle {}
/// ```
///
/// Implementations cannot be generic.
///
/// ```compile_fail
/// #[deser::open_enum]
/// trait Shape: Send + Sync {}
///
/// #[derive(deser::Serialize, deser::Deserialize)]
/// struct Wrapper<T>(T);
///
/// #[deser::variant]
/// impl<T: deser::Serialize + Send + Sync + 'static> Shape for Wrapper<T> {}
/// ```
///
/// Types with generic arguments need a name.
///
/// ```compile_fail
/// #[deser::open_enum]
/// trait Shape: Send + Sync {}
///
/// #[derive(deser::Serialize, deser::Deserialize)]
/// struct Wrapper<T>(T);
///
/// #[deser::variant]
/// impl Shape for Wrapper<u32> {}
/// ```
///
/// Variants cannot borrow.
///
/// ```compile_fail
/// #[deser::open_enum]
/// trait Shape: Send + Sync {}
///
/// #[derive(deser::Serialize, deser::Deserialize)]
/// struct Name<'a>(&'a str);
///
/// #[deser::variant(rename = "name")]
/// impl Shape for Name<'static> {}
/// ```
///
/// Variants need to be serializable and deserializable.
///
/// ```compile_fail,E0277
/// #[deser::open_enum]
/// trait Shape: Send + Sync {}
///
/// struct Circle;
///
/// #[deser::variant]
/// impl Shape for Circle {}
/// ```
///
/// The variant attribute is placed on implementations of traits.
///
/// ```compile_fail
/// struct Circle;
///
/// #[deser::variant]
/// impl Circle {}
/// ```
///
/// Only variants can be registered.
///
/// ```compile_fail,E0277
/// #[deser::open_enum]
/// trait Shape: Send + Sync {}
///
/// #[derive(deser::Serialize, deser::Deserialize)]
/// struct Circle;
///
/// let mut variants = deser::OpenEnums::new();
/// variants.register::<dyn Shape, Circle>().unwrap();
/// ```
///
/// Types with generic arguments work with a name (the counterpart of the
/// tests above).
///
/// ```
/// #[deser::open_enum]
/// trait Shape: Send + Sync {}
///
/// #[derive(deser::Serialize, deser::Deserialize)]
/// struct Wrapper<T>(T);
///
/// #[deser::variant(rename = "wrapper")]
/// impl Shape for Wrapper<u32> {}
/// ```
pub struct OpenEnumErrors;
