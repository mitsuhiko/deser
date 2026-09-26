use deser::State;
use deser::de::{DeserializeDriver, DeserializeOwned, Sink, SinkHandle};
use deser::ext::{ExtValue, Extension};
use deser::ser::{Chunk, SerializeDriver};
use deser::{Atom, Deserialize, Error, ErrorKind, Event, Serialize, make_slot_wrapper};

fn capture_events(s: &dyn Serialize) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(s);
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(event.to_static());
    }
    events
}

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
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
    fn serialize(&self, _state: &mut State) -> Result<Chunk<'_>, Error> {
        Ok(Chunk::Atom(Atom::Ext(ExtValue::borrowed(self))))
    }
}

make_slot_wrapper!(SlotWrapper);

impl<'de> Sink<'de> for SlotWrapper<Timestamp> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
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

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SlotWrapper::make_handle(out)
    }
}

#[test]
fn test_custom_extension_roundtrip() {
    let events = capture_events(&vec![Timestamp(42)]);
    assert_eq!(
        events,
        vec![
            Event::seq_start(),
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

/// An extension value that represents a null with additional information.
#[derive(Debug, Clone, PartialEq)]
struct AnnotatedNull(&'static str);

impl Extension for AnnotatedNull {
    fn name(&self) -> &str {
        "annotated null"
    }

    fn fallback(&self) -> Atom<'_> {
        Atom::Null
    }
}

#[test]
fn test_optional_null_extension() {
    let value = AnnotatedNull("missing");
    let null = || Event::Atom(Atom::Ext(ExtValue::borrowed(&value)));
    assert_eq!(deserialize::<Option<u32>>(vec![null()]).unwrap(), None);
    assert_eq!(
        deserialize::<Vec<Option<String>>>(vec![
            Event::seq_start(),
            null(),
            "x".into(),
            Event::SeqEnd
        ])
        .unwrap(),
        vec![None, Some("x".to_string())]
    );
    // non optional types still reject it
    assert!(deserialize::<u32>(vec![null()]).is_err());
}

/// An extension that borrows its text.
mod borrowed {
    use std::borrow::Cow;

    use deser::Atom;
    use deser::ext::BorrowedExtension;

    #[derive(Debug, Clone, PartialEq)]
    pub struct Literal<'a> {
        pub text: Cow<'a, str>,
        pub value: f64,
    }

    impl BorrowedExtension for Literal<'static> {
        type Value<'a> = Literal<'a>;

        fn name<'v>(_value: &'v Literal<'_>) -> &'v str {
            "literal"
        }

        fn fallback<'v>(value: &'v Literal<'_>) -> Atom<'v> {
            Atom::F64(value.value)
        }

        fn to_static(value: &Literal<'_>) -> Literal<'static> {
            Literal {
                text: Cow::Owned(value.text.to_string()),
                value: value.value,
            }
        }

        fn shorten<'s, 'l: 's>(value: &'s Literal<'l>) -> &'s Literal<'s> {
            value
        }
    }
}

fn shorten<'long: 'short, 'short>(ext: ExtValue<'long>) -> ExtValue<'short> {
    ext
}

#[test]
fn test_borrowed_extension() {
    use borrowed::Literal;
    use std::borrow::Cow;

    let input = String::from("1.50");
    let detached = {
        let literal = Literal {
            text: Cow::Borrowed(&input),
            value: 1.5,
        };
        let ext = ExtValue::borrowed_value::<Literal>(&literal);
        assert!(ext.is::<Literal>());
        assert!(!ext.is::<u128>());
        assert_eq!(ext.name(), "literal");
        assert_eq!(ext.fallback(), Atom::F64(1.5));
        assert_eq!(ext.downcast_value_ref::<Literal>().unwrap().text, "1.50");
        assert!(ext.downcast_ref::<u128>().is_none());
        assert_eq!(format!("{:?}", ext), format!("{:?}", literal));

        // values can be used with shorter lifetimes, cloned and compared
        let ext = shorten(ext);
        let cloned = ext.clone();
        assert_eq!(cloned, ext);
        assert_eq!(ext.as_borrowed(), ext);
        ext.to_static()
    };
    drop(input);
    assert_eq!(
        detached.downcast_value_ref::<Literal>().unwrap().text,
        "1.50"
    );

    // owned values that borrow
    let input = String::from("2.5");
    let owned = ExtValue::owned_value::<Literal>(Literal {
        text: Cow::Borrowed(&input),
        value: 2.5,
    });
    assert_eq!(owned.clone(), owned);
    assert_ne!(owned, detached);
    assert_ne!(owned, ExtValue::owned(42u128));
    let other = String::from("2.5");
    assert_eq!(
        owned,
        ExtValue::owned_value::<Literal>(Literal {
            text: Cow::Borrowed(&other),
            value: 2.5,
        })
    );

    // sinks that do not know the extension get the fallback
    let atom = Atom::Ext(owned.clone());
    let value: f64 = {
        let mut out = None;
        {
            let mut driver = DeserializeDriver::new(&mut out);
            driver.emit(atom).unwrap();
        }
        out.unwrap()
    };
    assert_eq!(value, 2.5);
}
