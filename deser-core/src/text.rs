//! Compact borrowed or owned slices for atoms.
//!
//! Atoms hold text and bytes that either borrow from the data that is
//! processed or are owned.  A [`Cow`] needs three words for this (the owned
//! variant carries the capacity of a `String`), the types here need two: a
//! pointer and a length whose highest bit marks owned data.  Owned data is a
//! boxed slice without spare capacity.  This keeps [`Atom`](crate::Atom)
//! small enough to carry text next to another value.
//!
//! The three bits below the owned bit are a small tag that other types can
//! store in the slice (like the type of the value of an
//! [`Implicit`](crate::Implicit)).  Slices ignore it.
use std::borrow::{Borrow, Cow};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr::NonNull;

/// The bit of the length that marks owned data.
const OWNED: usize = 1 << (usize::BITS - 1);

/// The shift of the tag stored in the length.
const TAG_SHIFT: u32 = usize::BITS - 4;

/// The bits of the length that hold the tag.
const TAG: usize = 0b111 << TAG_SHIFT;

/// The bits of the length that hold the length.
///
/// No slice is this large (this is a fraction of the address space of any
/// 64-bit system), but the constructors check it as the length of owned
/// data has to be exact to free it.
const LEN: usize = !(OWNED | TAG);

/// A byte slice that is borrowed for `'a` or owned (a `Box<[u8]>`).
pub(crate) struct Slice<'a> {
    ptr: NonNull<u8>,
    // the length, the highest bit is set if the data is owned
    len: usize,
    _marker: PhantomData<&'a [u8]>,
}

// SAFETY: the slice is either a `&'a [u8]` or a `Box<[u8]>`, both of which
// are `Send` and `Sync`.
unsafe impl Send for Slice<'_> {}
// SAFETY: see above
unsafe impl Sync for Slice<'_> {}

impl<'a> Slice<'a> {
    #[inline]
    pub(crate) const fn borrowed(data: &'a [u8]) -> Slice<'a> {
        assert!(data.len() <= LEN, "slice too large");
        Slice {
            // SAFETY: the pointer of a slice is never null.  It's never
            // written through as borrowed data is only read.
            ptr: unsafe { NonNull::new_unchecked(data.as_ptr().cast_mut()) },
            len: data.len(),
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn owned(data: Box<[u8]>) -> Slice<'a> {
        let len = data.len();
        assert!(len <= LEN, "slice too large");
        let ptr = Box::into_raw(data).cast::<u8>();
        Slice {
            // SAFETY: the pointer of a box is never null
            ptr: unsafe { NonNull::new_unchecked(ptr) },
            len: len | OWNED,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len & LEN
    }

    /// Returns the tag stored in the slice (`0` to `7`).
    #[inline]
    pub(crate) fn tag(&self) -> u8 {
        ((self.len & TAG) >> TAG_SHIFT) as u8
    }

    /// Returns the slice with a tag (`0` to `7`).
    #[inline]
    pub(crate) fn with_tag(mut self, tag: u8) -> Slice<'a> {
        debug_assert!(tag <= 7);
        self.len = (self.len & !TAG) | ((tag as usize) << TAG_SHIFT) & TAG;
        self
    }

    #[inline]
    pub(crate) fn is_owned(&self) -> bool {
        self.len & OWNED != 0
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &[u8] {
        // SAFETY: the pointer and length are the ones of a slice that is
        // borrowed for `'a` or owned by this value.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len()) }
    }

    /// Returns the data if it's borrowed for `'a`.
    #[inline]
    pub(crate) fn borrowed_slice(&self) -> Option<&'a [u8]> {
        if self.is_owned() {
            None
        } else {
            // SAFETY: borrowed data lives for `'a`
            Some(unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len()) })
        }
    }

    /// Returns a slice borrowing from this one.
    #[inline]
    pub(crate) fn reborrow(&self) -> Slice<'_> {
        Slice::borrowed(self.as_slice())
    }

    /// Converts the slice into a box, copying borrowed data.
    #[inline]
    pub(crate) fn into_box(self) -> Box<[u8]> {
        if self.is_owned() {
            let len = self.len();
            let ptr = self.ptr.as_ptr();
            std::mem::forget(self);
            // SAFETY: owned data was created from a box with this pointer
            // and length.  `self` was forgotten so the box is not freed.
            unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) }
        } else {
            Box::from(self.as_slice())
        }
    }

    /// Converts the slice into a `Cow`, owned data is not copied.
    #[inline]
    pub(crate) fn into_cow(self) -> Cow<'a, [u8]> {
        match self.borrowed_slice() {
            Some(data) => Cow::Borrowed(data),
            None => Cow::Owned(self.into_box().into_vec()),
        }
    }

    #[inline]
    pub(crate) fn from_cow(data: Cow<'a, [u8]>) -> Slice<'a> {
        match data {
            Cow::Borrowed(data) => Slice::borrowed(data),
            Cow::Owned(data) => Slice::owned(data.into_boxed_slice()),
        }
    }

    #[inline]
    pub(crate) fn to_static(&self) -> Slice<'static> {
        Slice::owned(Box::from(self.as_slice()))
    }
}

