/// Creates a newtype wrapper around `Option<T>` to implement sinks on.
///
/// Slot wrappers are useful to implement deserialization when stateless
/// deserialization is an option: the sink is the slot itself, which avoids
/// allocating the sink.  Due to Rust's orphan rules the wrapper has to be a
/// type of your crate so that you can implement
/// [`Sink`](crate::de::Sink) for it.  For more information see
/// [`de`](crate::de).
///
/// The macro creates a crate private type with the given name:
///
/// ```rust
/// deser::make_slot_wrapper!(SlotWrapper);
/// ```
///
/// This is a `#[repr(transparent)]` wrapper around `Option<T>` which
/// dereferences to the `Option<T>` and has these functions:
///
/// * `SlotWrapper::wrap(out: &mut Option<T>) -> &mut SlotWrapper<T>`
///   wraps a slot.
/// * `SlotWrapper::make_handle(out: &mut Option<T>) -> SinkHandle<'_, 'de>`
///   wraps a slot and returns a handle to it (if the wrapper implements
///   [`Sink`](crate::de::Sink)), which is what
///   [`Deserialize::deserialize_into`](crate::de::Deserialize::deserialize_into)
///   typically returns.  This is equivalent to
///   `SinkHandle::to(SlotWrapper::wrap(out))`.
#[macro_export]
macro_rules! make_slot_wrapper {
    ($name:ident) => {
        /// A slot wrapper created by `make_slot_wrapper!`.
        #[repr(transparent)]
        pub(crate) struct $name<T>(Option<T>);

        impl<T> $name<T> {
            /// Wraps a slot transparently.
            #[allow(dead_code)]
            pub(crate) fn wrap(out: &mut Option<T>) -> &mut Self {
                // SAFETY: the wrapper is a transparent wrapper around the slot
                unsafe { &mut *(out as *mut Option<T> as *mut $name<T>) }
            }

            /// Wraps a slot transparently and returns a handle to it.
            #[allow(dead_code)]
            pub(crate) fn make_handle<'de>(out: &mut Option<T>) -> $crate::de::SinkHandle<'_, 'de>
            where
                $name<T>: $crate::de::Sink<'de>,
            {
                $crate::de::SinkHandle::to(Self::wrap(out))
            }
        }

        impl<T> core::ops::Deref for $name<T> {
            type Target = Option<T>;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl<T> core::ops::DerefMut for $name<T> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}

/// Implements `__private_begin` for types which do not implement `finish`.
///
/// The derive generates the same code.
macro_rules! begin_without_finish {
    () => {
        #[inline]
        fn __private_begin(
            &self,
            state: &mut crate::State,
        ) -> Result<crate::ser::Begin<'_>, crate::Error> {
            let shape = crate::ser::Serialize::container_shape(self);
            Ok(crate::ser::Begin::chunk(
                crate::ser::Serialize::serialize(self, state)?,
                shape,
                false,
            ))
        }
    };
}
