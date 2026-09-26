use crate::map::{Map, MapKey};
use crate::value::{Kind, Value};

mod sealed {
    pub trait Sealed {}
}

/// A type that can index into a [`Value`].
///
/// Sequences are indexed by `usize`, maps by strings or any other
/// [`Value`] as key.  This is used by [`Kind::get`] and the `Index`
/// implementation of [`Value`].
///
/// ```
/// use deser_value::value;
///
/// let value = value!({"items": [1, 2], 42: "answer"});
/// assert_eq!(value["items"][1], 2);
/// assert_eq!(value[&value!(42)], "answer");
/// assert!(value["missing"].is_null());
/// ```
pub trait ValueIndex: sealed::Sealed {
    #[doc(hidden)]
    fn index_into<'v>(&self, value: &'v Kind) -> Option<&'v Value>;

    #[doc(hidden)]
    fn index_into_mut<'v>(&self, value: &'v mut Kind) -> Option<&'v mut Value>;

    #[doc(hidden)]
    fn index_or_insert<'v>(&self, value: &'v mut Value) -> &'v mut Value;
}

impl sealed::Sealed for usize {}

impl ValueIndex for usize {
    fn index_into<'v>(&self, value: &'v Kind) -> Option<&'v Value> {
        match value {
            Kind::Seq(seq) => seq.get(*self),
            _ => None,
        }
    }

    fn index_into_mut<'v>(&self, value: &'v mut Kind) -> Option<&'v mut Value> {
        match value {
            Kind::Seq(seq) => seq.get_mut(*self),
            _ => None,
        }
    }

    fn index_or_insert<'v>(&self, value: &'v mut Value) -> &'v mut Value {
        match &mut value.kind {
            Kind::Seq(seq) => {
                let len = seq.len();
                seq.get_mut(*self).unwrap_or_else(|| {
                    panic!(
                        "index {} out of bounds for sequence of length {}",
                        self, len
                    )
                })
            }
            other => panic!("cannot index into {} with an integer", other.name()),
        }
    }
}

/// Indexes into maps by key.
fn index_or_insert_key<'v, K: MapKey + ?Sized>(
    key: &K,
    make_key: impl FnOnce() -> Value,
    value: &'v mut Value,
) -> &'v mut Value {
    if let Kind::Null = value.kind {
        value.kind = Kind::Map(Map::new());
    }
    match &mut value.kind {
        Kind::Map(map) => {
            if let Some(index) = map.get_index_of(key) {
                map.get_index_mut(index).unwrap().1
            } else {
                map.get_or_insert_with(make_key(), Value::null)
            }
        }
        other => panic!("cannot index into {} with a key", other.name()),
    }
}

impl sealed::Sealed for str {}

impl ValueIndex for str {
    fn index_into<'v>(&self, value: &'v Kind) -> Option<&'v Value> {
        value.as_map().and_then(|map| map.get(self))
    }

    fn index_into_mut<'v>(&self, value: &'v mut Kind) -> Option<&'v mut Value> {
        value.as_map_mut().and_then(|map| map.get_mut(self))
    }

    fn index_or_insert<'v>(&self, value: &'v mut Value) -> &'v mut Value {
        index_or_insert_key(self, || Value::from(self), value)
    }
}

impl sealed::Sealed for String {}

impl ValueIndex for String {
    fn index_into<'v>(&self, value: &'v Kind) -> Option<&'v Value> {
        self.as_str().index_into(value)
    }

    fn index_into_mut<'v>(&self, value: &'v mut Kind) -> Option<&'v mut Value> {
        self.as_str().index_into_mut(value)
    }

    fn index_or_insert<'v>(&self, value: &'v mut Value) -> &'v mut Value {
        self.as_str().index_or_insert(value)
    }
}

impl sealed::Sealed for Value {}

impl ValueIndex for Value {
    fn index_into<'v>(&self, value: &'v Kind) -> Option<&'v Value> {
        value.as_map().and_then(|map| map.get(self))
    }

    fn index_into_mut<'v>(&self, value: &'v mut Kind) -> Option<&'v mut Value> {
        value.as_map_mut().and_then(|map| map.get_mut(self))
    }

    fn index_or_insert<'v>(&self, value: &'v mut Value) -> &'v mut Value {
        index_or_insert_key(self, || self.clone(), value)
    }
}

impl<T: ValueIndex + ?Sized> sealed::Sealed for &T {}

impl<T: ValueIndex + ?Sized> ValueIndex for &T {
    fn index_into<'v>(&self, value: &'v Kind) -> Option<&'v Value> {
        (**self).index_into(value)
    }

    fn index_into_mut<'v>(&self, value: &'v mut Kind) -> Option<&'v mut Value> {
        (**self).index_into_mut(value)
    }

    fn index_or_insert<'v>(&self, value: &'v mut Value) -> &'v mut Value {
        (**self).index_or_insert(value)
    }
}
