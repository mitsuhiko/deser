//! This library takes a [`Serialize`](deser::ser::Serialize) and
//! formats it with [`std::fmt`] to debug representation.
use std::fmt;
use std::sync::atomic::{self, AtomicUsize};

use deser::ser::{for_each_event, Serialize};
use deser::Event;

/// Serializes a serializable value to `Debug` format.
pub struct ToDebug {
    events: Vec<(Event<'static>, Option<String>)>,
}

impl fmt::Display for ToDebug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&Helper(&self.events, AtomicUsize::default()), f)
    }
}

impl fmt::Debug for ToDebug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&Helper(&self.events, AtomicUsize::default()), f)
    }
}

impl ToDebug {
    /// Creates a new [`ToDebug`] object from a serializable value.
    pub fn new(value: &dyn Serialize) -> ToDebug {
        let mut events = Vec::new();
        for_each_event(value, |event, descriptor, _| {
            events.push((event.to_static(), descriptor.name().map(|x| x.to_string())));
            Ok(())
        })
        .unwrap();
        ToDebug { events }
    }
}

fn dump<'a, 'f>(
    tokens: &'a [(Event<'a>, Option<String>)],
    f: &'f mut fmt::Formatter<'_>,
) -> Result<&'a [(Event<'a>, Option<String>)], fmt::Error> {
    if let Some((first, mut rest)) = tokens.split_first() {
        match first.0 {
            Event::Null => fmt::Debug::fmt(&(), f)?,
            Event::Bool(v) => fmt::Debug::fmt(&v, f)?,
            Event::Str(ref v) => fmt::Debug::fmt(v, f)?,
            Event::Bytes(ref v) => {
                write!(f, "b\"")?;
                for &b in &v[..] {
                    if b == b'\n' {
                        write!(f, "\\n")?;
                    } else if b == b'\r' {
                        write!(f, "\\r")?;
                    } else if b == b'\t' {
                        write!(f, "\\t")?;
                    } else if b == b'\\' || b == b'"' {
                        write!(f, "\\{}", b as char)?;
                    } else if b == b'\0' {
                        write!(f, "\\0")?;
                    } else if (0x20..0x7f).contains(&b) {
                        write!(f, "{}", b as char)?;
                    } else {
                        write!(f, "\\x{:02x}", b)?;
                    }
                }
                write!(f, "\"")?;
            }
            Event::U64(v) => fmt::Debug::fmt(&v, f)?,
            Event::I64(v) => fmt::Debug::fmt(&v, f)?,
            Event::F64(v) => fmt::Debug::fmt(&v, f)?,
            Event::MapStart => {
                if let Some(ref name) = first.1 {
                    write!(f, "{} ", name)?;
                }
                let mut map = f.debug_map();
                let mut is_key = true;
                loop {
                    if rest.get(0).map_or(false, |x| matches!(x.0, Event::MapEnd)) {
                        rest = &rest[1..];
                        break;
                    }
                    let inner = Helper(rest, AtomicUsize::default());
                    if is_key {
                        map.key(&inner);
                    } else {
                        map.value(&inner);
                    }
                    is_key = !is_key;
                    rest = &rest[inner.1.load(atomic::Ordering::Relaxed)..];
                }
                map.finish()?;
            }
            Event::MapEnd => unreachable!(),
            Event::SeqStart => {
                if let Some(ref name) = first.1 {
                    if name != "Vec" && name != "slice" {
                        write!(f, "{} ", name)?;
                    }
                }
                let mut list = f.debug_list();
                loop {
                    if rest.get(0).map_or(false, |x| matches!(x.0, Event::SeqEnd)) {
                        rest = &rest[1..];
                        break;
                    }
                    let inner = Helper(rest, AtomicUsize::default());
                    list.entry(&inner);
                    rest = &rest[inner.1.load(atomic::Ordering::Relaxed)..];
                }
                list.finish()?;
            }
            Event::SeqEnd => unreachable!(),
        }
        Ok(rest)
    } else {
        Ok(tokens)
    }
}

struct Helper<'a>(&'a [(Event<'a>, Option<String>)], AtomicUsize);

impl<'a> fmt::Debug for Helper<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let new = dump(self.0, f)?;
        self.1
            .store(self.0.len() - new.len(), atomic::Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn test_debug_format() {
    let mut m = std::collections::BTreeMap::new();
    m.insert(true, vec![vec![&b"x"[..], b"yyy"], vec![b"zzzz\x00\x01"]]);
    m.insert(false, vec![]);

    assert_eq!(
        ToDebug::new(&m).to_string(),
        "BTreeMap {false: [], true: [[b\"x\", b\"yyy\"], [b\"zzzz\\0\\x01\"]]}"
    );
}
