use deser::de::{Layer, LayerEvent, Next};
use deser::{Error, Event};

use crate::{Frame, Path, PathLayer, PathSegment};

/// Tracks the path during deserialization.
///
/// The path of a value is set before the value is passed on, so that the
/// sinks can access it.  Containers push a segment for their items after
/// they started and pop it before they end, so the path of a container is
/// its own path while it's finished.
impl Layer for PathLayer {
    fn event<'de>(
        &mut self,
        event: LayerEvent<'_, 'de>,
        next: &mut Next<'_, 'de>,
    ) -> Result<(), Error> {
        if !self.registered {
            self.register(next.state_mut(), true);
        }
        let is_map = match *event.event() {
            Event::MapEnd | Event::SeqEnd => {
                self.frames.pop();
                next.state_mut().get_mut::<Path>().pop();
                return next.emit(event);
            }
            Event::MapStart => Some(true),
            Event::SeqStart => Some(false),
            Event::Atom(_) => None,
        };

        let is_map_key = next.state().is_map_key();
        match self.frames.last_mut() {
            Some(Frame::Seq(index)) => {
                let segment = PathSegment::Index(*index);
                *index += 1;
                next.state_mut().get_mut::<Path>().set_last(segment);
            }
            Some(Frame::Map(_)) if is_map_key => {
                let path = next.state_mut().get_mut::<Path>();
                match *event.event() {
                    Event::Atom(ref atom) => path.set_last_key(atom),
                    _ => path.set_last(PathSegment::Unknown),
                }
            }
            _ => {}
        }

        next.emit(event)?;

        if let Some(is_map) = is_map {
            self.frames.push(if is_map {
                Frame::Map(false)
            } else {
                Frame::Seq(0)
            });
            next.state_mut()
                .get_mut::<Path>()
                .push(PathSegment::Unknown);
        }
        Ok(())
    }
}
