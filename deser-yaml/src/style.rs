//! Hints for the style of YAML scalars.
//!
//! How a scalar is written is decided by the serializer (see
//! [`SerializerConfig`](crate::SerializerConfig)).  Values can ask for a
//! specific style with a [`ScalarStyle`] hint, which is
//! [event data](deser::State::event) of the value.  The adapters of this
//! module set it, [layers](deser::ser::Layer) can set it too (for instance
//! by path).  Hints only apply to strings and are preferences: if a string
//! cannot be written in the requested style without changing it, it's
//! written in a style that can represent it.
//!
//! ```
//! use deser::Serialize;
//! use deser_yaml::style::{DoubleQuoted, Literal};
//!
//! #[derive(Serialize)]
//! struct Config {
//!     #[deser(as = DoubleQuoted)]
//!     name: String,
//!     #[deser(as = Literal)]
//!     script: String,
//! }
//!
//! let config = Config {
//!     name: "web".into(),
//!     script: "echo hello".into(),
//! };
//! assert_eq!(
//!     deser_yaml::to_string(&config).unwrap(),
//!     "name: \"web\"\nscript: |-\n  echo hello\n"
//! );
//! ```
//!
//! How collections are laid out (flow or block) is controlled with the
//! well-known [`Layout`](deser::hints::Layout) hint.
use deser::State;
use deser::adapters::Same;
use deser::hints::{Hint, Hinted};

/// The style of a scalar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ScalarStyle {
    /// Plain (`value`), if the string does not need quotes.
    Plain,
    /// Single-quoted (`'value'`), if the string does not need escapes.
    SingleQuoted,
    /// Double-quoted (`"value"`).
    DoubleQuoted,
    /// A literal block scalar (`|`), if the string only contains characters
    /// that can be written in it.
    Literal,
    /// A folded block scalar (`>`), long lines are folded at spaces.  If the
    /// string cannot be folded without changing it, it's written as literal
    /// block scalar.
    Folded,
}

/// The scalar style of the current event, attached as event data.
#[derive(Debug, Default, Clone)]
pub(crate) struct StyleHint(pub(crate) Option<ScalarStyle>);

impl ScalarStyle {
    /// Returns the style requested for the current event.
    #[inline]
    pub fn of(state: &State) -> Option<ScalarStyle> {
        if !state.has_event_data() {
            return None;
        }
        state.event::<StyleHint>().and_then(|x| x.0)
    }

    /// Requests the style for the value that is serialized.
    ///
    /// This is intended to be called from
    /// [`Serialize::serialize`](deser::Serialize::serialize) or a
    /// [`Layer`](deser::ser::Layer).
    #[inline]
    pub fn set(self, state: &mut State) {
        state.event_mut::<StyleHint>().0 = Some(self);
    }
}

macro_rules! style_adapter {
    ($(#[$meta:meta])* $alias:ident, $hint:ident, $style:expr) => {
        #[doc = concat!("The [`Hint`] for [`", stringify!($style), "`].")]
        pub struct $hint;

        impl Hint for $hint {
            #[inline]
            fn set(state: &mut State) {
                $style.set(state);
            }
        }

        $(#[$meta])*
        ///
        /// This is an adapter for all types (see [`Hinted`]), `A` is the
        /// adapter used for the value.
        pub type $alias<A = Same> = Hinted<$hint, A>;
    };
}

style_adapter!(
    /// Requests [`ScalarStyle::Plain`] for strings.
    Plain,
    PlainStyle,
    ScalarStyle::Plain
);
style_adapter!(
    /// Requests [`ScalarStyle::SingleQuoted`] for strings.
    SingleQuoted,
    SingleQuotedStyle,
    ScalarStyle::SingleQuoted
);
style_adapter!(
    /// Requests [`ScalarStyle::DoubleQuoted`] for strings.
    DoubleQuoted,
    DoubleQuotedStyle,
    ScalarStyle::DoubleQuoted
);
style_adapter!(
    /// Requests [`ScalarStyle::Literal`] for strings.
    Literal,
    LiteralStyle,
    ScalarStyle::Literal
);
style_adapter!(
    /// Requests [`ScalarStyle::Folded`] for strings.
    Folded,
    FoldedStyle,
    ScalarStyle::Folded
);
