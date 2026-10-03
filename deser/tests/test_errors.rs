use std::collections::BTreeMap;

use deser::de::{
    DeserializeDriver, Deserializer, Layer, LayerEvent, Limits, Next, deserialize_value,
};
use deser::ser::{Emit, SerializeDriver};
use deser::{Deserialize, Error, ErrorCategory, ErrorKind, Event, Serialize, State};

/// Deserializes a value from events (which are not borrowed).
fn deserialize<'de, T: Deserialize<'de>>(events: Vec<Event<'_>>) -> Result<T, Error> {
    deserialize_value(|driver| {
        for event in events {
            driver.emit(event)?;
        }
        Ok(())
    })
}

/// A format which emits events and then fails with an error.
struct FailingFormat(Vec<Event<'static>>, ErrorKind);

impl<'de> Deserializer<'de> for FailingFormat {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
        for event in self.0.drain(..) {
            driver.emit(event)?;
        }
        Err(Error::new(self.1, "the format failed"))
    }
}

/// Rejects every atom with an error of the custom kind.
struct RejectAtoms;

impl Layer for RejectAtoms {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        match event.event() {
            Event::Atom(_) => Err(Error::new(ErrorKind::Custom, "rejected")),
            _ => next.emit(event),
        }
    }
}

#[test]
fn test_categories_of_kinds() {
    for (kind, category) in [
        (ErrorKind::Syntax, ErrorCategory::Syntax),
        (ErrorKind::EndOfFile, ErrorCategory::Eof),
        (ErrorKind::LimitExceeded, ErrorCategory::Limit),
        (ErrorKind::InvalidType, ErrorCategory::Data),
        (ErrorKind::InvalidValue, ErrorCategory::Data),
        (ErrorKind::OutOfRange, ErrorCategory::Data),
        (ErrorKind::WrongLength, ErrorCategory::Data),
        (ErrorKind::MissingField, ErrorCategory::Data),
        (ErrorKind::UnknownField, ErrorCategory::Data),
        (ErrorKind::UnknownVariant, ErrorCategory::Data),
        (ErrorKind::DuplicateKey, ErrorCategory::Data),
        (ErrorKind::UnsupportedType, ErrorCategory::Unsupported),
        (ErrorKind::InvalidState, ErrorCategory::Usage),
        (ErrorKind::Configuration, ErrorCategory::Usage),
        (ErrorKind::Io, ErrorCategory::Io),
        // errors without context come from the formats
        (ErrorKind::Custom, ErrorCategory::Syntax),
    ] {
        assert_eq!(Error::new(kind, "x").category(), category, "{kind:?}");
    }
}

#[test]
fn test_categories_of_values() {
    #[derive(Deserialize, Debug)]
    #[deser(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Point {
        x: u8,
        y: u8,
    }

    #[derive(Deserialize, Debug)]
    #[allow(dead_code)]
    enum Shape {
        Circle,
    }

    let fields = |items: &[(&'static str, Event<'static>)]| {
        let mut events = vec![Event::map_start()];
        for (key, value) in items {
            events.push((*key).into());
            events.push(value.clone());
        }
        events.push(Event::MapEnd);
        events
    };

    let check = |err: Error, kind: ErrorKind| {
        assert_eq!(err.kind(), kind, "{err}");
        assert_eq!(err.category(), ErrorCategory::Data, "{err}");
    };
    check(
        deserialize::<Point>(fields(&[("x", "a".into()), ("y", 1u64.into())])).unwrap_err(),
        ErrorKind::InvalidType,
    );
    check(
        deserialize::<Point>(fields(&[("x", 300u64.into()), ("y", 1u64.into())])).unwrap_err(),
        ErrorKind::OutOfRange,
    );
    check(
        deserialize::<Point>(fields(&[("x", 1u64.into())])).unwrap_err(),
        ErrorKind::MissingField,
    );
    check(
        deserialize::<Point>(fields(&[
            ("x", 1u64.into()),
            ("y", 1u64.into()),
            ("z", 1u64.into()),
        ]))
        .unwrap_err(),
        ErrorKind::UnknownField,
    );
    check(
        deserialize::<Shape>(vec!["Square".into()]).unwrap_err(),
        ErrorKind::UnknownVariant,
    );
    check(
        deserialize::<[u8; 2]>(vec![Event::seq_start(), 1u64.into(), Event::SeqEnd]).unwrap_err(),
        ErrorKind::WrongLength,
    );
    check(
        deserialize::<BTreeMap<String, u8>>(vec![
            Event::map_start(),
            "a".into(),
            1u64.into(),
            "a".into(),
            2u64.into(),
            Event::MapEnd,
        ])
        .unwrap_err(),
        ErrorKind::DuplicateKey,
    );
    check(
        deserialize::<u8>(vec![Event::Atom(deser::Atom::Lexical("abc".into()))]).unwrap_err(),
        ErrorKind::InvalidValue,
    );
}

#[test]
fn test_category_of_custom_errors_depends_on_the_origin() {
    // the format fails
    let err = FailingFormat(vec![Event::seq_start()], ErrorKind::Custom)
        .deserialize::<Vec<u32>>()
        .unwrap_err();
    assert_eq!(err.category(), ErrorCategory::Syntax);

    // a value fails
    let err = FailingFormat(vec![Event::seq_start(), 1u64.into()], ErrorKind::Custom)
        .deserialize_with::<Vec<u32>, _>(|driver| driver.push_layer(RejectAtoms))
        .unwrap_err();
    assert_eq!(err.message(), "rejected");
    assert_eq!(err.category(), ErrorCategory::Data);

    // a value fails to serialize
    struct Fails;

    impl Serialize for Fails {
        fn serialize<'a>(_value: &'a Self, _state: &mut State) -> Result<Emit<'a>, Error> {
            Err(Error::new(ErrorKind::Custom, "nope"))
        }
    }

    let value = vec![Fails];
    let err = SerializeDriver::new(&value)
        .drive(|_, _| Ok(()))
        .unwrap_err();
    assert_eq!(err.category(), ErrorCategory::Data);
}

#[test]
fn test_category_of_formats_and_layers() {
    // the kind of an error of the format decides
    let err = FailingFormat(vec![Event::seq_start()], ErrorKind::EndOfFile)
        .deserialize::<Vec<u32>>()
        .unwrap_err();
    assert_eq!(err.category(), ErrorCategory::Eof);

    // limits are their own category, although they fail values
    let err = FailingFormat(
        vec![Event::seq_start(), Event::seq_start()],
        ErrorKind::Syntax,
    )
    .deserialize_with::<Vec<Vec<u32>>, _>(|driver| {
        driver.set_context(deser::Context::with(Limits::builder().max_depth(1).build()));
    })
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::LimitExceeded);
    assert_eq!(err.category(), ErrorCategory::Limit);
}

#[test]
fn test_category_of_borrowing() {
    // a value which cannot be deserialized from what the format provides
    // is unsupported (it's not the input that is wrong)
    let err = deserialize::<&str>(vec!["hello".into()]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);
    assert_eq!(err.category(), ErrorCategory::Unsupported);
}

#[test]
fn test_category_of_multiple_errors() {
    let err = Error::from_errors([
        Error::new(ErrorKind::MissingField, "missing field `a`"),
        Error::new(ErrorKind::Syntax, "expected a comma"),
    ])
    .unwrap();
    assert_eq!(err.category(), ErrorCategory::Data);
}
