//! Tests of the context (configuration given to serializations and
//! deserializations from the outside).
use std::collections::BTreeMap;

use deser::de::{
    CollectErrors, DeserializeDriver, Deserializer, DuplicateKeys, Layer, LayerEvent, LexicalRules,
    Limits, Next, UnknownFields,
};
use deser::ser::{SerializeDriver, Serializer};
use deser::{Atom, Context, Deserialize, Error, Event, Serialize, Source, State, TrackLocations};

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
    let context = Context::with(UnknownFields::Error);

    let config: Config = Events(config_events()).deserialize().unwrap();
    assert_eq!(config.name, "x");

    let err = Events(config_events())
        .deserialize_with::<Config, _>(|driver| driver.set_context(context.clone()))
        .unwrap_err();
    assert_eq!(err.message(), "unknown field `nmae`, expected `name`");

    // the same context can be used for many deserializations
    let err = Events(config_events())
        .deserialize_with::<Config, _>(|driver| driver.set_context(context.clone()))
        .unwrap_err();
    assert_eq!(err.message(), "unknown field `nmae`, expected `name`");
}

#[test]
fn test_state_overrides_context() {
    let context = Context::with(UnknownFields::Error);
    let config: Config = Events(config_events())
        .deserialize_with(|driver| {
            driver.set_context(context.clone());
            // set in the state, this hides the value of the context
            UnknownFields::Ignore.set(driver.state_mut());
        })
        .unwrap();
    assert_eq!(config.name, "x");
}

#[test]
fn test_update_in() {
    let context = Context::with(DuplicateKeys::Last);
    let mut map = BTreeMap::<String, u32>::new();
    Events(vec![
        Event::map_start(),
        "a".into(),
        1u64.into(),
        "a".into(),
        2u64.into(),
        Event::MapEnd,
    ])
    .update_with(&mut map, |driver| driver.set_context(context.clone()))
    .unwrap();
    assert_eq!(map["a"], 2);
}

#[test]
fn test_state_get() {
    let mut state = State::new();
    assert_eq!(state.get::<u32>(), None);
    state.set_context(Context::with(1u32));
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
    out.serialize_with(&FromState, |driver| {
        driver.set_context(Context::with(42u32).clone())
    })
    .unwrap();
    assert_eq!(out.0, vec![Event::from(0u64), Event::from(42u64)]);
}

#[test]
fn test_set_default() {
    // formats set their defaults unless the context has a value
    let mut state = State::new();
    state.set_default(DuplicateKeys::Last);
    assert_eq!(DuplicateKeys::of(&state), DuplicateKeys::Last);

    let mut state = State::new();
    state.set_context(Context::with(DuplicateKeys::First));
    DuplicateKeys::Last.set_default(&mut state);
    assert_eq!(DuplicateKeys::of(&state), DuplicateKeys::First);
}

#[test]
fn test_collect_errors() {
    let events = || {
        vec![
            Event::seq_start(),
            "a".into(),
            1u64.into(),
            "b".into(),
            "c".into(),
            Event::SeqEnd,
        ]
    };
    let err = Events(events()).deserialize::<Vec<u32>>().unwrap_err();
    assert_eq!(err.errors().count(), 1);

    let all = Context::with(CollectErrors::new());
    let err = Events(events())
        .deserialize_with::<Vec<u32>, _>(|driver| driver.set_context(all.clone()))
        .unwrap_err();
    assert_eq!(err.errors().count(), 3);

    let limited = Context::with(CollectErrors::with_max_errors(1));
    let err = Events(events())
        .deserialize_with::<Vec<u32>, _>(|driver| driver.set_context(limited.clone()))
        .unwrap_err();
    assert_eq!(err.errors().count(), 2);
}

/// A format with a context, like the deserializers of the formats.
struct Format {
    events: Vec<Event<'static>>,
    context: Context,
}

