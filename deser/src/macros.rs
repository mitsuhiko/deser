macro_rules! extend_lifetime {
    ($expr:expr, $t:ty) => {
        std::mem::transmute::<$t, $t>($expr)
    };
}

/// Creates a typed slot wrapper.
///
/// Slot wrappers are required to implement deserialization.  For
/// more information see [`de`](crate::de).  To see the generated
/// slot wrapper API see [`SlotWrapper`](crate::de::SlotWrapper).
///
/// ## Example
///
/// ```rust
/// use deser::make_slot_wrapper;
/// make_slot_wrapper!(SlotWrapper);
/// ```
#[macro_export]
macro_rules! make_slot_wrapper {
    ($name:ident) => {
        $crate::__make_slot_wrapper!((pub(crate)), $name);
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! __make_slot_wrapper {
    (($($vis:tt)*), $name:ident) => {
        /// The generated slot wrapper.
        ///
        /// Note that you need to generate your own slot wrapper by using the
        /// [`make_slot_wrapper`] macro so you're able to implement a sink
        /// for it.
        #[repr(transparent)]
        $($vis)* struct $name<T>(Option<T>);

        impl<T> $name<T> {
            /// Wraps a slot transparently.
            pub fn wrap(out: &mut Option<T>) -> &mut Self {
                unsafe { &mut *(out as *mut Option<T> as *mut $name<T>) }
            }
        }

        impl<T> std::ops::Deref for $name<T> {
            type Target = Option<T>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl<T> std::ops::DerefMut for $name<T> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}
