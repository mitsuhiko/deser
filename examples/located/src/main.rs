//! This example shows how to pass information through deser that is not part
//! of its data model: every value is annotated with the path where it was
//! found in the input, and types can pick up that path.
//!
//! This is similar to what `serde_spanned` or `serde_path_to_error` do for
//! serde.  The interesting part is that this keeps working when a value is
//! internally buffered and replayed (as untagged enums have to do).  In serde
//! such information is typically lost in that case (serde issue #1183).
//!
//! The pieces are:
//!
//! * [`LocatedAtom`]: an extension value that carries a primitive value
//!   together with its path.  Its fallback is the plain value, so types that
//!   do not know about it continue to work.
//! * [`Annotator`]: a sink wrapper which sits between the format and the
//!   target type.  It turns every primitive value into a [`LocatedAtom`].
//!   The paths are tracked by `deser_path::PathSink`.
//! * [`Located`]: a type that picks up the path from the extension value
//!   (in-band).
//! * [`StatePath`]: for comparison, a type that reads the path from the
//!   deserializer state (out-of-band).
//! * [`Either`]: an untagged enum that has to buffer its input and replays it
//!   into its variants.
use std::fmt;

use deser::de::{DeserializeDriver, DeserializerState, OwnedSink, Sink, SinkHandle};
use deser::ext::{ExtValue, Extension};
use deser::{Atom, Descriptor, Deserialize, Error, ErrorKind, Event};
use deser_path::{Path, PathSegment, PathSink};

/// Formats a path as `servers[1].host`.
fn format_path(path: &Path) -> String {
    let mut rv = String::new();
    for segment in path.segments() {
        match segment {
            PathSegment::Key(key) => {
                if !rv.is_empty() {
                    rv.push('.');
                }
                rv.push_str(key);
            }
            PathSegment::Index(idx) => rv.push_str(&format!("[{}]", idx)),
            PathSegment::Unknown => rv.push_str(".?"),
        }
    }
    rv
}

/// A primitive value annotated with the path where it was found.
///
/// This is the value that is passed through the data model as extension.
#[derive(Debug, Clone, PartialEq)]
pub struct LocatedAtom {
    path: String,
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

/// Wraps a sink and annotates all primitive values with their path.
///
/// This needs to be wrapped by a `PathSink` which maintains the path.
pub struct Annotator<'a> {
    sink: SinkHandle<'a>,
}

impl<'a> Annotator<'a> {
    pub fn wrap(sink: SinkHandle<'a>) -> Annotator<'a> {
        Annotator { sink }
    }
}

impl<'a> Sink for Annotator<'a> {
    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
        // map keys are not annotated and values that already are extension
        // values are passed through as is as fallbacks cannot be extension
        // values themselves.
        if state.is_map_key() || matches!(atom, Atom::Ext(_)) {
            return self.sink.atom(atom, state);
        }
        let located = LocatedAtom {
            path: format_path(&state.get::<Path>()),
            value: atom.to_static(),
        };
        self.sink
            .atom(Atom::Ext(ExtValue::borrowed(&located)), state)
    }

    fn map(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.sink.map(state)
    }

    fn seq(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.sink.seq(state)
    }

    fn next_key(&mut self, state: &DeserializerState) -> Result<SinkHandle<'_>, Error> {
        Ok(SinkHandle::boxed(Annotator::wrap(
            self.sink.next_key(state)?,
        )))
    }

    fn next_value(&mut self, state: &DeserializerState) -> Result<SinkHandle<'_>, Error> {
        Ok(SinkHandle::boxed(Annotator::wrap(
            self.sink.next_value(state)?,
        )))
    }

    fn value_for_key(
        &mut self,
        key: &str,
        state: &DeserializerState,
    ) -> Result<Option<SinkHandle<'_>>, Error> {
        Ok(self
            .sink
            .value_for_key(key, state)?
            .map(|sink| SinkHandle::boxed(Annotator::wrap(sink))))
    }

    fn finish(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.sink.finish(state)
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.sink.descriptor()
    }
}

/// Deserializes JSON and annotates all values with their location.
pub fn from_json_with_locations<T: Deserialize>(json: &str) -> Result<T, Error> {
    let mut out = None;
    {
        let sink = Annotator::wrap(T::deserialize_into(&mut out));
        let sink = PathSink::wrap_ref(SinkHandle::boxed(sink));
        let mut driver = DeserializeDriver::from_sink(SinkHandle::boxed(sink));
        deser_json::Deserializer::new(json).drive(&mut driver)?;
    }
    out.ok_or_else(|| Error::new(ErrorKind::EndOfFile, "empty input"))
}

