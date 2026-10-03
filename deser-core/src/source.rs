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
/// with [`TrackLocations`].  They store it in the [`State`] as an extension
/// value with [`set`](Self::set) before they emit the first event:
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

/// Asks the formats to provide the [`Source`] (a value of the
/// [`Context`](crate::Context)).
///
/// Formats publish the byte range of every event, resolving them into
/// lines and columns (for instance with the `Spanned` type of
/// [`deser-location`](https://docs.rs/deser-location)) also requires the
/// source, which is a copy of the input.  Formats only provide it if this
/// is set to `true` in the context (or the state).  The errors a
/// deserialization fails with have their line and column either way, but
/// for instance the keys collected with
/// [`UnknownFields::Collect`](crate::de::UnknownFields::Collect) only
/// have them with the source:
///
/// ```
/// use deser::de::{Deserializer, IgnoredFields, UnknownFields};
/// use deser::{Context, Deserialize, TrackLocations};
///
/// #[derive(Deserialize)]
/// struct Config {
///     name: String,
/// }
///
/// let config = deser_json::DeserializerConfig::builder()
///     .context(Context::with(TrackLocations(true)))
///     .build();
/// let input = "{\n  \"name\": \"demo\",\n  \"nmae\": \"x\"\n}";
/// let ignored = IgnoredFields::new();
/// deser_json::Deserializer::from_str_with_config(input, config)
///     .deserialize_with::<Config, _>(|driver| {
///         UnknownFields::Collect(ignored.clone()).set(driver.state_mut())
///     })
///     .unwrap();
/// let ignored = ignored.take();
/// assert_eq!((ignored[0].line(), ignored[0].column()), (Some(3), Some(3)));
/// ```
///
/// Formats check this with [`of`](Self::of) and set the [`Source`] before
/// they emit the first event:
///
/// ```
/// use deser::de::{DeserializeDriver, Deserializer};
/// use deser::{Error, Source, TrackLocations};
///
/// /// A format which provides the source if asked to.
/// struct Text<'a>(&'a str);
///
/// impl<'de> Deserializer<'de> for Text<'de> {
///     fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
///         if TrackLocations::of(driver.state()) {
///             Source(self.0.into()).set(driver.state_mut());
///         }
///         driver.state_mut().set_input_range(0, self.0.len());
///         driver.emit(self.0)
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrackLocations(pub bool);

impl TrackLocations {
    /// Returns `true` if the formats provide the source.
    // not inlined: formats read it once per value
    #[inline(never)]
    pub fn of(state: &State) -> bool {
        state.get::<TrackLocations>().is_some_and(|track| track.0)
    }

    /// Sets if the formats provide the source.
    #[inline]
    pub fn set(self, state: &mut State) {
        *state.get_mut::<TrackLocations>() = self;
    }
}
