use deser::ser::{Layer, Next};
use deser::{Error, Event, State};

use crate::{Frame, Path, PathLayer, PathSegment};

/// Tracks the path during serialization.
///
/// The format receives the events with the path of the value they belong
/// to (map keys with the path of the map).  After an event was passed on,
/// the path is updated for the value that follows, so that its
/// [`Serialize`](deser::Serialize) implementation can access it.
impl Layer for PathLayer {
    fn event(&mut self, event: Event<'_>, next: &mut Next<'_>) -> Result<(), Error> {
        if !self.registered {
            self.register(next.state_mut(), false);
        }
        match event {
            Event::MapEnd | Event::SeqEnd => {
                if let Some(Frame::Seq(_)) = self.frames.pop() {
                    next.state_mut().get_mut::<Path>().pop();
                }
                next.emit(event)?;
                self.complete_item(next.state_mut(), None);
            }
            Event::MapStart(_) | Event::SeqStart(_) => {
                let is_map = matches!(event, Event::MapStart(_));
                next.emit(event)?;
                if is_map {
                    self.frames.push(Frame::Map(false));
                } else {
                    self.frames.push(Frame::Seq(0));
                    next.state_mut()
                        .get_mut::<Path>()
                        .push(PathSegment::Index(0));
                }
            }
            Event::Atom(ref atom) => {
                let key = match self.frames.last() {
                    Some(Frame::Map(false)) => {
                        Some(next.state_mut().get_mut::<Path>().key_segment(atom))
                    }
                    _ => None,
                };
                next.emit(event)?;
                self.complete_item(next.state_mut(), key);
            }
        }
        Ok(())
    }
}

impl PathLayer {
    /// Updates the path after an item of the current container was
    /// serialized.
    ///
    /// For map keys the key segment is given.
    fn complete_item(&mut self, state: &mut State, key: Option<PathSegment>) {
        match self.frames.last_mut() {
            Some(Frame::Map(in_value)) => {
                let path = state.get_mut::<Path>();
                if *in_value {
                    path.pop();
                } else {
                    path.push(key.unwrap_or(PathSegment::Unknown));
                }
                *in_value = !*in_value;
            }
            Some(Frame::Seq(index)) => {
                *index += 1;
                let index = *index;
                state.get_mut::<Path>().set_last(PathSegment::Index(index));
            }
            None => {}
        }
    }
}
