use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt;
use core::marker::PhantomData;

use crate::State;
use crate::adapters::{Borrowed, DeserializeAs, SerializeAs};
use crate::de::recording::Capture;
use crate::de::{Deserialize, DeserializeDriver, RecordBuf, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ext::{BorrowedExtension, ExtValue};
use crate::ser::{Chunk, Serialize, SerializeHandle};

/// A data format whose encoded values can be held by [`Raw`].
///
/// This is implemented by a type of the crate of the format that stands
/// for the format (for instance `deser_json::Json`).  The format is
/// described at runtime by a [`RawFormatInfo`].
pub trait RawFormat: 'static {
    /// Returns the description of the format.
    ///
    /// This must always return the same static: formats are identified by
    /// the address of their description.
    fn info() -> &'static RawFormatInfo;
}

/// A [`RawFormat`] whose encoding is text.
///
/// [`Raw`] values of such formats can be accessed as strings (see
/// [`Raw::get`]).
///
/// # Safety
///
/// The format must be described as text (see [`RawFormatInfo::new`]): its
/// parser only passes on and its encoder only produces valid UTF-8.
pub unsafe trait TextRawFormat: RawFormat {}

/// Describes a [`RawFormat`] at runtime.
///
/// Formats that can pass on the input of values define a static of this
/// type.  It travels with the input of values (see [`RawInput`]) so that
/// code which does not know the format can still parse it.
pub struct RawFormatInfo {
    name: &'static str,
    is_text: bool,
    replay: for<'de> fn(&'de [u8], &mut DeserializeDriver<'_, 'de>) -> Result<(), Error>,
    encode: fn(&dyn Serialize) -> Result<Vec<u8>, Error>,
    fallback: for<'v> fn(&'v [u8]) -> Atom<'v>,
}

impl RawFormatInfo {
    /// Creates the description of a format.
    ///
    /// * `name` is the name of the format (like `"json"`).
    /// * `is_text` is `true` if the encoding is text.  The encoded values
    ///   must then be valid UTF-8.
    /// * `replay` parses a value and emits its events into the driver.
    /// * `encode` encodes a value.
    /// * `fallback` returns the fallback atom of an encoded value (see
    ///   [`Extension::fallback`](crate::ext::Extension::fallback)).  It
    ///   must be [`Atom::Null`] for null, so that optionals are `None` for
    ///   it, and must not be an extension value.
    pub const fn new(
        name: &'static str,
        is_text: bool,
        replay: for<'de> fn(&'de [u8], &mut DeserializeDriver<'_, 'de>) -> Result<(), Error>,
        encode: fn(&dyn Serialize) -> Result<Vec<u8>, Error>,
        fallback: for<'v> fn(&'v [u8]) -> Atom<'v>,
    ) -> RawFormatInfo {
        RawFormatInfo {
            name,
            is_text,
            replay,
            encode,
            fallback,
        }
    }

    /// Returns the name of the format.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns `true` if the encoding of the format is text.
    pub fn is_text(&self) -> bool {
        self.is_text
    }

    /// Records an encoded value.
    fn record<'a>(&self, bytes: &'a [u8]) -> Result<RecordBuf<'a>, Error> {
        let mut recording = RecordBuf::new();
        {
            let mut driver = DeserializeDriver::from_fn(|state| recording.recorder(state));
            (self.replay)(bytes, &mut driver)?;
        }
        Ok(recording)
    }
}

impl fmt::Debug for RawFormatInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RawFormatInfo").field(&self.name).finish()
    }
}

/// Returns `true` if two descriptions are the same format.
#[inline(always)]
fn same_format(a: &'static RawFormatInfo, b: &'static RawFormatInfo) -> bool {
    core::ptr::eq(a, b)
}

/// The encoded input of a value.
///
/// This is a well-known borrowing extension (see [`ext`](crate::ext)) which
/// carries the encoding of a value between formats and [`Raw`] values:
///
/// * formats emit it for values that are requested as raw values (see
///   [`Deserialize::__private_raw`]).  They validate the value and pass on
///   its input instead of its events.
/// * [`Raw`] values emit it when they are serialized and the serializer
///   writes the format as it is (see [`State::__private_accept_raw`]).
///
/// It knows its format, so it's parsed into its value where it ends up in
/// something that does not know it (for instance when a recording that
/// holds it is serialized).
#[derive(Clone)]
pub struct RawInput<'a> {
    bytes: Cow<'a, [u8]>,
    format: &'static RawFormatInfo,
}

