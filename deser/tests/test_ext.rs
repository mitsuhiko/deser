use deser::de::{DeserializeDriver, DeserializerState, Sink, SinkHandle};
use deser::ext::{ExtValue, Extension};
use deser::ser::{Chunk, SerializeDriver, SerializerState};
use deser::{make_slot_wrapper, Atom, Deserialize, Error, ErrorKind, Event, Serialize};

fn capture_events(s: &dyn Serialize) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(s);
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(event.to_static());
    }
    events
}

fn deserialize<T: Deserialize>(events: Vec<Event<'_>>) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in events {
            driver.emit(event)?;
        }
    }
    Ok(out.unwrap())
}

/// A custom extension type: a timestamp that falls back to an integer.
#[derive(Debug, Clone, PartialEq)]
struct Timestamp(i64);

impl Extension for Timestamp {
    fn name(&self) -> &str {
        "timestamp"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::I64(self.0)
    }
}

impl Serialize for Timestamp {
    fn serialize(&self, _state: &SerializerState) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(self))))
    }
}

make_slot_wrapper!(SlotWrapper);

impl Sink for SlotWrapper<Timestamp> {
    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Ext(ref ext) if ext.is::<Timestamp>() => {
                **self = ext.downcast_ref::<Timestamp>().cloned();
                Ok(())
            }
            Atom::I64(value) => {
                **self = Some(Timestamp(value));
                Ok(())
            }
            other => self.unexpected_atom(other, state),
        }
    }
}

impl Deserialize for Timestamp {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        SlotWrapper::make_handle(out)
    }
}

#[test]
fn test_custom_extension_roundtrip() {
    let events = capture_events(&vec![Timestamp(42)]);
    assert_eq!(
        events,
        vec![
            Event::SeqStart,
            Event::Atom(Atom::Ext(ExtValue::owned(Timestamp(42)))),
            Event::SeqEnd,
        ]
    );
    assert_eq!(
        deserialize::<Vec<Timestamp>>(events).unwrap(),
        vec![Timestamp(42)]
    );
}

#[test]
fn test_unknown_extension_uses_fallback() {
    // a sink that does not know about timestamps gets the fallback
    let events = capture_events(&Timestamp(42));
    assert_eq!(deserialize::<i64>(events.clone()).unwrap(), 42);
    assert_eq!(
        deserialize::<Option<u32>>(events.clone()).unwrap(),
        Some(42)
    );

    // and still fails for incompatible types
    let err = deserialize::<String>(events).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Unexpected);
}

#[test]
fn test_wide_integers() {
    assert_eq!(
        capture_events(&u128::MAX),
        vec![Event::Atom(Atom::Ext(ExtValue::owned(u128::MAX)))]
    );
    assert_eq!(
        capture_events(&i128::MIN),
        vec![Event::Atom(Atom::Ext(ExtValue::owned(i128::MIN)))]
    );

    assert_eq!(
        deserialize::<u128>(vec![u128::MAX.into()]).unwrap(),
        u128::MAX
    );
    assert_eq!(
        deserialize::<i128>(vec![i128::MIN.into()]).unwrap(),
        i128::MIN
    );
    assert_eq!(deserialize::<u128>(vec![42u64.into()]).unwrap(), 42);
    assert_eq!(deserialize::<i128>(vec![(-42i64).into()]).unwrap(), -42);

    // wide integers can be deserialized into narrower types if they fit
    assert_eq!(deserialize::<u8>(vec![255u128.into()]).unwrap(), 255);
    assert_eq!(deserialize::<i64>(vec![(-1i128).into()]).unwrap(), -1);
    assert_eq!(
        deserialize::<u8>(vec![256u128.into()]).unwrap_err().kind(),
        ErrorKind::OutOfRange
    );
    assert_eq!(
        deserialize::<f64>(vec![(1u128 << 100).into()]).unwrap(),
        (1u128 << 100) as f64
    );
}

#[test]
fn test_integer_range_checks() {
    assert_eq!(
        deserialize::<i64>(vec![u64::MAX.into()])
            .unwrap_err()
            .kind(),
        ErrorKind::OutOfRange
    );
    assert_eq!(
        deserialize::<u64>(vec![(-1i64).into()]).unwrap_err().kind(),
        ErrorKind::OutOfRange
    );
    assert_eq!(
        deserialize::<u8>(vec![(-1i64).into()]).unwrap_err().kind(),
        ErrorKind::OutOfRange
    );
    assert_eq!(deserialize::<i8>(vec![(-128i64).into()]).unwrap(), -128);
}