impl Drop for Slice<'_> {
    #[inline]
    fn drop(&mut self) {
        if self.is_owned() {
            // SAFETY: owned data was created from a box with this pointer
            // and length.
            drop(unsafe {
                Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    self.ptr.as_ptr(),
                    self.len(),
                ))
            });
        }
    }
}

impl Clone for Slice<'_> {
    #[inline]
    fn clone(&self) -> Self {
        if self.is_owned() {
            Slice::owned(Box::from(self.as_slice())).with_tag(self.tag())
        } else {
            Slice {
                ptr: self.ptr,
                len: self.len,
                _marker: PhantomData,
            }
        }
    }
}

/// Text of an [`Atom`](crate::Atom), borrowed or owned.
///
/// This is like a `Cow<'a, str>` with a more compact representation: it's
/// two words large, owned text is a `Box<str>`.  It dereferences to `str`
/// and converts from and into `&str`, `String` and `Cow<str>`:
///
/// ```
/// use std::borrow::Cow;
/// use deser::Text;
///
/// let text = Text::from("hello");
/// assert_eq!(text, "hello");
/// assert_eq!(text.borrowed_str(), Some("hello"));
///
/// let owned = Text::from(String::from("world"));
/// assert_eq!(owned.len(), 5);
/// assert_eq!(owned.borrowed_str(), None);
/// assert_eq!(owned.into_cow(), Cow::<str>::Owned("world".into()));
/// ```
#[derive(Clone)]
pub struct Text<'a>(Slice<'a>);

impl<'a> Text<'a> {
    /// Creates text that borrows.
    #[inline]
    pub const fn borrowed(text: &'a str) -> Text<'a> {
        Text(Slice::borrowed(text.as_bytes()))
    }

    /// Creates owned text.
    ///
    /// The string is converted into a `Box<str>`, which reallocates if the
    /// string has spare capacity.
    #[inline]
    pub fn owned<S: Into<Box<str>>>(text: S) -> Text<'a> {
        Text(Slice::owned(text.into().into_boxed_bytes()))
    }

    /// Returns the tag stored in the text (see [`Slice::tag`]).
    #[inline]
    pub(crate) fn tag(&self) -> u8 {
        self.0.tag()
    }

    /// Returns the text with a tag (see [`Slice::with_tag`]).
    #[inline]
    pub(crate) fn with_tag(self, tag: u8) -> Text<'a> {
        Text(self.0.with_tag(tag))
    }

    /// Returns the text.
    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: the data is always valid UTF-8
        unsafe { std::str::from_utf8_unchecked(self.0.as_slice()) }
    }

    /// Returns the text if it borrows for `'a`.
    ///
    /// This is used by types which borrow from the data that is
    /// deserialized (like `&'de str`).
    #[inline]
    pub fn borrowed_str(&self) -> Option<&'a str> {
        // SAFETY: the data is always valid UTF-8
        self.0
            .borrowed_slice()
            .map(|data| unsafe { std::str::from_utf8_unchecked(data) })
    }

    /// Returns `true` if the text borrows for `'a`.
    #[inline]
    pub fn is_borrowed(&self) -> bool {
        !self.0.is_owned()
    }

    /// Returns text borrowing from this one.
    #[inline]
    pub fn as_borrowed(&self) -> Text<'_> {
        Text(self.0.reborrow())
    }

    /// Makes an owned copy decoupling the lifetimes.
    #[inline]
    pub fn to_static(&self) -> Text<'static> {
        Text(self.0.to_static())
    }

    /// Converts the text into a `Cow`.
    ///
    /// Owned text is not copied.
    #[inline]
    pub fn into_cow(self) -> Cow<'a, str> {
        match self.0.into_cow() {
            // SAFETY: the data is always valid UTF-8
            Cow::Borrowed(data) => Cow::Borrowed(unsafe { std::str::from_utf8_unchecked(data) }),
            Cow::Owned(data) => Cow::Owned(unsafe { String::from_utf8_unchecked(data) }),
        }
    }

    /// Converts the text into a `String`.
    ///
    /// Owned text is not copied.
    #[inline]
    pub fn into_owned(self) -> String {
        self.into_cow().into_owned()
    }
}