impl<'a> RawInput<'a> {
    /// Creates the input of a value.
    ///
    /// # Safety
    ///
    /// The input must be a single, valid value of the format.  If the
    /// format is text, it must be valid UTF-8.  Serializers of the format
    /// write the input as it is.
    pub unsafe fn new<B: Into<Cow<'a, [u8]>>>(
        bytes: B,
        format: &'static RawFormatInfo,
    ) -> RawInput<'a> {
        RawInput {
            bytes: bytes.into(),
            format,
        }
    }

    /// Returns the encoded value.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the encoded value as text if the format is text.
    pub fn as_str(&self) -> Option<&str> {
        // SAFETY: the input of text formats is valid UTF-8, see `new`
        self.format
            .is_text
            .then(|| unsafe { core::str::from_utf8_unchecked(&self.bytes) })
    }

    /// Returns the format of the value.
    pub fn format(&self) -> &'static RawFormatInfo {
        self.format
    }

    /// Returns `true` if the value is of the format `F`.
    pub fn is<F: RawFormat>(&self) -> bool {
        same_format(self.format, F::info())
    }

    /// Detaches the input from the data it borrows.
    pub fn into_owned(self) -> RawInput<'static> {
        RawInput {
            bytes: Cow::Owned(self.bytes.into_owned()),
            format: self.format,
        }
    }

    /// Records the value.
    pub fn record(&self) -> Result<RecordBuf<'_>, Error> {
        self.format.record(&self.bytes)
    }

    /// Replays the value into a sink.
    ///
    /// The value is parsed, the sink can borrow its data.
    pub fn replay<'x>(&'x self, sink: SinkHandle<'_, 'x>, state: &mut State) -> Result<(), Error> {
        // the type of the sink is unknown, it does not want a raw value
        self.replay_raw(sink, None, state)
    }

    /// Replays the value, `raw` is the raw value the sink wants.
    fn replay_raw<'x>(
        &'x self,
        sink: SinkHandle<'_, 'x>,
        raw: Option<&'static RawFormatInfo>,
        state: &mut State,
    ) -> Result<(), Error> {
        DeserializeDriver::nested(state, sink, false, |driver| {
            // the top-level value is requested before it starts
            driver.state_mut().raw_requested = raw;
            (self.format.replay)(&self.bytes, driver)
        })
    }

    /// Deserializes the value.
    ///
    /// The value can borrow from the input (and the data it borrows).
    pub fn deserialize<'x, T: Deserialize<'x>>(&'x self) -> Result<T, Error> {
        crate::de::deserializer::deserialize_value(|make_sink| {
            let mut state = State::new();
            let sink = make_sink(&mut state);
            self.replay_raw(sink, T::__private_raw(), &mut state)
        })
    }
}

/// Parses the input of a raw value into a sink that does not accept it.
///
/// The sink is finished by the caller (see `NoFinish`).
pub(crate) fn parse_into<'de>(
    input: &RawInput<'_>,
    sink: &mut (dyn Sink<'de> + '_),
    state: &mut State,
) -> Result<(), Error> {
    let mut sink = NoFinish(sink);
    DeserializeDriver::nested(state, SinkHandle::to(&mut sink), false, |driver| {
        driver.state_mut().raw_requested = None;
        // the input lives shorter than the data of the sink
        driver.transient(|driver| (input.format.replay)(&input.bytes, driver))
    })
}

/// Forwards to a sink except for [`Sink::finish`].
///
/// This is used to parse a value into a sink that received an atom (see
/// `parse_into`): the replay finishes the value, the sink is finished by
/// the code that delivered the atom.
struct NoFinish<'a, 'b, 'de>(&'a mut (dyn Sink<'de> + 'b));

