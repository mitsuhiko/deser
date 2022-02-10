use std::borrow::Cow;

use deser::de::{DeserializerState, Driver, Sink, SinkHandle};
use deser::ser::{Chunk, SerializeHandle, SerializerState, StructEmitter};
use deser::{Descriptor, Deserialize, Error, ErrorKind, Event, Serialize};
use deser_debug::ToDebug;

pub struct User {
    id: usize,
    email_address: String,
    attrs: Attrs,
}

#[derive(Default, Serialize, Deserialize)]
struct Attrs {
    is_special: bool,
}

impl Serialize for User {
    fn descriptor(&self) -> &dyn Descriptor {
        &UserDescriptor
    }

    fn serialize(&self, state: &SerializerState) -> Result<Chunk, Error> {
        Ok(Chunk::Struct(Box::new(UserEmitter {
            user: self,
            index: 0,
            attrs_emitter: match self.attrs.serialize(state)? {
                Chunk::Struct(emitter) => emitter,
                _ => unimplemented!(),
            },
        })))
    }
}

struct UserDescriptor;

impl Descriptor for UserDescriptor {
    fn name(&self) -> Option<&str> {
        Some("User")
    }
}

struct UserEmitter<'a> {
    user: &'a User,
    index: usize,
    attrs_emitter: Box<dyn StructEmitter + 'a>,
}

impl<'a> StructEmitter for UserEmitter<'a> {
    fn next(&mut self) -> Option<(Cow<'_, str>, SerializeHandle)> {
        let index = self.index;
        self.index += 1;
        match index {
            0 => Some((Cow::Borrowed("id"), SerializeHandle::to(&self.user.id))),
            1 => Some((
                Cow::Borrowed("emailAddress"),
                SerializeHandle::to(&self.user.email_address),
            )),
            2 => {
                self.index -= 1;
                self.attrs_emitter.next()
            }
            _ => None,
        }
    }
}

impl Deserialize for User {
    fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
        SinkHandle::boxed(UserSink {
            out,
            key: None,
            id: None,
            email_address: None,
        })
    }
}

struct UserSink<'a> {
    out: &'a mut Option<User>,
    key: Option<String>,
    id: Option<usize>,
    email_address: Option<String>,
}

impl<'a> Sink for UserSink<'a> {
    fn descriptor(&self) -> &dyn Descriptor {
        &UserDescriptor
    }

    fn map(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        Ok(())
    }

    fn next_key(&mut self) -> Result<SinkHandle, Error> {
        Ok(Deserialize::deserialize_into(&mut self.key))
    }

    fn next_value(&mut self) -> Result<SinkHandle, Error> {
        match self.key.take().as_deref() {
            Some("id") => Ok(Deserialize::deserialize_into(&mut self.id)),
            Some("emailAddress") => Ok(Deserialize::deserialize_into(&mut self.email_address)),
            _ => Ok(SinkHandle::null()),
        }
    }

    fn finish(&mut self, _state: &DeserializerState) -> Result<(), Error> {
        *self.out = Some(User {
            id: self
                .id
                .take()
                .ok_or_else(|| Error::new(ErrorKind::MissingField, "missing field"))?,
            email_address: self
                .email_address
                .take()
                .ok_or_else(|| Error::new(ErrorKind::MissingField, "missing field"))?,
            attrs: Attrs::default(),
        });
        Ok(())
    }
}

fn main() {
    let mut user = None::<User>;
    {
        let mut driver = Driver::new(&mut user);
        driver.emit(Event::MapStart).unwrap();
        driver.emit("id").unwrap();
        driver.emit(23u64).unwrap();
        driver.emit("emailAddress").unwrap();
        driver.emit("jane@example.com").unwrap();
        driver.emit(Event::MapEnd).unwrap();
    }
    println!("{:#?}", ToDebug::new(&user.unwrap()));
}
