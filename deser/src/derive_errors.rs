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
///
/// `deny_unknown_fields` has no effect on newtype structs.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(deny_unknown_fields)]
/// struct Test(u32);
/// ```
///
/// `deny_unknown_fields`, `default`, `rename_all` and `alias_all` have no
/// effect on tuple structs and unit structs.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(deny_unknown_fields)]
/// struct Test(u32, u32);
/// ```
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(rename_all = "camelCase")]
/// struct Test(u32, u32);
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(default)]
/// struct Test;
/// ```
///
/// `default` on unnamed fields requires `skip` or `skip_deserializing`.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test(u32, #[deser(default)] u32);
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// enum Test {
///     A(u32, #[deser(default = 1)] u32),
/// }
/// ```
///
/// Skipped unnamed fields cannot have adapters.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// struct Test(u32, #[deser(skip, as = deser::adapters::DisplayFromStr)] u32);
/// ```
///
/// `rename_all_fields` is for enums, `rename_all` on variants for struct
/// variants.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(rename_all_fields = "camelCase")]
/// struct Test {
///     a_b: u32,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// enum Test {
///     #[deser(rename_all = "camelCase")]
///     A(u32),
/// }
/// ```
///
/// Transparent structs have exactly one field that is not skipped.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(transparent)]
/// struct Test {
///     a: u32,
///     b: u32,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(transparent)]
/// struct Test(u32, u32);
/// ```
///
/// Field attributes other than adapters and skips have no effect on
/// transparent structs.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(transparent)]
/// struct Test {
///     #[deser(rename = "b")]
///     a: u32,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// #[deser(transparent)]
/// enum Test {
///     A(u32),
/// }
/// ```
///
/// Tuple structs have at most 12 fields.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test(u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8);
/// ```
///
/// Unions need a container adapter.
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// union Test {
///     a: u32,
/// }
/// ```
///
/// `deny_unknown_fields` has no effect on enums with only unit variants.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(deny_unknown_fields)]
/// enum Test {
///     A,
///     B,
/// }
/// ```
///
/// `deny_unknown_fields` does not take a value.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(deny_unknown_fields = true)]
/// struct Test {
///     field: u32,
/// }
/// ```
///
/// `deny_unknown_fields` has no effect if the type is deserialized with an
/// adapter.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(deserialize_as = deser::adapters::FromInto<u32>, deny_unknown_fields)]
/// struct Test {
///     field: u32,
/// }
///
/// impl From<u32> for Test {
///     fn from(field: u32) -> Test {
///         Test { field }
///     }
/// }
/// ```
///
/// `validate` cannot be combined with `flatten`.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Inner {
///     field: u32,
/// }
///
/// fn check(_: &Inner) -> Result<(), &'static str> {
///     Ok(())
/// }
///
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(flatten, validate = check)]
///     inner: Inner,
/// }
/// ```
///
/// `Self` is not supported in `validate`.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(validate = Self::check)]
/// struct Test {
///     field: u32,
/// }
///
/// impl Test {
///     fn check(&self) -> Result<(), &'static str> {
///         Ok(())
///     }
/// }
/// ```
///
/// Validators need to accept a reference to the value.
///
/// ```compile_fail
/// fn check(_: &String) -> Result<(), &'static str> {
///     Ok(())
/// }
///
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(validate = check)]
///     field: u32,
/// }
/// ```
///
/// Variants can only be named by strings, integers and booleans.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// enum Test {
///     #[deser(rename = 1.5)]
///     A,
/// }
/// ```
///
/// Integer names are unique too.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// enum Test {
///     #[deser(rename = 1)]
///     A,
///     #[deser(alias = 1)]
///     B,
/// }
/// ```
///
/// Fields are only named by strings.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(rename = 1)]
///     field: u32,
/// }
/// ```
///
/// Names are strings, paths to constants or macro invocations, other
/// expressions are not supported.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(rename = "a".trim())]
///     field: u32,
/// }
/// ```
///
/// `Self` is not supported in names.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(rename = Self::NAME)]
///     field: u32,
/// }
///
/// impl Test {
///     const NAME: &'static str = "name";
/// }
/// ```
///
/// Names that are expressions need to be strings.
///
/// ```compile_fail
/// const NAME: u32 = 1;
///
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(rename = NAME)]
///     field: u32,
/// }
/// ```
///
/// `alias_all` takes the same styles as `rename_all`.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(alias_all = "Title Case")]
/// struct Test {
///     field: u32,
/// }
/// ```
///
/// Attributes that have no effect on skipped fields are rejected.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(skip, rename = "other")]
///     field: u32,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Serialize)]
/// struct Test {
///     #[deser(skip_serializing, skip_serializing_if = Option::is_none)]
///     field: Option<u32>,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(skip, skip_deserializing)]
///     field: u32,
/// }
/// ```
///
/// Skipped fields need a default.
///
/// ```compile_fail
/// struct NoDefault;
///
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(skip)]
///     field: NoDefault,
/// }
/// ```
///
/// Required fields cannot have a default.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// struct Test {
///     #[deser(required, default)]
///     field: Option<u32>,
/// }
/// ```
///
/// Skipped variants cannot be deserialized, attributes for deserializing
/// them have no effect.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// enum Test {
///     A,
///     #[deser(skip, alias = "b")]
///     B,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// enum Test {
///     A,
///     #[deser(skip_deserializing, other)]
///     B,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(tag = "t")]
/// enum Test {
///     A,
///     #[deser(skip, default)]
///     B,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// enum Test {
///     A,
///     #[deser(skip, skip_serializing)]
///     B,
/// }
/// ```
///
/// Variants of enums with `repr` are named by their discriminants, which
/// have to be integer literals.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(repr)]
/// enum Test {
///     A,
///     #[deser(rename = 5)]
///     B,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(repr, rename_all = "lowercase")]
/// enum Test {
///     A,
///     B,
/// }
/// ```
///
/// ```compile_fail
/// const B: isize = 2;
///
/// #[derive(deser::Deserialize)]
/// #[deser(repr)]
/// enum Test {
///     A,
///     B = B,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(repr)]
/// struct Test {
///     a: u32,
/// }
/// ```
///
/// Aliases of the tag and the content need a tag and a content.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(tag_alias = "kind")]
/// enum Test {
///     A { a: u32 },
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(tag = "t", content_alias = "data")]
/// enum Test {
///     A(u32),
/// }
/// ```
///
/// The tag and the content cannot share a key.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(tag = "t", content = "c", tag_alias = "c")]
/// enum Test {
///     A(u32),
/// }
/// ```
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(tag = "t", content = "t")]
/// enum Test {
///     A(u32),
/// }
/// ```
///
/// Tag keys are strings or constants.
///
/// ```compile_fail
/// #[derive(deser::Deserialize)]
/// #[deser(tag = 1)]
/// enum Test {
///     A { a: u32 },
/// }
/// ```
pub struct DeriveErrors;
