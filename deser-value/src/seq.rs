use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};

use deser::Order;

use crate::tree;
use crate::value::{Kind, Value};

/// A sequence of values.
///
/// A sequence dereferences to a [`Vec`] of its values.  Additionally it
/// holds the [`Order`] of the values and if it holds the values of a
/// repeated key (see [`ContainerShape::with_repeated`]), which are passed on
/// when the sequence is serialized.  They are not considered when sequences
/// are compared.
///
/// [`ContainerShape::with_repeated`]: deser::ContainerShape::with_repeated
///
/// ```
/// use deser::Order;
/// use deser_value::{Seq, Value};
///
/// let mut seq = Seq::new().with_order(Order::Sorted);
/// seq.push(Value::from(1));
/// seq.push(Value::from(2));
/// assert_eq!(seq.len(), 2);
/// assert_eq!(seq.order(), Order::Sorted);
/// ```
#[derive(Default)]
pub struct Seq {
    pub(crate) items: Vec<Value>,
    pub(crate) order: Order,
    pub(crate) repeated: bool,
}

impl Seq {
    /// Creates an empty sequence.
    pub const fn new() -> Seq {
        Seq {
            items: Vec::new(),
            order: Order::Natural,
            repeated: false,
        }
    }

    /// Creates an empty sequence with a capacity.
    pub fn with_capacity(capacity: usize) -> Seq {
        Seq {
            items: Vec::with_capacity(capacity),
            order: Order::Natural,
            repeated: false,
        }
    }

    /// Returns the order of the values.
    pub fn order(&self) -> Order {
        self.order
    }

    /// Sets the order of the values.
    pub fn set_order(&mut self, order: Order) {
        self.order = order;
    }

    /// Sets the order of the values and returns the sequence.
    pub fn with_order(mut self, order: Order) -> Seq {
        self.order = order;
        self
    }

    /// Returns `true` if the sequence holds the values of a repeated key.
    ///
    /// See [`ContainerShape::with_repeated`](deser::ContainerShape::with_repeated).
    pub fn is_repeated(&self) -> bool {
        self.repeated
    }

    /// Sets if the sequence holds the values of a repeated key.
    pub fn set_repeated(&mut self, yes: bool) {
        self.repeated = yes;
    }

    /// Sets if the sequence holds the values of a repeated key and returns
    /// the sequence.
    pub fn with_repeated(mut self, yes: bool) -> Seq {
        self.repeated = yes;
        self
    }

    /// Creates an empty sequence with the order and the repeated flag of
    /// this one.
    pub(crate) fn empty_like(&self, capacity: usize) -> Seq {
        Seq::with_capacity(capacity)
            .with_order(self.order)
            .with_repeated(self.repeated)
    }

    /// Converts the sequence into a vector.
    pub fn into_vec(mut self) -> Vec<Value> {
        std::mem::take(&mut self.items)
    }
}

impl Deref for Seq {
    type Target = Vec<Value>;

    fn deref(&self) -> &Vec<Value> {
        &self.items
    }
}

impl DerefMut for Seq {
    fn deref_mut(&mut self) -> &mut Vec<Value> {
        &mut self.items
    }
}

impl Drop for Seq {
    fn drop(&mut self) {
        if self.items.iter().any(|item| item.has_children()) {
            tree::drop_values(std::mem::take(&mut self.items));
        }
    }
}

impl Clone for Seq {
    fn clone(&self) -> Seq {
        match tree::clone_seq(self) {
            Kind::Seq(seq) => seq,
            _ => unreachable!(),
        }
    }
}

impl PartialEq for Seq {
    fn eq(&self, other: &Seq) -> bool {
        tree::eq_seq(self, other)
    }
}

impl Eq for Seq {}

impl Hash for Seq {
    fn hash<H: Hasher>(&self, state: &mut H) {
        tree::hash_seq(self, state);
    }
}

impl fmt::Debug for Seq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        tree::fmt_seq(self, f)
    }
}

impl From<Vec<Value>> for Seq {
    fn from(items: Vec<Value>) -> Seq {
        Seq {
            items,
            order: Order::Natural,
            repeated: false,
        }
    }
}

impl From<Seq> for Vec<Value> {
    fn from(seq: Seq) -> Vec<Value> {
        seq.into_vec()
    }
}

impl<T: Into<Value>> FromIterator<T> for Seq {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Seq {
        Seq::from(iter.into_iter().map(Into::into).collect::<Vec<_>>())
    }
}

impl<T: Into<Value>> Extend<T> for Seq {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.items.extend(iter.into_iter().map(Into::into));
    }
}

impl IntoIterator for Seq {
    type Item = Value;
    type IntoIter = std::vec::IntoIter<Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<'a> IntoIterator for &'a Seq {
    type Item = &'a Value;
    type IntoIter = std::slice::Iter<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<'a> IntoIterator for &'a mut Seq {
    type Item = &'a mut Value;
    type IntoIter = std::slice::IterMut<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter_mut()
    }
}
