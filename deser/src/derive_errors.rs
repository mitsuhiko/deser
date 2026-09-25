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
/// Closures are not supported in default expressions.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(default = (|| 42)())]
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
pub struct DeriveErrors;
