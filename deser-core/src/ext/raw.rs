use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::any::Any;
use core::fmt;
use core::marker::PhantomData;

use crate::State;
use crate::adapters::Borrowed;
use crate::de::recording::Capture;
use crate::de::{Deserialize, DeserializeDriver, RecordBuf, Sink, SinkHandle};
use crate::error::{Error, ErrorKind};
use crate::event::Atom;
use crate::ext::{BorrowedExtension, ExtValue};
use crate::ser::{Emit, Serialize, SerializeHandle, SerializeRef};

/// A data format whose encoded values can be held by [`Raw`].
///
/// This is implemented by a type of the crate of the format that stands
/// for the format (for instance `deser_json::Json`).  The format is
/// described at runtime by a [`RawFormatInfo`].
///
/// # Passing on the Input of Values
///
/// Raw values of a format that is deserialized from the same format hold
/// the input of the value, and serializers of the format write them as
/// they are.  For this the format:
///
/// * calls [`State::declare_raw_format`](crate::State::declare_raw_format) with
///   its [`RawFormatId`] in the deserializer (before the first event, it
///   returns the description of the format if the top-level value is
///   wanted as raw value) and in the serializer (before the first value).
/// * checks with [`Error::is_raw_request`](crate::Error::is_raw_request)
///   whether the result of an event requests the next value as raw value
///   and takes the description of the format with
///   [`State::take_raw_request`](crate::State::take_raw_request).
/// * validates a value that is wanted as raw value and emits its input as
///   [`RawInput`] (an [`Atom::Ext`]) with that description rather than
///   its events.
/// * writes the [`RawInput`] of its format as it is when serializing
///   (see [`RawInput::is_format`]).
///
/// The parser and the serializer only refer to the [`RawFormatId`]: the
/// functions of the format (like `replay`, which brings in its parser)
/// are only in programs that use its raw values.
///
/// Formats that do not do this still have raw values: the values are
/// encoded with the format then.
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
/// The format must be described as text (see [`RawFormatId::new`]): its
/// parser only passes on and its encoder only produces valid UTF-8.
pub unsafe trait TextRawFormat: RawFormat {}

/// Identifies a [`RawFormat`].
///
/// Formats are identified by the address of a static of this type.  Unlike
/// the [`RawFormatInfo`] of the format it holds no functions: formats
/// declare which raw values they pass on with it (see
/// [`State::declare_raw_format`](crate::State::declare_raw_format)), so a program
/// only contains the functions of the format (like its parser for
/// `replay`) if it uses its raw values.
pub struct RawFormatId {
    name: &'static str,
    is_text: bool,
}

impl RawFormatId {
    /// Creates the identity of a format.
    ///
    /// * `name` is the name of the format (like `"json"`).
    /// * `is_text` is `true` if the encoding is text.  The encoded values
    ///   must then be valid UTF-8.
    pub const fn new(name: &'static str, is_text: bool) -> RawFormatId {
        RawFormatId { name, is_text }
    }

    /// Returns the name of the format.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Returns `true` if the encoding of the format is text.
    pub fn is_text(&self) -> bool {
        self.is_text
    }
}

impl fmt::Debug for RawFormatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RawFormatId").field(&self.name).finish()
    }
}

/// Describes a [`RawFormat`] at runtime.
///
/// Formats that can pass on the input of values define a static of this
/// type.  It travels with the input of values (see [`RawInput`]) so that
/// code which does not know the format can still parse it.  Formats are
/// identified by their [`RawFormatId`].
pub struct RawFormatInfo {
    id: &'static RawFormatId,
    replay: for<'de> fn(&'de [u8], &mut DeserializeDriver<'_, 'de>) -> Result<(), Error>,
    encode: fn(SerializeRef<'_>) -> Result<Vec<u8>, Error>,
    fallback: for<'v> fn(&'v [u8]) -> Atom<'v>,
    data: Option<&'static (dyn Any + Send + Sync)>,
}

impl RawFormatInfo {
    /// Creates the description of a format.
    ///
    /// * `id` identifies the format.
    /// * `replay` parses a value and emits its events into the driver.
    /// * `encode` encodes a value.
    /// * `fallback` returns the fallback atom of an encoded value (see
    ///   [`Extension::fallback`](crate::ext::Extension::fallback)).  It
    ///   must be [`Atom::Null`] for null, so that optionals are `None` for
    ///   it, and must not be an extension value.
    pub const fn new(
        id: &'static RawFormatId,
        replay: for<'de> fn(&'de [u8], &mut DeserializeDriver<'_, 'de>) -> Result<(), Error>,
        encode: fn(SerializeRef<'_>) -> Result<Vec<u8>, Error>,
        fallback: for<'v> fn(&'v [u8]) -> Atom<'v>,
    ) -> RawFormatInfo {
        RawFormatInfo {
            id,
            replay,
            encode,
            fallback,
            data: None,
        }
    }

    /// Attaches data of the format to the description.
    ///
    /// The format gets it back from the description of raw values that are
    /// requested (see [`data`](Self::data)).  Formats keep what only
    /// programs that use their raw values need here (like the scanner of
    /// raw values in the parser): as only raw values refer to the
    /// description, other programs do not contain it.
    pub const fn set_data(&mut self, data: &'static (dyn Any + Send + Sync)) {
        self.data = Some(data);
    }

    /// Returns the data of the format (see [`set_data`](Self::set_data)).
    pub fn data(&self) -> Option<&'static (dyn Any + Send + Sync)> {
        self.data
    }

    /// Returns the identity of the format.
    pub fn id(&self) -> &'static RawFormatId {
        self.id
    }

    /// Returns the name of the format.
    pub fn name(&self) -> &'static str {
        self.id.name
    }

    /// Returns `true` if the encoding of the format is text.
    pub fn is_text(&self) -> bool {
        self.id.is_text
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
        f.debug_tuple("RawFormatInfo").field(&self.id.name).finish()
    }
}

