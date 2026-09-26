//! Passing information through deser that is not part of its data model:
//! values are annotated with their path and source location (line and
//! column) and types pick that up.
//!
//! This is similar to `serde_spanned` or `serde_path_to_error`, but it keeps
//! working when values are buffered and replayed, as untagged enums and
//! internally tagged enums (where the tag does not come first) have to do.
//! In serde such information is typically lost then (serde issue #1183).
//! Errors carry locations and paths too, also for buffered values (see the
//! `config-errors` example).
//!
//! There are two ways to carry such information and both survive buffering:
//!
//! * Out-of-band in the state: the JSON deserializer publishes the input
//!   range of every event, `deser_location::Spanned` resolves it.
//! * In-band as extension values: the [`Annotator`] layer turns primitive
//!   values into [`LocatedAtom`] extension values.  Their fallback is the
//!   plain value, so types that do not know about them continue to work.
//!   [`Located`] picks the information up.
use std::borrow::Cow;
use std::fmt;

use deser::State;
use deser::de::{Layer, LayerEvent, Next, OwnedSink, Sink, SinkHandle};
use deser::ext::{ExtValue, Extension};
use deser::{Atom, Deserialize, Error, Event};
use deser_location::{Locations, Span, Spanned};
use deser_path::{Path, PathLayer};

/// A primitive value annotated with its path and location (in-band).
#[derive(Debug, Clone, PartialEq)]
pub struct LocatedAtom {
    path: String,
    span: Option<Span>,
    value: Atom<'static>,
}

impl Extension for LocatedAtom {
    fn name(&self) -> &str {
        self.value.name()
    }

    fn fallback(&self) -> Atom<'_> {
        // everybody who does not know about locations just gets the value
        self.value.clone()
    }
}

/// A layer which annotates primitive values (but not map keys) with their
/// path and location.  It needs to be added after a `PathLayer`.
pub struct Annotator;

impl Layer for Annotator {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        let located = match event.event() {
            // fallbacks cannot be extension values, those are passed on
            Event::Atom(atom) if !next.state().is_map_key() && !matches!(atom, Atom::Ext(_)) => {
                LocatedAtom {
                    path: next
                        .state()
                        .get::<Path>()
                        .map(Path::to_string)
                        .unwrap_or_default(),
                    span: Locations::current_span(next.state_mut()),
                    value: atom.to_static(),
                }
            }
            _ => return next.emit(event),
        };
        let ext = ExtValue::borrowed(&located);
        next.emit(LayerEvent::new(Event::Atom(Atom::Ext(ext))))
    }
}

/// Deserializes JSON and annotates all values with their path and location.
pub fn from_json_with_locations<'de, T: Deserialize<'de>>(json: &'de str) -> Result<T, Error> {
    // locations are needed for the spans
    let config = deser_json::DeserializerConfig::new().track_locations(true);
    deser_json::Deserializer::from_str_with_config(json, &config).deserialize_with(|driver| {
        driver.push_layer(PathLayer::new());
        driver.push_layer(Annotator);
    })
}

/// A primitive value with its path and location (in-band).  They are `None`
/// if the input was not annotated.
pub struct Located<T> {
    pub value: T,
    pub path: Option<String>,
    pub span: Option<Span>,
}

/// Renders as the value followed by the location, e.g. `8080 (port @ 4:13-4:17)`.
impl<T: fmt::Debug> fmt::Debug for Located<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.value, f)?;
        if let (Some(path), Some(span)) = (&self.path, &self.span) {
            write!(f, " ({} @ {:?})", path, span)?;
        }
        Ok(())
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Located<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        SinkHandle::boxed(LocatedSink {
            out,
            sink: OwnedSink::deserialize(),
            path: None,
            span: None,
        })
    }
}

struct LocatedSink<'a, 'de, T> {
    out: &'a mut Option<Located<T>>,
    sink: OwnedSink<'de, T>,
    path: Option<String>,
    span: Option<Span>,
}

impl<'a, 'de, T: Deserialize<'de>> Sink<'de> for LocatedSink<'a, 'de, T> {
    fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), Error> {
        match atom {
            Atom::Ext(ref ext) if ext.is::<LocatedAtom>() => {
                let located = ext.downcast_ref::<LocatedAtom>().unwrap();
                self.path = Some(located.path.clone());
                self.span = located.span;
                self.sink.borrow_mut().atom(located.value.clone(), state)
            }
            other => self.sink.borrow_mut().atom(other, state),
        }
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().finish(state)?;
        let (path, span) = (self.path.take(), self.span);
        *self.out = self.sink.take().map(|value| Located { value, path, span });
        Ok(())
    }

    fn expecting(&self) -> Cow<'_, str> {
        self.sink.borrow().expecting()
    }
}

/// Untagged: the value is buffered and replayed into the variants until
/// one accepts it.
#[derive(Debug, Deserialize)]
#[deser(untagged)]
pub enum Limit {
    Exact(Located<u64>),
    Range {
        min: Located<u64>,
        max: Spanned<u64>,
    },
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub name: Located<String>,
    // plain types do not know about locations and get the fallback
    pub debug: bool,
    pub limits: Vec<Limit>,
    // out-of-band spans also work for maps and sequences
    pub hosts: Spanned<Vec<Located<String>>>,
}

const INPUT: &str = r#"
{
    "name": "demo",
    "debug": true,
    "limits": [100, {"max": 20, "min": 10}],
    "hosts": ["a.example.com", "b.example.com"]
}
"#;

fn main() {
    let config: Config = from_json_with_locations(INPUT).unwrap();
    println!("{:#?}", config);
    let Limit::Range { ref min, ref max } = config.limits[1] else {
        panic!("expected a range");
    };
    assert_eq!(min.path.as_deref(), Some("limits[1].min"));
    assert_eq!(max.span.unwrap().start.line, 5);

    // without annotations and location tracking the values are plain
    let config: Config = deser_json::from_str(INPUT).unwrap();
    assert!(config.name.span.is_none() && config.hosts.span.is_none());
}
