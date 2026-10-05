//! A mutex that is available without the standard library.
//!
//! With `std` this is the mutex of the standard library, otherwise a spin
//! lock.  It's only used for short critical sections that do not call into
//! code that could block or panic, which is also why poisoning is ignored.

#[cfg(feature = "std")]
pub(crate) use std::sync::MutexGuard;

#[cfg(not(feature = "std"))]
pub(crate) use self::spin::MutexGuard;

#[derive(Default)]
pub(crate) struct Mutex<T> {
    #[cfg(feature = "std")]
    inner: std::sync::Mutex<T>,
    #[cfg(not(feature = "std"))]
    inner: spin::Mutex<T>,
}

impl<T> Mutex<T> {
    pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
        #[cfg(feature = "std")]
        {
            self.inner.lock().unwrap_or_else(|err| err.into_inner())
        }
        #[cfg(not(feature = "std"))]
        {
            self.inner.lock()
        }
    }
}

#[cfg(not(feature = "std"))]
mod spin {
    use core::cell::UnsafeCell;
    use core::marker::PhantomData;
    use core::ops::{Deref, DerefMut};
    use core::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    pub(crate) struct Mutex<T> {
        locked: AtomicBool,
        value: UnsafeCell<T>,
    }

    // SAFETY: the lock gives one thread at a time access to the value
    unsafe impl<T: Send> Sync for Mutex<T> {}

    impl<T> Mutex<T> {
        pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
            while self
                .locked
                .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                core::hint::spin_loop();
            }
            MutexGuard(self, PhantomData)
        }
    }

    /// Like the guard of the standard library this is neither `Send` nor
    /// `Sync` by default: it hands out references to `T` (so it's only
    /// `Sync` if `T` is) and it's not sent between threads.
    pub(crate) struct MutexGuard<'a, T>(&'a Mutex<T>, PhantomData<*const ()>);

    // SAFETY: shared references to the guard only give out `&T`
    unsafe impl<T: Sync> Sync for MutexGuard<'_, T> {}

    impl<T> Deref for MutexGuard<'_, T> {
        type Target = T;

        fn deref(&self) -> &T {
            // SAFETY: the guard holds the lock
            unsafe { &*self.0.value.get() }
        }
    }

    impl<T> DerefMut for MutexGuard<'_, T> {
        fn deref_mut(&mut self) -> &mut T {
            // SAFETY: the guard holds the lock
            unsafe { &mut *self.0.value.get() }
        }
    }

    impl<T> Drop for MutexGuard<'_, T> {
        fn drop(&mut self) {
            self.0.locked.store(false, Ordering::Release);
        }
    }
}

#[test]
fn test_mutex() {
    use alloc::sync::Arc;
    use alloc::vec::Vec;

    let mutex = Arc::new(Mutex::<Vec<usize>>::default());
    let threads = (0..4)
        .map(|idx| {
            let mutex = mutex.clone();
            std::thread::spawn(move || {
                for _ in 0..if cfg!(miri) { 10 } else { 1000 } {
                    mutex.lock().push(idx);
                }
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    let values = mutex.lock();
    assert_eq!(values.len(), 4 * if cfg!(miri) { 10 } else { 1000 });
}
