use crate::error::Error;
use crate::ser::{SerializeDriver, Serializer};

/// The result of [`StreamSerializer::drive_partial`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Written {
    /// The value is complete, the output holds the rest of it.
    Done,
    /// The value is not complete.
    ///
    /// The output holds a part of it which the caller takes (and clears)
    /// before it calls [`drive_partial`](StreamSerializer::drive_partial)
    /// again with the same driver to continue.
    Partial,
}

/// A [`Serializer`] that writes bytes which can be taken while values are
/// serialized.
///
/// This is implemented by the serializers of the data formats (for
/// instance `deser_json::Serializer`).  They hold the state of a stream of
/// values (for instance how many values were written, or the names of the
/// columns of a CSV file) and write everything that separates the values
/// of a stream, like the line breaks of JSON Lines or the markers between
/// YAML documents.
///
/// The bytes serialized so far are in [`output`](Self::output), whoever
/// writes them to a stream clears them with
/// [`clear_output`](Self::clear_output) afterwards.  This makes stream
/// serializers usable without IO (sans-io): the writers of `deser::io`
/// and of other IO adapters (like `deser-tokio`) only move the output to
/// their stream.
///
/// ```
/// use deser::ser::{SerializeDriver, Serializer, StreamSerializer};
/// use deser::{Atom, Error, Event};
///
/// /// A format with a number per line.
/// #[derive(Default)]
/// struct Lines(Vec<u8>);
///
/// impl Serializer for Lines {
///     fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
///         driver.drive(|event, _| {
///             if let Event::Atom(Atom::U64(value)) = event {
///                 self.0.extend_from_slice(format!("{value}\n").as_bytes());
///             }
///             Ok(())
///         })
///     }
/// }
///
/// impl StreamSerializer for Lines {
///     fn output(&self) -> &[u8] {
///         &self.0
///     }
///
///     fn clear_output(&mut self) {
///         self.0.clear();
///     }
/// }
///
/// let mut lines = Lines::default();
/// lines.serialize(&1u64).unwrap();
/// lines.serialize(&2u64).unwrap();
/// // the output would be written to a stream here
/// assert_eq!(lines.output(), b"1\n2\n");
/// lines.clear_output();
/// ```
///
/// # Serializing in Parts
///
/// Formats which can write the output of a value before the value is
/// complete additionally implement [`drive_partial`](Self::drive_partial).
/// Writers use it to write large values in parts, so the memory used does
/// not depend on the size of the values.  While a value is written in
/// parts, [`in_progress`](Self::in_progress) is `true`.
pub trait StreamSerializer: Serializer {
    /// Returns the bytes serialized so far which were not cleared yet.
    ///
    /// The output only holds final bytes: output that can still change
    /// (for instance the header of a container whose length is not known
    /// yet) is kept by the serializer until it's final.
    fn output(&self) -> &[u8];

    /// Discards the bytes returned by [`output`](Self::output), for
    /// instance because they were written.
    fn clear_output(&mut self);

    /// Returns `true` if the serializer implements
    /// [`drive_partial`](Self::drive_partial).
    ///
    /// This can depend on the configuration.
    fn supports_partial(&self) -> bool {
        false
    }

    /// Serializes a value, or a part of it, and appends its bytes to the
    /// output.
    ///
    /// This works like [`drive`](Serializer::drive) but the serializer can
    /// stop once the output holds at least `limit` bytes (it can hold
    /// more) and return [`Written::Partial`].  The caller then takes the
    /// output, clears it and calls again with the same driver until the
    /// value is complete ([`Written::Done`]).  In between
    /// [`in_progress`](Self::in_progress) is `true` and no other value can
    /// be serialized.  With a limit of `usize::MAX` the value is always
    /// completed in a single call, which allows serializers to use faster
    /// paths (see [`SerializeDriver::drive_until`]).
    ///
    /// If this fails in the first call for a value, the output and the
    /// state are as before the call (like with [`drive`](Serializer::drive))
    /// and the next value can be serialized.  If it fails after a part was
    /// returned, the bytes of the part cannot be taken back: the value
    /// stays in progress and the stream cannot continue.
    ///
    /// The provided implementation serializes the whole value with
    /// [`drive`](Serializer::drive).
    fn drive_partial(
        &mut self,
        driver: &mut SerializeDriver<'_>,
        limit: usize,
    ) -> Result<Written, Error> {
        let _ = limit;
        self.drive(driver)?;
        Ok(Written::Done)
    }

    /// Returns `true` if a value is partially serialized.
    ///
    /// This is the case after [`drive_partial`](Self::drive_partial)
    /// returned [`Written::Partial`] until it returns [`Written::Done`].
    /// If the value is abandoned (because it failed, or because the caller
    /// gave up on it, for instance when a write failed), this stays `true`:
    /// the output of the stream holds an incomplete value, so the stream
    /// cannot continue.  Serializing another value fails.
    fn in_progress(&self) -> bool {
        false
    }
}

impl<S: StreamSerializer + ?Sized> StreamSerializer for &mut S {
    fn output(&self) -> &[u8] {
        (**self).output()
    }

    fn clear_output(&mut self) {
        (**self).clear_output()
    }

    fn supports_partial(&self) -> bool {
        (**self).supports_partial()
    }

    fn drive_partial(
        &mut self,
        driver: &mut SerializeDriver<'_>,
        limit: usize,
    ) -> Result<Written, Error> {
        (**self).drive_partial(driver, limit)
    }

    fn in_progress(&self) -> bool {
        (**self).in_progress()
    }
}