impl<'de> Sink<'de> for NoFinish<'_, '_, 'de> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        self.0.atom(atom, state)
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        self.0.borrowed_atom(atom, state)
    }

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.0.map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.0.seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.0.next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.0.next_value(state)
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &mut State,
    ) -> Result<Option<SinkHandle<'_, 'de>>, Error> {
        self.0.value_for_key(key, state)
    }

    fn recover(&mut self, err: Error, state: &mut State) -> Result<(), Error> {
        self.0.recover(err, state)
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.0.expecting()
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }
}

impl fmt::Debug for RawInput<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("RawInput");
        debug.field("format", &self.format.name);
        match self.as_str() {
            Some(text) => debug.field("text", &text),
            None => debug.field("bytes", &self.bytes),
        };
        debug.finish()
    }
}

impl PartialEq for RawInput<'_> {
    fn eq(&self, other: &Self) -> bool {
        same_format(self.format, other.format) && self.bytes == other.bytes
    }
}

impl BorrowedExtension for RawInput<'static> {
    type Value<'a> = RawInput<'a>;

    fn name<'v>(_value: &'v RawInput<'_>) -> &'v str {
        "raw value"
    }

    fn fallback<'v>(value: &'v RawInput<'_>) -> Atom<'v> {
        (value.format.fallback)(&value.bytes)
    }

    fn to_static(value: &RawInput<'_>) -> RawInput<'static> {
        value.clone().into_owned()
    }

    fn shorten<'s, 'l: 's>(value: &'s RawInput<'l>) -> &'s RawInput<'s> {
        value
    }
}

/// Serializes the input of a value.
///
/// The input is passed on as extension value if the serializer writes its
/// format as it is, otherwise the value is parsed and serialized.
fn serialize_input<'a>(input: &'a RawInput<'_>, state: &mut State) -> Result<Chunk<'a>, Error> {
    if state.accepts_raw(input.format) {
        return Ok(Chunk::Atom(Atom::Ext(
            ExtValue::borrowed_value::<RawInput>(input),
        )));
    }
    Ok(Chunk::Forward(SerializeHandle::arena(
        input.record()?,
        state,
    )))
}

/// Serializes an atom of a recording.
///
/// The input of values that the serializer does not write as it is is
/// parsed and serialized.
pub(crate) fn serialize_recorded_atom<'a>(
    atom: &'a Atom<'_>,
    state: &mut State,
) -> Result<Chunk<'a>, Error> {
    if let Atom::Ext(ext) = atom
        && let Some(input) = ext.downcast_value_ref::<RawInput>()
    {
        return serialize_input(input, state);
    }
    Ok(Chunk::Atom(atom.as_borrowed()))
}

/// A value encoded in the format `F`.
///
/// A raw value holds a value in the encoding of a format, for instance
/// JSON text with `deser_json::RawJson`.  It can be deserialized later
/// (with [`deserialize`](Self::deserialize)), stored or written out again
/// unchanged:
///
/// * If it's deserialized from the format `F`, it holds the input of the
///   value as it is.  The format only validates the value and does not
///   produce its events, which is fast.  With the
///   [`Borrowed`](crate::adapters::Borrowed) adapter the input is
///   borrowed.
/// * Otherwise (from another format or where the format cannot pass on
///   the input, see below) the value is encoded in the format `F`.  The
///   encoding can lose information that `F` cannot express (bytes in JSON
///   for instance).  To keep a value of any format without interpreting
///   it, use [`Recording`](crate::de::Recording).
///
/// When serialized with the format `F`, the encoded value is written as it
/// is.  Other formats serialize the value it holds.
///
/// The input of values can only be passed on if the format knows before
/// the value starts that a raw value is wanted.  Derived structs (for their
/// first 64 fields), maps, sequences, `Option` and `Box` ask for it.
/// Values in other places (like the fields of flattened structs, the
/// variants of enums and values that are buffered, for instance for
/// untagged enums) are encoded.
///
/// Like `Cow`, raw values are deserialized owned, so a `Raw<'static, F>`
/// can be deserialized from any data.
pub struct Raw<'a, F: RawFormat> {
    input: RawInput<'a>,
    _format: PhantomData<fn() -> F>,
}

