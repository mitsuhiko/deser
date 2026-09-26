//! Compile tests for attributes that the derive rejects.  These are only
//! compiled as doctests.

/// `Self` is not supported in default expressions.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(default = Self::make())]
/// struct Test {
///     field: u32,
/// }
///
/// impl Test {
///     fn make() -> Test {
///         Test { field: 1 }
///     }
/// }
/// ```
///
/// `Self` is not supported in `skip_serializing_if`.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// struct Test {
///     #[deser(skip_serializing_if = Self::is_zero)]
///     field: u32,
/// }
///
/// impl Test {
///     fn is_zero(value: &u32) -> bool {
///         *value == 0
///     }
/// }
/// ```
///
/// `Self` is not supported in bounds.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(bound(Self: Clone))]
/// struct Test<T> {
///     field: T,
/// }
/// ```
///
/// Bounds are lists in parentheses.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(bound = T: deser::Serialize)]
/// struct Test<T> {
///     field: T,
/// }
/// ```
///
/// Custom bounds replace the inferred bounds.
///
/// ```compile_fail,E0277
/// #[derive(deser::Serialize)]
/// #[deser(bound())]
/// struct Test<T> {
///     field: T,
/// }
/// ```
///
/// The crate path must exist.
///
/// ```compile_fail,E0432
/// #[derive(deser::Serialize)]
/// #[deser(crate = does_not_exist)]
/// struct Test {
///     field: u32,
/// }
/// ```
///
/// `skip_serializing_if` takes a path, not a string.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// struct Test {
///     #[deser(skip_serializing_if = "Option::is_none")]
///     field: Option<u32>,
/// }
/// ```
///
/// Functions used as defaults need to be called.
///
/// ```compile_fail,E0308
/// fn make() -> u32 {
///     42
/// }
///
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(default = make)]
///     field: u32,
/// }
/// ```
///
/// The positive case for the above.
///
/// ```
/// fn make() -> u32 {
///     42
/// }
///
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(default = make())]
///     field: u32,
/// }
/// ```
///
/// Adapters cannot be combined with flatten.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Inner {
///     field: u32,
/// }
///
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(flatten, as = deser::adapters::Same)]
///     inner: Inner,
/// }
/// ```
///
/// `Self` is not supported in adapters.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// struct Test {
///     #[deser(as = Vec<Self>)]
///     field: Vec<u32>,
/// }
/// ```
///
/// Adapters must support the type of the field.
///
/// ```compile_fail,E0277
/// #[derive(deser::Serialize)]
/// struct Test {
///     #[deser(as = Vec<deser::adapters::DisplayFromStr>)]
///     field: Option<u32>,
/// }
/// ```
///
/// Tag fields are only supported in other variants.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(tag)]
///     field: String,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// enum Test {
///     A(#[deser(tag)] String),
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// enum Test {
///     #[deser(other)]
///     A(#[deser(tag)] String, #[deser(tag)] String),
/// }
/// ```
///
/// Tag fields only support `as`.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// enum Test {
///     #[deser(other)]
///     A {
///         #[deser(tag, rename = "x")]
///         tag: String,
///     },
/// }
/// ```
///
/// Default variants are only supported for internally and adjacently
/// tagged enums.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// enum Test {
///     #[deser(default)]
///     A(u32),
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// enum Test {
///     #[deser(default)]
///     A,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(tag = "type")]
/// enum Test {
///     #[deser(default)]
///     A,
///     #[deser(default)]
///     B,
/// }
/// ```
///
/// The positive case for the above.
///
/// ```
/// #[derive(deser::Deserialize)]
/// #[deser(tag = "type")]
/// enum Test {
///     #[deser(default)]
///     A,
///     B,
/// }
/// ```
///
/// Adapters on containers cannot use the implementation of the container
/// itself (`_`, `Same` or the type) as it forwards to the adapter.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(as = _)]
/// struct Test {
///     field: u32,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(as = deser::adapters::Same)]
/// struct Test {
///     field: u32,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(deserialize_as = deser::adapters::DefaultOnError<_>)]
/// struct Test {
///     field: u32,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(Clone, deser::Serialize)]
/// #[deser(serialize_as = deser::adapters::FromInto<Test>)]
/// struct Test {
///     field: u32,
/// }
/// ```
///
/// The positive case for the above: the type can be used in the adapter if
/// it's not used directly.
///
/// ```
/// #[derive(Clone, deser::Serialize)]
/// #[deser(serialize_as = deser::adapters::FromInto<Vec<Node>>)]
/// struct Node {
///     children: Vec<Node>,
/// }
///
/// impl From<Node> for Vec<Node> {
///     fn from(value: Node) -> Vec<Node> {
///         value.children
///     }
/// }
/// ```
///
/// `as` cannot be combined with `serialize_as` or `deserialize_as`.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(as = deser::adapters::DisplayFromStr, serialize_as = deser::adapters::DisplayFromStr)]
/// struct Test {
///     field: u32,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// struct Test {
///     #[deser(as = deser::adapters::DisplayFromStr, deserialize_as = deser::adapters::DisplayFromStr)]
///     field: u32,
/// }
/// ```
///
/// Attributes that have no effect because the container forwards to an
/// adapter are rejected.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(as = deser::adapters::DisplayFromStr)]
/// struct Test {
///     #[deser(rename = "x")]
///     field: u32,
/// }
/// # impl std::fmt::Display for Test {
/// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }
/// # }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(as = deser::adapters::DisplayFromStr, rename_all = "camelCase")]
/// struct Test {
///     field: u32,
/// }
/// # impl std::fmt::Display for Test {
/// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }
/// # }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(as = deser::adapters::DisplayFromStr)]
/// enum Test {
///     #[deser(rename = "a")]
///     A,
/// }
/// # impl std::fmt::Display for Test {
/// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }
/// # }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(deserialize_as = deser::adapters::DisplayFromStr, default)]
/// struct Test {
///     field: u32,
/// }
/// # impl std::str::FromStr for Test {
/// #     type Err = String;
/// #     fn from_str(s: &str) -> Result<Test, String> { Err(s.into()) }
/// # }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(deserialize_as = deser::adapters::DisplayFromStr)]
/// struct Test {
///     #[deser(alias = "x")]
///     field: u32,
/// }
/// # impl std::str::FromStr for Test {
/// #     type Err = String;
/// #     fn from_str(s: &str) -> Result<Test, String> { Err(s.into()) }
/// # }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(serialize_as = deser::adapters::DisplayFromStr, skip_serializing_optionals)]
/// struct Test {
///     field: Option<u32>,
/// }
/// # impl std::fmt::Display for Test {
/// #     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { Ok(()) }
/// # }
/// ```
///
/// The positive case for the above: attributes that affect the direction
/// which is derived are fine.
///
/// ```
/// #[derive(deser::Serialize, deser::Deserialize)]
/// #[deser(deserialize_as = deser::adapters::DisplayFromStr, rename_all = "camelCase")]
/// struct Test {
///     #[deser(skip_serializing_if = Option::is_none)]
///     some_field: Option<u32>,
/// }
/// # impl std::str::FromStr for Test {
/// #     type Err = String;
/// #     fn from_str(s: &str) -> Result<Test, String> { Err(s.into()) }
/// # }
/// ```
///
/// Adapters cannot be combined with flatten, also for one direction.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// struct Inner {
///     field: u32,
/// }
///
/// #[derive(deser::Serialize)]
/// struct Test {
///     #[deser(flatten, serialize_as = deser::adapters::Same)]
///     inner: Inner,
/// }
/// ```
///
/// Adapters on containers must support the type.
///
/// ```compile_fail,E0277
/// #[derive(deser::Serialize)]
/// #[deser(as = deser::adapters::DisplayFromStr)]
/// struct Test {
///     field: u32,
/// }
/// ```
///
/// The lifetime `'de` is reserved for the lifetime of `Deserialize`.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test<'de> {
///     field: &'de str,
/// }
/// ```
///
/// Types that borrow cannot be deserialized from data that does not
/// outlive them.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test<'a> {
///     field: &'a str,
/// }
///
/// fn owned<T: deser::de::DeserializeOwned>() {}
/// owned::<Test<'static>>();
/// ```
pub struct DeriveErrors;