/// Returns `true` if two descriptions are the same format.
#[inline(always)]
fn same_format(a: &'static RawFormatInfo, b: &'static RawFormatInfo) -> bool {
    core::ptr::eq(a.id, b.id)
}

/// The encoded input of a value.
///
/// This is a well-known borrowing extension (see [`ext`](crate::ext)) which
/// carries the encoding of a value between formats and [`Raw`] values:
///
/// * formats emit it for values that are requested as raw values (the
///   values of [`Raw`] types).  They validate the value and pass on its
///   input instead of its events.
/// * [`Raw`] values emit it when they are serialized and the serializer
///   writes the format as it is (see [`State::declare_raw_format`]).
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
            .is_text()
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

    /// Returns `true` if the value is of the format with the identity.
    ///
    /// Serializers check with this whether they write a value as it is
    /// (unlike [`is`](Self::is), this does not refer to the functions of
    /// the format).
    pub fn is_format(&self, id: &'static RawFormatId) -> bool {
        core::ptr::eq(self.format.id, id)
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
        // the driver requests the value as raw value if `T` wants one
        crate::de::deserialize_value(|driver| (self.format.replay)(&self.bytes, driver))
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
        debug.field("format", &self.format.id.name);
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
fn serialize_input<'a>(input: &'a RawInput<'_>, state: &mut State) -> Result<Emit<'a>, Error> {
    if state.accepts_raw(input.format) {
        return Ok(Emit::Atom(Atom::Ext(ExtValue::borrowed_value::<RawInput>(
            input,
        ))));
    }
    Ok(Emit::Forward(SerializeHandle::arena(
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
) -> Result<Emit<'a>, Error> {
    if let Atom::Ext(ext) = atom
        && let Some(input) = ext.downcast_value_ref::<RawInput>()
    {
        return serialize_input(input, state);
    }
    Ok(Emit::Atom(atom.as_borrowed()))
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
///   [`Borrowed`] adapter the input is
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
        if info.is_text() && core::str::from_utf8(&bytes).is_err() {
            return Err(Error::new(ErrorKind::Syntax, "invalid utf-8"));
        }
        {
            let mut driver = DeserializeDriver::from_fn(|_| SinkHandle::null());
            (info.replay)(&bytes, &mut driver)?;
        }
        // SAFETY: the value was validated
        Ok(Raw::from_input(unsafe { RawInput::new(bytes, info) }))
    }

    /// Encodes a value.
    pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Raw<'static, F>, Error> {
        let info = F::info();
        let bytes = (info.encode)(SerializeRef::new(&value))?;
        if info.is_text() && core::str::from_utf8(&bytes).is_err() {
            return Err(Error::new(
                ErrorKind::InvalidState,
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
        debug.field(&self.input.format.id.name);
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
    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        serialize_input(&value.input, state)
    }
}

/// Raw values are deserialized owned so that `Raw<'static, F>` can be
/// deserialized from any data.  To borrow use the
/// [`Borrowed`] adapter.
impl<'de, 'a, F: RawFormat> Deserialize<'de> for Raw<'a, F> {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        RecordBuf::capture_with(RawCapture(out), state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("any value")
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
impl<'de: 'a, 'a, F: RawFormat> Deserialize<'de, Raw<'a, F>> for Borrowed {
    fn deserialize_into<'out>(
        out: &'out mut Option<Raw<'a, F>>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        RecordBuf::capture_with(BorrowedRawCapture(out), state)
    }

    fn expecting() -> Cow<'static, str> {
        Cow::Borrowed("any value")
    }

    #[inline]
    fn __private_atom_into(
        out: &mut Option<Raw<'a, F>>,
        atom: Atom,
        state: &mut State,
    ) -> Result<(), Error> {
        *out = Some(Raw::from_atom(atom, state)?);
        Ok(())
    }

    #[inline]
    fn __private_borrowed_atom_into(
        out: &mut Option<Raw<'a, F>>,
        atom: Atom<'de>,
        state: &mut State,
    ) -> Result<(), Error> {
        *out = Some(Raw::from_borrowed_atom(atom, state)?);
        Ok(())
    }

    #[inline(always)]
    fn __private_raw() -> Option<&'static RawFormatInfo> {
        Some(F::info())
    }
}

impl<'a, F: RawFormat> Serialize<Raw<'a, F>> for Borrowed {
    fn serialize<'x>(value: &'x Raw<'a, F>, state: &mut State) -> Result<Emit<'x>, Error> {
        <Raw<'a, F>>::serialize(value, state)
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
