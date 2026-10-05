//! Dropping stacks whose items borrow from the items below them.
//!
//! The drivers hold stacks of sinks, emitters and values where every item
//! can borrow from the items below it, so they have to be dropped from the
//! last to the first.  A vector drops its items from the first to the last,
//! also when it's dropped while unwinding from a panic in the drop of one
//! of its items.  The helpers here keep the order in both cases.
use alloc::vec::Vec;

/// Drops the remaining items of a vector from the last to the first when
/// it's dropped.
///
/// This is a guard for code that pops the items of a stack itself: if that
/// panics, the items that are left are still dropped in the right order.
/// If one of them panics as well, the process aborts (a panic while
/// unwinding), which does not drop the items below it.
pub(crate) struct DropInReverse<'a, T>(pub(crate) &'a mut Vec<T>);

impl<T> Drop for DropInReverse<'_, T> {
    fn drop(&mut self) {
        while let Some(item) = self.0.pop() {
            drop(item);
        }
    }
}

/// Pops the items of a vector from the last to the first and passes them
/// to `f`.
///
/// If `f` (or the drop of an item) panics, the remaining items are dropped
/// from the last to the first (see [`DropInReverse`]).
#[inline]
pub(crate) fn pop_all<T>(vec: &mut Vec<T>, mut f: impl FnMut(T)) {
    let guard = DropInReverse(vec);
    while let Some(item) = guard.0.pop() {
        f(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::rc::Rc;
    use core::cell::RefCell;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    /// Logs its index when dropped, panics if it's the given one.
    struct Item {
        index: usize,
        panics: bool,
        log: Rc<RefCell<Vec<usize>>>,
    }

    impl Drop for Item {
        fn drop(&mut self) {
            self.log.borrow_mut().push(self.index);
            if self.panics {
                panic!("item {} panics", self.index);
            }
        }
    }

    fn items(log: &Rc<RefCell<Vec<usize>>>, panics: Option<usize>) -> Vec<Item> {
        (0..5)
            .map(|index| Item {
                index,
                panics: panics == Some(index),
                log: log.clone(),
            })
            .collect()
    }

    #[test]
    fn test_pop_all() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut vec = items(&log, None);
        pop_all(&mut vec, drop);
        assert_eq!(*log.borrow(), [4, 3, 2, 1, 0]);
        assert!(vec.is_empty());
    }

    #[test]
    fn test_pop_all_panics() {
        // the items after the one that panics are dropped in reverse
        for panics in 0..5 {
            let log = Rc::new(RefCell::new(Vec::new()));
            let mut vec = items(&log, Some(panics));
            let rv = catch_unwind(AssertUnwindSafe(|| pop_all(&mut vec, drop)));
            assert!(rv.is_err());
            assert_eq!(*log.borrow(), [4, 3, 2, 1, 0]);
            assert!(vec.is_empty());
        }
    }
}
