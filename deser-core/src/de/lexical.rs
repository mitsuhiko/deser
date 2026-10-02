//! Parsing of lexical atoms.
//!
//! See [`Atom::Lexical`](crate::Atom::Lexical) and [`LexicalRules`].
use alloc::format;
use alloc::string::String;
use core::fmt::Write;
use core::num::{IntErrorKind, ParseIntError};

use crate::State;
use crate::error::{Error, ErrorKind, discarded_error};

/// The longest part of a value that is included in error messages.
const MAX_QUOTED: usize = 64;

/// How [lexical atoms](crate::Atom::Lexical) are interpreted.
///
/// Lexical atoms are text whose type the format cannot express.  The types
/// they are delivered to interpret them: numbers and booleans parse them,
/// strings take them as they are.  What text means beyond that depends on
/// where it comes from.  Keys of JSON objects are strict (a `bool` key is
/// `true` or `false`), everything in a query string or an environment
/// variable is text so `on` and `yes` are booleans too and empty values are
/// missing values.
///
/// The rules are an extension value in the [`State`] (see
/// [`State::get_mut`]) which formats set, the default are the
/// [strict](Self::STRICT) rules.  As they are part of the state (and not of
/// the atoms), lexical atoms that are buffered and replayed are
/// interpreted with the rules of the deserialization they are replayed in.
///
/// ```
/// use deser::de::{DeserializeDriver, LexicalRules};
/// use deser::{Atom, Text};
///
/// let mut out = None::<bool>;
/// let mut driver = DeserializeDriver::new(&mut out);
/// LexicalRules::LENIENT.set(driver.state_mut());
/// driver.emit(Atom::Lexical(Text::borrowed("on"))).unwrap();
/// drop(driver);
/// assert_eq!(out, Some(true));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexicalRules {
    lenient_bools: bool,
    empty_is_null: bool,
}

impl LexicalRules {
    /// The rules for text that happens to be text, like the keys of JSON
    /// objects.  This is the default.
    ///
    /// * booleans are `true` and `false`
    /// * integers and floats are parsed with [`str::parse`]
    /// * empty text is not a missing value
    pub const STRICT: LexicalRules = LexicalRules {
        lenient_bools: false,
        empty_is_null: false,
    };

    /// The rules for formats where everything is text, like query strings
    /// and environment variables.
    ///
    /// * booleans are `true`, `yes`, `on` and `1` and `false`, `no`, `off`
    ///   and `0` (ignoring ASCII case)
    /// * integers and floats are parsed with [`str::parse`]
    /// * empty text is a missing value for types that do not accept it:
    ///   `None` for an `Option<u32>`, `Some("")` for an `Option<String>`
    ///   and `()`
    ///
    /// These formats typically also allow keys to repeat, a key given once
    /// can then stand for a sequence of one value.  That is not a rule of
    /// the text but of the maps they emit (see
    /// [`ContainerShape::with_multimap`](crate::ContainerShape::with_multimap)).
    pub const LENIENT: LexicalRules = LexicalRules {
        lenient_bools: true,
        empty_is_null: true,
    };

    /// Returns the rules of a deserialization.
    #[inline]
    pub fn of(state: &State) -> LexicalRules {
        state.get::<LexicalRules>().copied().unwrap_or_default()
    }

    /// Sets the rules of a deserialization.
    #[inline]
    pub fn set(self, state: &mut State) {
        *state.get_mut::<LexicalRules>() = self;
    }

    /// Sets if booleans are also `yes`, `on` and `1` and `no`, `off` and
    /// `0` (ignoring ASCII case).
    pub const fn with_lenient_bools(mut self, yes: bool) -> LexicalRules {
        self.lenient_bools = yes;
        self
    }

    /// Sets if empty text is a missing value for types that do not accept
    /// it.
    pub const fn with_empty_is_null(mut self, yes: bool) -> LexicalRules {
        self.empty_is_null = yes;
        self
    }
}

impl Default for LexicalRules {
    fn default() -> LexicalRules {
        LexicalRules::STRICT
    }
}

/// The key under which maps hold their own content.
///
/// In some formats a value is text or a map that holds the text together
/// with more entries: the elements of XML are their text (`<count>3</count>`)
/// or, if they have attributes, a map (`<count unit="m">3</count>` is
/// `{"@unit": "m", "$text": "3"}`).  Which of the two a value is depends on
/// the input, not on the type it's deserialized into.  Formats like this
/// set the key of the content in the [`State`] (see [`set`](Self::set)),
/// and values are then passed on in the form the type accepts:
///
/// * A type that rejects a map (like `u32`) receives the value of the key
///   of the content, the other entries are skipped.  If the map has no
///   such key it receives empty text (`""` as
///   [lexical atom](crate::Atom::Lexical)).
/// * A type that rejects [lexical atoms](crate::Atom::Lexical) but accepts
///   maps (like a struct) receives a map with the text under the key of the
///   content (no entry for empty text).
///
/// Both only happen after a type rejected the value, so they cost nothing
/// otherwise.  Without the key (the default), values are passed on as
/// they are.
///
/// ```
/// use deser::de::{ContentKey, DeserializeDriver};
/// use deser::{Atom, Deserialize, Event};
///
/// #[derive(Deserialize, Debug, PartialEq)]
/// struct Price {
///     #[deser(rename = "@currency")]
///     currency: Option<String>,
///     #[deser(rename = "$text")]
///     amount: u32,
/// }
///
/// let mut out = None::<(u32, Price)>;
/// let mut driver = DeserializeDriver::new(&mut out);
/// ContentKey("$text").set(driver.state_mut());
/// driver.emit(Event::seq_start()).unwrap();
/// // a map for a `u32`
/// driver.emit(Event::map_start()).unwrap();
/// for text in ["@unit", "m", "$text", "3"] {
///     driver.emit(Atom::Lexical(text.into())).unwrap();
/// }
/// driver.emit(Event::MapEnd).unwrap();
/// // text for a struct
/// driver.emit(Atom::Lexical("12".into())).unwrap();
/// driver.emit(Event::SeqEnd).unwrap();
/// drop(driver);
/// assert_eq!(out, Some((3, Price { currency: None, amount: 12 })));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentKey(pub &'static str);

