//! The YAML parser: turns text into [`Event`]s.
use deser::{Error, ErrorKind};

use crate::event::{Event, EventKind, Mark};

pub struct Parser<'a> {
    #[allow(dead_code)]
    input: &'a str,
    state: State,
}

enum State {
    StreamStart,
    Body,
    Done,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Parser<'a> {
        Parser {
            input,
            state: State::StreamStart,
        }
    }

    /// Returns the next event.
    ///
    /// After [`EventKind::StreamEnd`] or an error, no more events must be
    /// requested.
    pub fn next_event(&mut self) -> Result<Event<'a>, Error> {
        match self.state {
            State::StreamStart => {
                self.state = State::Body;
                Ok(Event {
                    kind: EventKind::StreamStart,
                    start: Mark::default(),
                    end: Mark::default(),
                })
            }
            State::Body => {
                self.state = State::Done;
                Err(syntax_error(
                    Mark::default(),
                    "YAML parsing is not implemented yet",
                ))
            }
            State::Done => panic!("next_event called after the end of the stream"),
        }
    }
}

#[cold]
pub fn syntax_error(mark: Mark, msg: &str) -> Error {
    Error::new(
        ErrorKind::Unexpected,
        format!(
            "syntax error at line {} column {}: {}",
            mark.line + 1,
            mark.column + 1,
            msg
        ),
    )
}
