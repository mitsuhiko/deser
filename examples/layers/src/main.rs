//! This example shows layers: they sit between the values and the format
//! and see (and can change) all events.
//!
//! During serialization, layers can transform the output without the types
//! knowing about it:
//!
//! * [`RenameKeys`] renames the keys of maps (here to camel case),
//! * [`SkipNulls`] drops map entries with null values,
//! * [`Redact`] replaces the values of sensitive keys.
//!
//! During deserialization, the `Limits` layer of deser limits the input and
//! the `PathLayer` of `deser-path` adds the path to errors.
use deser::de::{Format, Limits};
use deser::ser::{Layer, Next};
use deser::{Atom, Descriptor, Deserialize, Error, Event, Serialize};
use deser_path::PathLayer;

/// Renames the string keys of maps.
pub struct RenameKeys(pub fn(&str) -> String);

impl Layer for RenameKeys {
    fn event(
        &mut self,
        event: Event<'_>,
        descriptor: &'static dyn Descriptor,
        next: &mut Next<'_>,
    ) -> Result<(), Error> {
        match event {
            Event::Atom(Atom::Str(ref key)) if next.state().is_map_key() => {
                let key = (self.0)(key);
                next.emit(Event::from(key), descriptor)
            }
            event => next.emit(event, descriptor),
        }
    }
}

/// Converts `snake_case` to `camelCase`.
fn camel_case(key: &str) -> String {
    let mut rv = String::with_capacity(key.len());
    let mut upper = false;
    for c in key.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            rv.extend(c.to_uppercase());
            upper = false;
        } else {
            rv.push(c);
        }
    }
    rv
}

/// Drops map entries with null values.
///
/// The key of an entry is held back until the value is known.
#[derive(Default)]
pub struct SkipNulls {
    key: Option<(Event<'static>, &'static dyn Descriptor)>,
}

impl Layer for SkipNulls {
    fn event(
        &mut self,
        event: Event<'_>,
        descriptor: &'static dyn Descriptor,
        next: &mut Next<'_>,
    ) -> Result<(), Error> {
        if let Some((key, key_descriptor)) = self.key.take() {
            if event == Event::Atom(Atom::Null) {
                return Ok(());
            }
            next.emit_key(key, key_descriptor)?;
        } else if next.state().is_map_key() && matches!(event, Event::Atom(_)) {
            self.key = Some((event.to_static(), descriptor));
            return Ok(());
        }
        next.emit(event, descriptor)
    }
}

/// Replaces the values of the given keys with `"[redacted]"`.
///
/// Maps and sequences are replaced as a whole.
pub struct Redact {
    keys: Vec<&'static str>,
    state: RedactState,
}

enum RedactState {
    Idle,
    /// The next value is redacted.
    Pending,
    /// The events of a redacted map or sequence are dropped, this is the
    /// depth within it.
    Skipping(usize),
}

impl Redact {
    pub fn new(keys: &[&'static str]) -> Redact {
        Redact {
            keys: keys.to_vec(),
            state: RedactState::Idle,
        }
    }
}

impl Layer for Redact {
    fn event(
        &mut self,
        event: Event<'_>,
        descriptor: &'static dyn Descriptor,
        next: &mut Next<'_>,
    ) -> Result<(), Error> {
        match self.state {
            RedactState::Idle => {
                if let Event::Atom(Atom::Str(ref key)) = event {
                    if next.state().is_map_key() && self.keys.contains(&&**key) {
                        self.state = RedactState::Pending;
                    }
                }
                next.emit(event, descriptor)
            }
            RedactState::Pending => {
                self.state = match event {
                    Event::MapStart | Event::SeqStart => RedactState::Skipping(1),
                    _ => RedactState::Idle,
                };
                next.emit(Event::from("[redacted]"), descriptor)
            }
            RedactState::Skipping(ref mut depth) => {
                match event {
                    Event::MapStart | Event::SeqStart => *depth += 1,
                    Event::MapEnd | Event::SeqEnd => {
                        *depth -= 1;
                        if *depth == 0 {
                            self.state = RedactState::Idle;
                        }
                    }
                    Event::Atom(_) => {}
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub user_name: String,
    pub email_address: Option<String>,
    pub password_hash: String,
    pub api_tokens: Vec<String>,
    pub settings: Settings,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Settings {
    pub dark_mode: bool,
    pub time_zone: Option<String>,
}

fn main() {
    let user = User {
        user_name: "jdoe".into(),
        email_address: None,
        password_hash: "$argon2id$...".into(),
        api_tokens: vec!["tok-1".into(), "tok-2".into()],
        settings: Settings {
            dark_mode: true,
            time_zone: None,
        },
    };

    println!("plain:");
    let json = deser_json::to_string(&user).unwrap();
    println!("{}", json);

    println!();
    println!("with layers:");
    let json = deser_json::SerializerConfig::new()
        .to_string_with(&user, |driver| {
            driver.push_layer(SkipNulls::default());
            driver.push_layer(Redact::new(&["password_hash", "api_tokens"]));
            driver.push_layer(RenameKeys(camel_case));
        })
        .unwrap();
    println!("{}", json);
    assert_eq!(
        json,
        r#"{"userName":"jdoe","passwordHash":"[redacted]","apiTokens":"[redacted]","settings":{"darkMode":true}}"#
    );

    // during deserialization, the limits layer rejects input that is too
    // large and the path layer adds the path to errors.
    println!();
    println!("deserialization errors:");
    let input = r#"{
        "user_name": "jdoe",
        "email_address": null,
        "password_hash": "x",
        "api_tokens": ["a", "b", "c", "d", "e", "f"],
        "settings": {"dark_mode": "yes", "time_zone": null}
    }"#;
    let parse = |limits: Limits| {
        deser_json::Deserializer::from_str(input).deserialize_with::<User, _>(|driver| {
            // the path layer comes first so that the errors of the limits
            // layer get the path of the event it rejects
            driver.push_layer(PathLayer::new());
            driver.push_layer(limits);
        })
    };
    let err = parse(Limits::new().max_items(5)).unwrap_err();
    println!("{}", err);
    assert_eq!(err.path(), Some("api_tokens[5]"));
    let err = parse(Limits::new()).unwrap_err();
    println!("{}", err);
    assert_eq!(err.path(), Some("settings.dark_mode"));
}
