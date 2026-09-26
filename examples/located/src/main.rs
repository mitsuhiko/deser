//! This example shows how to pass information through deser that is not part
//! of its data model: every value is annotated with the path and the source
//! location (line and column) where it was found in the input, and types can
//! pick up that information.
//!
//! This is similar to what `serde_spanned` or `serde_path_to_error` do for
//! serde.  The interesting part is that this keeps working when a value is
//! internally buffered and replayed, as untagged enums or internally tagged
//! enums (where the tag does not come first) have to do.  In serde such
//! information is typically lost in that case (serde issue #1183).
//!
//! There are two ways to carry such information and both survive buffering:
//!
//! * Out-of-band in the deserializer state.  The JSON deserializer publishes
//!   the input range of every event there (`State::input_range`) which
//!   `deser_location::Locations` resolves into lines and columns, and the
//!   `deser_path::PathLayer` maintains the path there as replayable state.
//!   A `deser::de::Recording` captures both for every recorded event and
//!   restores them on replay.  `deser_location::Spanned` reads the location
//!   from the state.
//! * In-band as extension values.  [`Annotator`] is a layer which turns
//!   every primitive value into a [`LocatedAtom`] extension value carrying
//!   the value, its path and its location.  Its fallback is the plain value,
//!   so types that do not know about it continue to work.  [`Located`]
//!   picks the information up from the extension value.  As layers see the
//!   events before they are recorded, the annotated values are recorded
//!   and replayed as well.
//!
//! [`Either`] is a hand written untagged enum on top of
//! `Recording::capture`.  [`Backend`], [`Action`], [`Hook`] and [`Limit`] are
//! derived enums in the different representations (internally tagged,
//! externally tagged, adjacently tagged and untagged) which buffer where
//! needed.
//!
//! The path layer also attaches the path to errors, see the end of `main`.
use std::fmt;

use deser::de::{Format, Layer, LayerEvent, Next, OwnedSink, Recording, Sink, SinkHandle};
use deser::ext::{ExtValue, Extension};
use deser::State;
use deser::{Atom, Descriptor, Deserialize, Error, Event};
use deser_location::{Locations, Span, Spanned};
use deser_path::{Path, PathLayer};

/// A primitive value annotated with the path and location where it was found.
///
/// This is the value that is passed through the data model as extension.
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

/// A layer which annotates all primitive values with their path and
/// location.
///
/// This needs to be added after a `PathLayer` which maintains the path and
/// the format needs to publish locations.
pub struct Annotator;

impl Layer for Annotator {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        // map keys are not annotated and values that already are extension
        // values are passed through as is as fallbacks cannot be extension
        // values themselves.
        let located = match event.event() {
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
        next.emit(LayerEvent::new(Event::Atom(Atom::Ext(ExtValue::borrowed(
            &located,
        )))))
    }
}

/// The JSON configuration: locations are needed for the spans.
const CONFIG: deser_json::DeserializerConfig =
    deser_json::DeserializerConfig::new().track_locations(true);

/// Deserializes JSON and annotates all values with their path and location.
pub fn from_json_with_locations<'de, T: Deserialize<'de>>(json: &'de str) -> Result<T, Error> {
    deser_json::Deserializer::from_str_with_config(json, &CONFIG).deserialize_with(|driver| {
        driver.push_layer(PathLayer::new());
        driver.push_layer(Annotator);
    })
}

/// A value together with the path and location where it was found (in-band).
///
/// This information is only available if the input was annotated, otherwise
/// it's `None`.
pub struct Located<T> {
    pub value: T,
    pub path: Option<String>,
    pub span: Option<Span>,
}

/// Renders as the value followed by the location, e.g. `8080 (port @ 4:13-4:17)`.
impl<T: fmt::Debug> fmt::Debug for Located<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.value, f)?;
        match (&self.path, &self.span) {
            (Some(path), Some(span)) => write!(f, " ({} @ {:?})", path, span),
            (Some(path), None) => write!(f, " ({})", path),
            (None, Some(span)) => write!(f, " (@ {:?})", span),
            (None, None) => Ok(()),
        }
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

    fn map(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().map(state)
    }

    fn seq(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().seq(state)
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink.borrow_mut().next_key(state)
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        self.sink.borrow_mut().next_value(state)
    }

    fn finish(&mut self, state: &mut State) -> Result<(), Error> {
        self.sink.borrow_mut().finish(state)?;
        let span = self.span;
        *self.out = self.sink.take().map(|value| Located {
            value,
            path: self.path.take(),
            span,
        });
        Ok(())
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.sink.borrow().descriptor()
    }
}

