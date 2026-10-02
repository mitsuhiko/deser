//! Tests of the context (configuration given to serializations and
//! deserializations from the outside).
use std::collections::BTreeMap;

use deser::de::{DeserializeDriver, Deserializer, DuplicateKeys, UnknownFields};
use deser::ser::{SerializeDriver, Serializer};
use deser::{Context, Deserialize, Error, Event, Serialize, State};

#[derive(Debug, Deserialize)]
struct Config {
    name: String,
}

/// A deserializer for a list of events.
struct Events(Vec<Event<'static>>);

impl<'de> Deserializer<'de> for Events {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
        for event in self.0.drain(..) {
            driver.emit(event)?;
        }
        Ok(())
    }
}

fn config_events() -> Vec<Event<'static>> {
    vec![
        Event::map_start(),
        "name".into(),
        "x".into(),
        "nmae".into(),
        "y".into(),
        Event::MapEnd,
    ]
}

#[test]
fn test_deserialize_in() {
    let context = Context::new().with(UnknownFields::Error);

    let config: Config = Events(config_events()).deserialize().unwrap();
    assert_eq!(config.name, "x");

    let err = Events(config_events())
        .deserialize_in::<Config>(&context)
        .unwrap_err();
    assert_eq!(err.message(), "unknown field `nmae`, expected `name`");

    // the same context can be used for many deserializations
    let err = Events(config_events())
        .deserialize_in::<Config>(&context)
        .unwrap_err();
    assert_eq!(err.message(), "unknown field `nmae`, expected `name`");
}

#[test]
fn test_state_overrides_context() {
    let context = Context::new().with(UnknownFields::Error);
    let config: Config = Events(config_events())
        .deserialize_with(|driver| {
            driver.set_context(&context);
            // set in the state, this hides the value of the context
            UnknownFields::Ignore.set(driver.state_mut());
        })
        .unwrap();
    assert_eq!(config.name, "x");
}

#[test]
fn test_update_in() {
    let context = Context::new().with(DuplicateKeys::Last);
    let mut map = BTreeMap::<String, u32>::new();
    Events(vec![
        Event::map_start(),
        "a".into(),
        1u64.into(),
        "a".into(),
        2u64.into(),
        Event::MapEnd,
    ])
    .update_in(&mut map, &context)
    .unwrap();
    assert_eq!(map["a"], 2);
}

#[test]
fn test_state_get() {
    let mut state = State::new();
    assert_eq!(state.get::<u32>(), None);
    state.set_context(Context::new().with(1u32));
    assert_eq!(state.get::<u32>(), Some(&1));
    assert_eq!(state.context().get::<u32>(), Some(&1));
    // `get_mut` starts from the default of the type, not the context
    *state.get_mut::<u32>() += 2;
    assert_eq!(state.get::<u32>(), Some(&2));
    assert_eq!(state.context().get::<u32>(), Some(&1));
}

/// A value that serializes the `u32` of the state.
struct FromState;

impl Serialize for FromState {
    fn serialize<'a>(_value: &'a Self, state: &mut State) -> Result<deser::ser::Emit<'a>, Error> {
        Ok(deser::ser::Emit::Atom(deser::Atom::U64(
            state.get::<u32>().copied().unwrap_or(0).into(),
        )))
    }
}

/// A serializer that collects the events.
#[derive(Default)]
struct Collect(Vec<Event<'static>>);

impl Serializer for Collect {
    fn drive(&mut self, driver: &mut SerializeDriver<'_>) -> Result<(), Error> {
        while let Some((event, _, _)) = driver.next()? {
            self.0.push(event.to_static());
        }
        Ok(())
    }
}

#[test]
fn test_serialize_in() {
    let mut out = Collect::default();
    out.serialize(&FromState).unwrap();
    out.serialize_in(&FromState, &Context::new().with(42u32))
        .unwrap();
    assert_eq!(out.0, vec![Event::from(0u64), Event::from(42u64)]);
}