impl ContentKey {
    /// Returns the key of the content of a deserialization if there is one.
    #[inline]
    pub fn of(state: &State) -> Option<&'static str> {
        Some(state.content_key).filter(|key| !key.is_empty())
    }

    /// Sets the key of the content of a deserialization.
    ///
    /// The empty key means that there is no key of the content (the
    /// default).
    #[inline]
    pub fn set(self, state: &mut State) {
        state.content_key = self.0;
    }
}

/// Parses the lexical form of a boolean with the rules of the state.
pub(crate) fn parse_bool(value: &str, state: &State) -> Result<bool, Error> {
    parse_bool_with(value, LexicalRules::of(state).lenient_bools, state)
}

/// Parses the lexical form of a boolean.
///
/// If `lenient` is set, the spellings of booleans in query strings,
/// environment variables and command lines are accepted, ignoring ASCII
/// case.
pub(crate) fn parse_bool_with(value: &str, lenient: bool, state: &State) -> Result<bool, Error> {
    // the common spellings first
    match value {
        "true" => return Ok(true),
        "false" => return Ok(false),
        _ => {}
    }
    if lenient {
        const TRUE: [&str; 4] = ["true", "yes", "on", "1"];
        const FALSE: [&str; 4] = ["false", "no", "off", "0"];
        if TRUE.iter().any(|x| x.eq_ignore_ascii_case(value)) {
            Ok(true)
        } else if FALSE.iter().any(|x| x.eq_ignore_ascii_case(value)) {
            Ok(false)
        } else {
            Err(invalid(
                value,
                "bool (true, yes, on, 1, false, no, off or 0)",
                state,
            ))
        }
    } else {
        Err(invalid(value, "bool (true or false)", state))
    }
}

/// Returns `true` if the text of a lexical atom is empty and empty text is
/// a missing value.
#[inline]
pub(crate) fn is_empty_null(value: &str, state: &State) -> bool {
    value.is_empty() && LexicalRules::of(state).empty_is_null
}

/// Converts the error of parsing an integer.
#[cold]
pub(crate) fn int_error(value: &str, err: ParseIntError, expecting: &str, state: &State) -> Error {
    let kind = match err.kind() {
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => ErrorKind::OutOfRange,
        _ => ErrorKind::InvalidValue,
    };
    if state.discards_errors {
        return discarded_error(kind);
    }
    Error::new(kind, invalid_message(value, expecting))
}

/// Creates the error for a lexical atom that cannot be parsed.
#[cold]
pub(crate) fn invalid(value: &str, expecting: &str, state: &State) -> Error {
    if state.discards_errors {
        return discarded_error(ErrorKind::InvalidValue);
    }
    Error::new(ErrorKind::InvalidValue, invalid_message(value, expecting))
}

fn invalid_message(value: &str, expecting: &str) -> String {
    let mut msg = String::from("invalid value ");
    match value.char_indices().nth(MAX_QUOTED) {
        Some((end, _)) => write!(msg, "{:?}...", &value[..end]).unwrap(),
        None => write!(msg, "{:?}", value).unwrap(),
    }
    write!(msg, ", expected {}", expecting).unwrap();
    msg
}

/// Creates the error for a number that does not fit into the type.
#[cold]
pub(crate) fn out_of_range(
    value: &dyn core::fmt::Display,
    expecting: &str,
    state: &State,
) -> Error {
    if state.discards_errors {
        return discarded_error(ErrorKind::OutOfRange);
    }
    Error::new(
        ErrorKind::OutOfRange,
        format!("invalid value {}, expected {}", value, expecting),
    )
}

#[test]
fn test_parse_bool() {
    let state = State::new();
    for value in ["true", "TRUE", "Yes", "on", "1"] {
        assert!(parse_bool_with(value, true, &state).unwrap(), "{}", value);
    }
    for value in ["false", "False", "NO", "off", "0"] {
        assert!(!parse_bool_with(value, true, &state).unwrap(), "{}", value);
    }
    for value in ["", "2", "y", "n", "t", "truee", " true"] {
        assert!(parse_bool_with(value, true, &state).is_err(), "{}", value);
    }
    assert!(parse_bool(" true", &state).is_err());
    assert!(parse_bool("true", &state).unwrap());
    assert!(!parse_bool("false", &state).unwrap());
    for value in ["TRUE", "yes", "on", "1", "0", "off"] {
        assert!(parse_bool(value, &state).is_err(), "{}", value);
    }
}

#[test]
fn test_invalid_truncates() {
    let err = invalid(&"x".repeat(100), "u32", &State::new());
    assert_eq!(
        err.message(),
        format!("invalid value {:?}..., expected u32", "x".repeat(64))
    );
}