impl Default for Text<'_> {
    #[inline]
    fn default() -> Self {
        Text::borrowed("")
    }
}

impl Deref for Text<'_> {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for Text<'_> {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for Text<'_> {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl<'a> From<&'a str> for Text<'a> {
    #[inline]
    fn from(text: &'a str) -> Text<'a> {
        Text::borrowed(text)
    }
}

impl<'a> From<&'a String> for Text<'a> {
    #[inline]
    fn from(text: &'a String) -> Text<'a> {
        Text::borrowed(text)
    }
}

impl From<String> for Text<'_> {
    #[inline]
    fn from(text: String) -> Self {
        Text::owned(text)
    }
}

impl From<Box<str>> for Text<'_> {
    #[inline]
    fn from(text: Box<str>) -> Self {
        Text::owned(text)
    }
}

impl<'a> From<Cow<'a, str>> for Text<'a> {
    #[inline]
    fn from(text: Cow<'a, str>) -> Text<'a> {
        match text {
            Cow::Borrowed(text) => Text::borrowed(text),
            Cow::Owned(text) => Text::owned(text),
        }
    }
}

impl<'a> From<Text<'a>> for Cow<'a, str> {
    #[inline]
    fn from(text: Text<'a>) -> Cow<'a, str> {
        text.into_cow()
    }
}

impl From<Text<'_>> for String {
    #[inline]
    fn from(text: Text<'_>) -> String {
        text.into_owned()
    }
}

impl fmt::Debug for Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl PartialEq for Text<'_> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Text<'_> {}

impl PartialOrd for Text<'_> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Text<'_> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for Text<'_> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

impl PartialEq<str> for Text<'_> {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Text<'_> {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for Text<'_> {
    #[inline]
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<Text<'_>> for str {
    #[inline]
    fn eq(&self, other: &Text<'_>) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<Text<'_>> for &str {
    #[inline]
    fn eq(&self, other: &Text<'_>) -> bool {
        *self == other.as_str()
    }
}

#[test]
fn test_text() {
    assert_eq!(std::mem::size_of::<Text>(), 16);
    assert_eq!(std::mem::size_of::<Option<Text>>(), 16);

    let borrowed = Text::from("borrowed");
    assert!(borrowed.is_borrowed());
    assert_eq!(borrowed.borrowed_str(), Some("borrowed"));
    assert_eq!(borrowed.clone(), "borrowed");
    assert_eq!(borrowed.to_static(), "borrowed");
    assert!(!borrowed.to_static().is_borrowed());

    let mut owned = String::with_capacity(64);
    owned.push_str("owned");
    let owned = Text::from(owned);
    assert!(!owned.is_borrowed());
    assert_eq!(owned.borrowed_str(), None);
    assert_eq!(owned.as_borrowed().borrowed_str(), Some("owned"));
    assert_eq!(owned.clone(), owned);
    assert_eq!(owned.clone().into_owned(), "owned");
    assert_eq!(owned.into_cow(), Cow::<str>::Owned("owned".into()));

    let empty = Text::from(String::new());
    assert_eq!(empty, "");
    assert_eq!(empty.into_owned(), "");
    assert_eq!(Text::default(), "");
}

#[test]
fn test_text_tag() {
    // the tag is not part of the text
    for text in [Text::from("tagged"), Text::from(String::from("tagged"))] {
        let owned = !text.is_borrowed();
        let tagged = text.with_tag(5);
        assert_eq!(tagged.tag(), 5);
        assert_eq!(tagged, "tagged");
        assert_eq!(tagged.len(), 6);
        assert_eq!(tagged.is_borrowed(), !owned);
        // clones keep it, conversions do not
        assert_eq!(tagged.clone().tag(), 5);
        assert_eq!(tagged.as_borrowed().tag(), 0);
        assert_eq!(tagged.to_static().tag(), 0);
        let retagged = tagged.with_tag(2);
        assert_eq!(retagged.tag(), 2);
        assert_eq!(retagged.clone().into_owned(), "tagged");
        assert_eq!(retagged.with_tag(0).tag(), 0);
    }
}