/// An untagged enum: the value is either an `A` or a `B`.
///
/// Since it's not known upfront which one it is, the value is recorded and
/// then replayed into `A` and if that fails, into `B`.
#[derive(Debug)]
pub enum Either<A, B> {
    Left(A),
    Right(B),
}

impl<'de, A: Deserialize<'de>, B: Deserialize<'de>> Deserialize<'de> for Either<A, B> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_, 'de> {
        Recording::capture(move |recording, state| {
            let mut left = None;
            if recording
                .replay(A::deserialize_into(&mut left), state)
                .is_ok()
            {
                *out = left.map(Either::Left);
            } else {
                let mut right = None;
                recording.replay(B::deserialize_into(&mut right), state)?;
                *out = right.map(Either::Right);
            }
            Ok(())
        })
    }
}

/// An internally tagged enum.  If the tag does not come first, the fields
/// have to be buffered until the tag is known.
#[derive(Debug, Deserialize)]
#[deser(tag = "type", rename_all = "lowercase")]
pub enum Backend {
    Http {
        url: Located<String>,
        timeout: Spanned<u32>,
    },
    File {
        path: Located<String>,
    },
}

/// An externally tagged enum (the default) with the different variant kinds.
#[derive(Debug, Deserialize)]
#[deser(rename_all = "snake_case")]
pub enum Action {
    Restart { delay: Located<u32> },
    Notify(Located<String>),
    Scale(Located<u32>, Spanned<u32>),
    Noop,
}

/// An adjacently tagged enum.  If the content comes before the tag, it's
/// buffered until the tag is known.
#[derive(Debug, Deserialize)]
#[deser(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Hook {
    Command(Vec<Located<String>>),
    Url(Spanned<String>),
}

/// An untagged enum.  The value is buffered and replayed into the variants
/// until one accepts it.
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
    pub port: Located<u16>,
    // plain types do not know about locations and get the fallback
    pub debug: bool,
    // the out-of-band location from the state works as long as nothing is
    // buffered
    pub workers: Spanned<u32>,
    // untagged values are buffered and replayed, in-band information
    // survives this
    pub timeout: Either<Located<u64>, Located<String>>,
    // as does out-of-band information from the state as recordings capture
    // and restore it
    pub retries: Either<Spanned<u64>, Spanned<String>>,
    // the tag comes last, so the fields are buffered
    pub backend: Backend,
    // the other enum representations
    pub actions: Vec<Action>,
    pub hooks: Vec<Hook>,
    pub limits: Vec<Limit>,
    // out-of-band spans also work for maps and sequences
    pub servers: Vec<Spanned<Server>>,
}

#[derive(Debug, Deserialize)]
pub struct Server {
    pub host: Located<String>,
    pub weight: Option<Located<u32>>,
    pub backup: Option<bool>,
}

const INPUT: &str = r#"
{
    "name": "demo",
    "port": 8080,
    "debug": true,
    "workers": 4,
    "timeout": "30s",
    "retries": 3,
    "backend": {
        "url": "https://example.com/",
        "timeout": 30,
        "type": "http"
    },
    "actions": [
        {"restart": {"delay": 5}},
        {"notify": "ops@example.com"},
        {"scale": [2, 10]},
        "noop"
    ],
    "hooks": [
        {"data": ["systemctl", "reload"], "kind": "command"},
        {"kind": "url", "data": "https://hooks.example.com/"}
    ],
    "limits": [
        100,
        {"max": 20, "min": 10}
    ],
    "servers": [
        {"host": "a.example.com", "weight": 2, "backup": false},
        {"host": "b.example.com", "weight": null, "backup": null}
    ]
}
"#;

fn main() {
    println!("with locations:");
    let config: Config = from_json_with_locations(INPUT).unwrap();
    println!("{:#?}", config);

    println!();
    println!("plain JSON:");
    let config: Config = deser_json::from_str(INPUT).unwrap();
    println!("{:#?}", config);

    // errors carry the location and, with the path layer, the path.  This
    // also works for values which are buffered: the backend is replayed
    // once its type is known.
    println!();
    println!("error:");
    let input = INPUT.replace(r#""timeout": 30,"#, r#""timeout": "30s","#);
    let err = from_json_with_locations::<Config>(&input).unwrap_err();
    println!("{}", err);
    assert_eq!(err.path(), Some("backend.timeout"));
    assert_eq!((err.line(), err.column()), (Some(11), Some(20)));
}
