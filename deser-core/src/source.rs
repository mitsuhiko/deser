use alloc::sync::Arc;
use core::fmt;

use crate::State;

/// The source the input ranges refer to.
///
/// Formats can publish the byte range in the input of every event (see
/// [`State::input_range`](crate::State::input_range)).  This is cheap, but
/// resolving the ranges into lines and columns (see
/// [`Position::of`](crate::Position::of)) requires the source.  As
/// this requires a copy of the input, formats only provide it when asked to
/// (for instance with their `track_locations` option).  They store it in
/// the [`State`] as an extension value with [`set`](Self::set) before they
/// emit the first event:
///
/// ```
/// use deser::de::DeserializeDriver;
/// use deser::Source;
///
/// let mut out = None::<bool>;
/// let mut driver = DeserializeDriver::new(&mut out);
/// Source("true".into()).set(driver.state_mut());
/// assert_eq!(&*driver.state().get::<Source>().unwrap().0, "true");
/// ```
#[derive(Clone, Default)]
pub struct Source(pub Arc<str>);

impl Source {
    /// Sets the source in the state.
    #[inline]
    pub fn set(self, state: &mut State) {
        *state.get_mut::<Source>() = self;
    }
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // the source can be large, it's not included
        f.debug_struct("Source")
            .field("len", &self.0.len())
            .finish()
    }
}
