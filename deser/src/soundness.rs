//! Compile tests for soundness issues that were fixed.  These are only
//! compiled as doctests.

/// A driver must not outlive the sink it drives.
///
/// ```compile_fail,E0597
/// use deser::de::DeserializeDriver;
/// use deser::Deserialize;
///
/// let mut driver = {
///     let mut out = None::<Vec<u32>>;
///     DeserializeDriver::from_sink(Deserialize::deserialize_into(&mut out))
/// };
/// driver.emit(1u64).unwrap();
/// ```
///
/// The sink held by an owned sink cannot be replaced.
///
/// ```compile_fail,E0277
/// use deser::de::{OwnedSink, SinkHandle};
/// use deser::Deserialize;
///
/// let mut owned = OwnedSink::<u32>::deserialize();
/// let mut local = None::<u32>;
/// *owned.borrow_mut() = Deserialize::deserialize_into(&mut local);
/// ```
///
/// Borrowed extensions have to prove that their values can be shortened.
/// For types that are not covariant, the trivial implementation does not
/// compile.
///
/// ```compile_fail
/// use std::sync::Mutex;
/// use deser::ext::BorrowedExtension;
/// use deser::Atom;
///
/// #[derive(Debug)]
/// pub struct Invariant<'a>(pub Mutex<&'a str>);
///
/// impl PartialEq for Invariant<'_> {
///     fn eq(&self, _other: &Self) -> bool {
///         true
///     }
/// }
///
/// impl BorrowedExtension for Invariant<'static> {
///     type Value<'a> = Invariant<'a>;
///     fn name<'v>(_value: &'v Invariant<'_>) -> &'v str {
///         "invariant"
///     }
///     fn fallback<'v>(_value: &'v Invariant<'_>) -> Atom<'v> {
///         Atom::Null
///     }
///     fn to_static(_value: &Invariant<'_>) -> Invariant<'static> {
///         Invariant(Mutex::new(""))
///     }
///     fn shorten<'s, 'l: 's>(value: &'s Invariant<'l>) -> &'s Invariant<'s> {
///         value
///     }
/// }
/// ```
///
/// Values of borrowed extensions cannot escape the extension value.
///
/// ```compile_fail,E0515
/// # use std::borrow::Cow;
/// # use deser::ext::BorrowedExtension;
/// # use deser::Atom;
/// # #[derive(Debug, Clone, PartialEq)]
/// # pub struct Literal<'a>(Cow<'a, str>);
/// # impl BorrowedExtension for Literal<'static> {
/// #     type Value<'a> = Literal<'a>;
/// #     fn name<'v>(_value: &'v Literal<'_>) -> &'v str { "literal" }
/// #     fn fallback<'v>(value: &'v Literal<'_>) -> Atom<'v> { Atom::Str(Cow::Borrowed(&value.0)) }
/// #     fn to_static(value: &Literal<'_>) -> Literal<'static> { Literal(Cow::Owned(value.0.to_string())) }
/// #     fn shorten<'s, 'l: 's>(value: &'s Literal<'l>) -> &'s Literal<'s> { value }
/// # }
/// use deser::ext::ExtValue;
///
/// fn leak<'a>(ext: ExtValue<'a>) -> &'a Literal<'a> {
///     ext.downcast_value_ref::<Literal>().unwrap()
/// }
/// ```
///
/// Extension values cannot outlive the data they borrow.
///
/// ```compile_fail,E0597
/// # use std::borrow::Cow;
/// # use deser::ext::BorrowedExtension;
/// # use deser::Atom;
/// # #[derive(Debug, Clone, PartialEq)]
/// # pub struct Literal<'a>(Cow<'a, str>);
/// # impl BorrowedExtension for Literal<'static> {
/// #     type Value<'a> = Literal<'a>;
/// #     fn name<'v>(_value: &'v Literal<'_>) -> &'v str { "literal" }
/// #     fn fallback<'v>(value: &'v Literal<'_>) -> Atom<'v> { Atom::Str(Cow::Borrowed(&value.0)) }
/// #     fn to_static(value: &Literal<'_>) -> Literal<'static> { Literal(Cow::Owned(value.0.to_string())) }
/// #     fn shorten<'s, 'l: 's>(value: &'s Literal<'l>) -> &'s Literal<'s> { value }
/// # }
/// use deser::ext::ExtValue;
///
/// let ext = {
///     let text = String::from("1.5");
///     ExtValue::owned_value::<Literal>(Literal(Cow::Borrowed(&text)))
/// };
/// drop(ext);
/// ```
pub struct SoundnessTests;