impl<'a, F: RawFormat> Raw<'a, F> {
    /// Creates a raw value from its encoding.
    ///
    /// The value is validated.
    pub fn new<B: Into<Cow<'a, [u8]>>>(bytes: B) -> Result<Raw<'a, F>, Error> {
        let bytes = bytes.into();
        let info = F::info();
        if info.is_text && core::str::from_utf8(&bytes).is_err() {
            return Err(Error::new(ErrorKind::Unexpected, "invalid utf-8"));
        }
        {
            let mut driver = DeserializeDriver::from_sink(SinkHandle::null());
            (info.replay)(&bytes, &mut driver)?;
        }
        // SAFETY: the value was validated
        Ok(Raw::from_input(unsafe { RawInput::new(bytes, info) }))
    }

    /// Encodes a value.
    pub fn encode(value: &dyn Serialize) -> Result<Raw<'static, F>, Error> {
        let info = F::info();
        let bytes = (info.encode)(value)?;
        if info.is_text && core::str::from_utf8(&bytes).is_err() {
            return Err(Error::new(
                ErrorKind::Unexpected,
                "the encoding of a text format is not utf-8",
            ));
        }
        // SAFETY: the encoder produces a valid value
        Ok(Raw::from_input(unsafe { RawInput::new(bytes, info) }))
    }

    fn from_input(input: RawInput<'a>) -> Raw<'a, F> {
        debug_assert!(input.is::<F>());
        Raw {
            input,
            _format: PhantomData,
        }
    }

    /// Returns the encoded value.
    pub fn as_bytes(&self) -> &[u8] {
        &self.input.bytes
    }

    /// Returns `true` if the value borrows from the data it was
    /// deserialized from.
    pub fn is_borrowed(&self) -> bool {
        matches!(self.input.bytes, Cow::Borrowed(_))
    }

    /// Replays the value into a sink.
    ///
    /// The value is parsed, the sink can borrow its data.
    pub fn replay<'x>(&'x self, sink: SinkHandle<'_, 'x>, state: &mut State) -> Result<(), Error> {
        self.input.replay(sink, state)
    }

    /// Deserializes the value.
    ///
    /// The value can borrow from the raw value (and the data it borrows).
    pub fn deserialize<'x, T: Deserialize<'x>>(&'x self) -> Result<T, Error> {
        self.input.deserialize()
    }

    /// Detaches the value from the data it borrows.
    pub fn into_owned(self) -> Raw<'static, F> {
        Raw::from_input(self.input.into_owned())
    }

    /// Creates a raw value from an atom that was delivered for it.
    #[inline(never)]
    fn from_atom(atom: Atom<'_>, state: &State) -> Result<Raw<'static, F>, Error> {
        if let Atom::Ext(ref ext) = atom
            && let Some(input) = ext.downcast_value_ref::<RawInput>()
        {
            if input.is::<F>() {
                return Ok(Raw::from_input(input.clone().into_owned()));
            }
            return Raw::encode(&input.record()?);
        }
        let mut recording = RecordBuf::new();
        recording.set_atom(&atom, state);
        Raw::encode(&recording)
    }
}

impl<'de, F: RawFormat> Raw<'de, F> {
    /// Creates a raw value from an atom that was delivered borrowed for it.
    #[inline(never)]
    fn from_borrowed_atom(atom: Atom<'de>, state: &State) -> Result<Raw<'de, F>, Error> {
        if let Atom::Ext(ref ext) = atom {
            // SAFETY: raw inputs are covariant in their lifetime
            if let Some(input) = unsafe { ext.downcast_value_ref_covariant::<RawInput>() }
                && input.is::<F>()
            {
                return Ok(Raw::from_input(input.clone()));
            }
        }
        Raw::from_atom(atom, state)
    }
}

impl<F: TextRawFormat> Raw<'_, F> {
    /// Returns the encoded value as text.
    pub fn get(&self) -> &str {
        // SAFETY: the encoding of text formats is valid UTF-8
        unsafe { core::str::from_utf8_unchecked(&self.input.bytes) }
    }
}

impl<F: RawFormat> Clone for Raw<'_, F> {
    fn clone(&self) -> Self {
        Raw::from_input(self.input.clone())
    }
}

impl<F: RawFormat> fmt::Debug for Raw<'_, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_tuple("Raw");
        debug.field(&self.input.format.name);
        match self.input.as_str() {
            Some(text) => debug.field(&text),
            None => debug.field(&self.input.bytes),
        };
        debug.finish()
    }
}

