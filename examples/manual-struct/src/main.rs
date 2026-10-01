//! Implementing `Serialize` and `Deserialize` by hand, which is what the
//! derive generates for you.
//!
//! Serialization hands out a `StructEmitter` which yields the fields one by
//! one, deserialization a `Sink` which receives them.  Neither recurses:
//! the nested values are handed back to the driver as handles.  The sink is
//! fed with events directly through a `DeserializeDriver`, no data format
//! is involved.
use std::borrow::Cow;

use deser::State;
use deser::de::{DeserializeDriver, Sink, SinkHandle};
use deser::ser::{Describe, Emit, SerializeHandle, StructEmitter};
use deser::{Deserialize, Error, ErrorKind, Event, Serialize};
use deser_debug::ToDebug;

pub struct User {
    id: usize,
    email_address: String,
}

impl Serialize for User {
    fn describe(_value: &Self, d: &mut dyn Describe) {
        d.structure("User");
    }

    fn serialize<'a>(value: &'a Self, state: &mut State) -> Result<Emit<'a>, Error> {
        Ok(Emit::structure(
            UserEmitter {
                user: value,
                index: 0,
            },
            state,
        ))
    }
}

struct UserEmitter<'a> {
    user: &'a User,
    index: usize,
}

impl<'a> StructEmitter for UserEmitter<'a> {
    fn next(
        &mut self,
        _state: &mut State,
    ) -> Result<Option<(Cow<'_, str>, SerializeHandle<'_>)>, Error> {
        let index = self.index;
        self.index += 1;
        Ok(match index {
            0 => Some((Cow::Borrowed("id"), SerializeHandle::to(&self.user.id))),
            1 => Some((
                Cow::Borrowed("emailAddress"),
                SerializeHandle::to(&self.user.email_address),
            )),
            _ => None,
        })
    }
}

impl<'de> Deserialize<'de> for User {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        SinkHandle::arena(
            UserSink {
                out,
                key: None,
                id: None,
                email_address: None,
            },
            state,
        )
    }
}

struct UserSink<'a> {
    out: &'a mut Option<User>,
    key: Option<String>,
    id: Option<usize>,
    email_address: Option<String>,
}

impl<'a, 'de> Sink<'de> for UserSink<'a> {
    fn expecting(&self) -> Cow<'_, str> {
        Cow::Borrowed("User")
    }

    fn map(&mut self, _state: &mut State) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        Ok(String::deserialize_into(&mut self.key, state))
    }

    fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, Error> {
        match self.key.take().as_deref() {
            Some("id") => Ok(usize::deserialize_into(&mut self.id, state)),
            Some("emailAddress") => Ok(String::deserialize_into(&mut self.email_address, state)),
            _ => Ok(SinkHandle::null()),
        }
    }

    fn finish(&mut self, _state: &mut State) -> Result<(), Error> {
        *self.out = Some(User {
            id: self
                .id
                .take()
                .ok_or_else(|| Error::new(ErrorKind::MissingField, "missing field"))?,
            email_address: self
                .email_address
                .take()
                .ok_or_else(|| Error::new(ErrorKind::MissingField, "missing field"))?,
        });
        Ok(())
    }
}

fn main() {
    let mut user = None::<User>;
    {
        let mut driver = DeserializeDriver::new(&mut user);
        driver.emit(Event::map_start()).unwrap();
        driver.emit("id").unwrap();
        driver.emit(23u64).unwrap();
        driver.emit("emailAddress").unwrap();
        driver.emit("jane@example.com").unwrap();
        driver.emit(Event::MapEnd).unwrap();
    }
    println!("{:#?}", ToDebug::new(&user.unwrap()));
}