/// A value together with the path where it was found (in-band).
///
/// The path is only available if the input was annotated, otherwise it's
/// `None`.
#[derive(Debug)]
pub struct Located<T> {
    pub value: T,
    pub path: Option<String>,
}

impl<T: Deserialize> Deserialize for Located<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        SinkHandle::boxed(LocatedSink {
            out,
            sink: OwnedSink::deserialize(),
            path: None,
        })
    }
}

struct LocatedSink<'a, T> {
    out: &'a mut Option<Located<T>>,
    sink: OwnedSink<T>,
    path: Option<String>,
}

impl<'a, T: Deserialize> Sink for LocatedSink<'a, T> {
    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
        match atom {
            Atom::Ext(ref ext) if ext.is::<LocatedAtom>() => {
                let located = ext.downcast_ref::<LocatedAtom>().unwrap();
                self.path = Some(located.path.clone());
                self.sink.borrow_mut().atom(located.value.clone(), state)
            }
            other => self.sink.borrow_mut().atom(other, state),
        }
    }

    fn map(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.sink.borrow_mut().map(state)
    }

    fn seq(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.sink.borrow_mut().seq(state)
    }

    fn next_key(&mut self, state: &DeserializerState) -> Result<SinkHandle<'_>, Error> {
        self.sink.borrow_mut().next_key(state)
    }

    fn next_value(&mut self, state: &DeserializerState) -> Result<SinkHandle<'_>, Error> {
        self.sink.borrow_mut().next_value(state)
    }

    fn finish(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.sink.borrow_mut().finish(state)?;
        *self.out = self.sink.take().map(|value| Located {
            value,
            path: self.path.take(),
        });
        Ok(())
    }

    fn descriptor(&self) -> &'static dyn Descriptor {
        self.sink.borrow().descriptor()
    }
}

/// A value together with the path from the deserializer state (out-of-band).
///
/// This is what one would do without extensions: look at the state of the
/// deserializer while the value is deserialized.
#[derive(Debug)]
pub struct StatePath<T> {
    pub value: T,
    pub path: String,
}

impl<T: Deserialize> Deserialize for StatePath<T> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        SinkHandle::boxed(StatePathSink {
            out,
            sink: OwnedSink::deserialize(),
            path: String::new(),
        })
    }
}

struct StatePathSink<'a, T> {
    out: &'a mut Option<StatePath<T>>,
    sink: OwnedSink<T>,
    path: String,
}

impl<'a, T: Deserialize> Sink for StatePathSink<'a, T> {
    fn atom(&mut self, atom: Atom, state: &DeserializerState) -> Result<(), Error> {
        self.path = format_path(&state.get::<Path>());
        self.sink.borrow_mut().atom(atom, state)
    }

    fn finish(&mut self, state: &DeserializerState) -> Result<(), Error> {
        self.sink.borrow_mut().finish(state)?;
        *self.out = self.sink.take().map(|value| StatePath {
            value,
            path: std::mem::take(&mut self.path),
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

impl<A: Deserialize, B: Deserialize> Deserialize for Either<A, B> {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle<'_> {
        SinkHandle::boxed(EitherSink {
            out,
            events: Vec::new(),
            end: None,
        })
    }
}

struct EitherSink<'a, A, B> {
    out: &'a mut Option<Either<A, B>>,
    events: Vec<Event<'static>>,
    end: Option<Event<'static>>,
}

/// Replays recorded events into a fresh driver.
fn replay<T: Deserialize>(events: &[Event<'static>]) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in events {
            driver.emit(event.clone())?;
        }
    }
    out.ok_or_else(|| Error::new(ErrorKind::Unexpected, "no value"))
}

impl<'a, A: Deserialize, B: Deserialize> Sink for EitherSink<'a, A, B> {
    fn atom(&mut self, atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
        // `to_static` clones extension values, so the location survives
        self.events.push(Event::Atom(atom.to_static()));
        Ok(())
    }

    fn map(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.events.push(Event::MapStart);
        self.end = Some(Event::MapEnd);
        Ok(())
    }

    fn seq(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.events.push(Event::SeqStart);
        self.end = Some(Event::SeqEnd);
        Ok(())
    }