/// Raw values are equal if their encodings are equal.
impl<F: RawFormat> PartialEq for Raw<'_, F> {
    fn eq(&self, other: &Self) -> bool {
        self.input.bytes == other.input.bytes
    }
}

impl<F: RawFormat> Eq for Raw<'_, F> {}

impl<F: RawFormat> core::hash::Hash for Raw<'_, F> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.input.bytes.hash(state)
    }
}

/// Serialized with the format `F`, the encoded value is written as it is.
/// Other formats serialize the value it holds.
impl<F: RawFormat> Serialize for Raw<'_, F> {
    fn serialize(&self, state: &mut State) -> Result<Chunk<'_>, Error> {
        serialize_input(&self.input, state)
    }
}

/// Raw values are deserialized owned so that `Raw<'static, F>` can be
/// deserialized from any data.  To borrow use the
/// [`Borrowed`](crate::adapters::Borrowed) adapter.
impl<'de, 'a, F: RawFormat> Deserialize<'de> for Raw<'a, F> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        RecordBuf::capture_with(RawCapture(out), state)
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<Self>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        *out = Some(Raw::from_atom(atom, state)?);
        Ok(())
    }

    #[inline]
    fn __private_borrowed_atom_into(
        out: &mut Option<Self>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        *out = Some(Raw::from_atom(atom, state)?);
        Ok(())
    }

    #[inline(always)]
    fn __private_raw() -> Option<&'static RawFormatInfo> {
        Some(F::info())
    }
}

/// Borrows raw values from the data if the format passes it on borrowed.
impl<'de: 'a, 'a, F: RawFormat> DeserializeAs<'de, Raw<'a, F>> for Borrowed {
    fn deserialize_into_as<'out>(
        out: &'out mut Option<Raw<'a, F>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        RecordBuf::capture_with(BorrowedRawCapture(out), state)
    }

    #[inline]
    fn __private_atom_into_as(
        out: &mut Option<Raw<'a, F>>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        *out = Some(Raw::from_atom(atom, state)?);
        Ok(())
    }

    #[inline]
    fn __private_borrowed_atom_into_as(
        out: &mut Option<Raw<'a, F>>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        *out = Some(Raw::from_borrowed_atom(atom, state)?);
        Ok(())
    }

    #[inline(always)]
    fn __private_raw_as() -> Option<&'static RawFormatInfo> {
        Some(F::info())
    }
}

impl<'a, F: RawFormat> SerializeAs<Raw<'a, F>> for Borrowed {
    fn serialize_as<'x>(value: &'x Raw<'a, F>, state: &mut State) -> Result<Chunk<'x>, Error> {
        value.serialize(state)
    }
}

/// Places a captured value into the slot of a raw value, owned.
struct RawCapture<'o, 'a, F: RawFormat>(&'o mut Option<Raw<'a, F>>);

impl<'o, 'a, 'de, F: RawFormat> Capture<'de, RecordBuf<'de>> for RawCapture<'o, 'a, F> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        *self.0 = Some(Raw::from_atom(atom, state)?);
        Ok(())
    }

    fn recorded(&mut self, recording: RecordBuf<'de>, _state: &mut State) -> Result<(), Error> {
        *self.0 = Some(Raw::encode(&recording)?);
        Ok(())
    }
}

/// Places a captured value into the slot of a raw value, borrowed.
struct BorrowedRawCapture<'o, 'a, F: RawFormat>(&'o mut Option<Raw<'a, F>>);

impl<'o, 'a, 'de: 'a, F: RawFormat> Capture<'de, RecordBuf<'de>> for BorrowedRawCapture<'o, 'a, F> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        *self.0 = Some(Raw::from_atom(atom, state)?);
        Ok(())
    }

    fn borrowed_atom(&mut self, atom: Atom<'de>, state: &mut State) -> Result<(), Error> {
        *self.0 = Some(Raw::from_borrowed_atom(atom, state)?);
        Ok(())
    }

    fn recorded(&mut self, recording: RecordBuf<'de>, _state: &mut State) -> Result<(), Error> {
        *self.0 = Some(Raw::encode(&recording)?);
        Ok(())
    }
}