impl<'de> Deserializer<'de> for Format {
    fn drive(&mut self, driver: &mut DeserializeDriver<'_, 'de>) -> Result<(), Error> {
        driver.set_default_context(self.context.clone());
        if TrackLocations::of(driver.state()) {
            Source("input".into()).set(driver.state_mut());
        }
        // everything is text unless the context says otherwise
        LexicalRules::LENIENT.set_default(driver.state_mut());
        Events(std::mem::take(&mut self.events)).drive(driver)
    }
}

/// Replaces strings with another string.
struct Replace(&'static str);

impl Layer for Replace {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        match event.event() {
            Event::Atom(Atom::Str(_)) => next.emit(LayerEvent::new(Event::from(self.0))),
            _ => next.emit(event),
        }
    }
}

#[test]
fn test_limits() {
    let events = || vec![Event::seq_start(), "abc".into(), Event::SeqEnd];
    let short = Context::with(Limits::builder().max_len(2).build());

    // the limits of the context of the format apply
    let err = Format {
        events: events(),
        context: short.clone(),
    }
    .deserialize::<Vec<String>>()
    .unwrap_err();
    assert_eq!(err.to_string(), "LimitExceeded: string or bytes too long");

    // the limits see the events after the layers, no matter if the layers
    // are added before or after the context is set
    let rv = Format {
        events: events(),
        context: short.clone(),
    }
    .deserialize_with::<Vec<String>, _>(|driver| driver.push_layer(Replace("a")));
    assert_eq!(rv.unwrap(), ["a"]);
    let rv = Events(events()).deserialize_with::<Vec<String>, _>(|driver| {
        driver.set_context(short.clone());
        driver.push_layer(Replace("a"));
    });
    assert_eq!(rv.unwrap(), ["a"]);
    let err = Events(vec![Event::seq_start(), "a".into(), Event::SeqEnd])
        .deserialize_with::<Vec<String>, _>(|driver| {
            driver.set_context(short.clone());
            driver.push_layer(Replace("abc"));
        })
        .unwrap_err();
    assert_eq!(err.to_string(), "LimitExceeded: string or bytes too long");

    // a context set on the driver replaces the one of the format, setting
    // the context again replaces the limits
    let rv = Format {
        events: events(),
        context: short.clone(),
    }
    .deserialize_with::<Vec<String>, _>(|driver| {
        driver.set_context(short.clone());
        driver.set_context(Context::with(Limits::builder().max_len(3).build()));
    });
    assert_eq!(rv.unwrap(), ["abc"]);
    let rv = Events(events()).deserialize_with::<Vec<String>, _>(|driver| {
        driver.set_context(short.clone());
        driver.set_context(Context::new());
    });
    assert_eq!(rv.unwrap(), ["abc"]);
}

#[test]
fn test_track_locations() {
    let source = |context: Context, state: Option<TrackLocations>| {
        let mut out = None::<bool>;
        let mut driver = DeserializeDriver::new(&mut out);
        if let Some(track) = state {
            track.set(driver.state_mut());
        }
        Format {
            events: vec![true.into()],
            context,
        }
        .drive(&mut driver)
        .unwrap();
        driver.state().get::<Source>().is_some()
    };
    let track = Context::with(TrackLocations(true));
    assert!(!source(Context::new(), None));
    assert!(source(track.clone(), None));
    assert!(!source(track.clone(), Some(TrackLocations(false))));
    assert!(source(Context::new(), Some(TrackLocations(true))));
}

#[test]
fn test_lexical_rules() {
    let events = || vec![Atom::Lexical("yes".into()).into()];
    // the format's default
    let value: bool = Format {
        events: events(),
        context: Context::new(),
    }
    .deserialize()
    .unwrap();
    assert!(value);
    // the context overrides it
    let err = Format {
        events: events(),
        context: Context::with(LexicalRules::STRICT),
    }
    .deserialize::<bool>()
    .unwrap_err();
    assert_eq!(
        err.message(),
        "invalid value \"yes\", expected bool (true or false)"
    );
}