    fn next_key(&mut self, _state: &DeserializerState) -> Result<SinkHandle<'_>, Error> {
        Ok(SinkHandle::boxed(Recorder::new(&mut self.events)))
    }

    fn next_value(&mut self, _state: &DeserializerState) -> Result<SinkHandle<'_>, Error> {
        Ok(SinkHandle::boxed(Recorder::new(&mut self.events)))
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.events.extend(self.end.take());
        *self.out = Some(match replay::<A>(&self.events) {
            Ok(value) => Either::Left(value),
            Err(_) => Either::Right(replay::<B>(&self.events)?),
        });
        Ok(())
    }
}

/// Records all events of a value into a buffer.
struct Recorder<'a> {
    events: &'a mut Vec<Event<'static>>,
    end: Option<Event<'static>>,
}

impl<'a> Recorder<'a> {
    fn new(events: &'a mut Vec<Event<'static>>) -> Recorder<'a> {
        Recorder { events, end: None }
    }
}

impl<'a> Sink for Recorder<'a> {
    fn atom(&mut self, atom: Atom, _state: &DeserializerState) -> Result<(), Error> {
        self.events.push(Event::Atom(atom.to_static()));
        Ok(())
    }

    fn map(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.events.push(Event::MapStart);
        self.end = Some(Event::MapEnd);
        Ok(())
    }

    fn seq(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.events.push(Event::SeqStart);
        self.end = Some(Event::SeqEnd);
        Ok(())
    }

    fn next_key(&mut self, _state: &DeserializerState) -> Result<SinkHandle<'_>, Error> {
        Ok(SinkHandle::boxed(Recorder::new(self.events)))
    }

    fn next_value(&mut self, _state: &DeserializerState) -> Result<SinkHandle<'_>, Error> {
        Ok(SinkHandle::boxed(Recorder::new(self.events)))
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        self.events.extend(self.end.take());
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    name: Located<String>,
    port: Located<u16>,
    // plain types do not know about locations and get the fallback
    debug: bool,
    // the out-of-band path from the state works as long as nothing is
    // buffered
    workers: StatePath<u32>,
    // untagged values are buffered, the in-band location survives this
    timeout: Either<Located<u64>, Located<String>>,
    // the out-of-band path from the state does not survive buffering
    retries: Either<StatePath<u64>, StatePath<String>>,
    servers: Vec<Server>,
}

#[derive(Debug, Deserialize)]
pub struct Server {
    host: Located<String>,
    weight: Option<Located<u32>>,
    backup: Option<bool>,
}

impl<T: fmt::Debug> fmt::Display for Located<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.path {
            Some(ref path) => write!(f, "{:?} (at {})", self.value, path),
            None => write!(f, "{:?} (at unknown location)", self.value),
        }
    }
}

impl<T: fmt::Debug> fmt::Display for StatePath<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} (at {:?})", self.value, self.path)
    }
}

impl<A: fmt::Display, B: fmt::Display> fmt::Display for Either<A, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Either::Left(value) => fmt::Display::fmt(value, f),
            Either::Right(value) => fmt::Display::fmt(value, f),
        }
    }
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "name:    {}", self.name)?;
        writeln!(f, "port:    {}", self.port)?;
        writeln!(f, "debug:   {:?}", self.debug)?;
        writeln!(f, "workers: {}", self.workers)?;
        writeln!(f, "timeout: {}", self.timeout)?;
        writeln!(f, "retries: {}", self.retries)?;
        for server in &self.servers {
            write!(f, "server:  {}", server.host)?;
            if let Some(ref weight) = server.weight {
                write!(f, ", weight {}", weight)?;
            }
            writeln!(f, ", backup {:?}", server.backup)?;
        }
        Ok(())
    }
}

const INPUT: &str = r#"
{
    "name": "demo",
    "port": 8080,
    "debug": true,
    "workers": 4,
    "timeout": "30s",
    "retries": 3,
    "servers": [
        {"host": "a.example.com", "weight": 2, "backup": false},
        {"host": "b.example.com", "weight": null, "backup": null}
    ]
}
"#;

fn main() {
    println!("with locations:");
    let config: Config = from_json_with_locations(INPUT).unwrap();
    println!("{}", config);

    println!("plain JSON:");
    let config: Config = deser_json::from_str(INPUT).unwrap();
    println!("{}", config);
}
