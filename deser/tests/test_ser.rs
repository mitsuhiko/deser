use std::borrow::Cow;
use std::collections::BTreeSet;

use deser::ser::SerializeDriver;
use deser::{Atom, Event, Serialize};

fn capture_events(s: &dyn Serialize) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(s);
    while let Some((event, _)) = driver.next().unwrap() {
        events.push(event.to_static());
    }
    events
}

#[test]
fn test_optional() {
    let events = capture_events(&None::<usize>);
    assert_eq!(events, vec![Event::Atom(Atom::Null)]);

    let events = capture_events(&Some(42usize));
    assert_eq!(events, vec![Event::Atom(Atom::U64(42))]);
}

#[test]
fn test_tuples() {
    let events = capture_events(&(1, 2, 3));
    assert_eq!(
        events,
        vec![
            Event::SeqStart,
            1i64.into(),
            2i64.into(),
            3i64.into(),
            Event::SeqEnd,
        ]
    );
}

#[test]
fn test_unit() {
    let events = capture_events(&());
    assert_eq!(events, vec![Event::Atom(Atom::Null)]);
}

#[test]
fn test_array() {
    let events = capture_events(&[1i64, 2, 3, 4]);
    assert_eq!(
        events,
        vec![
            Event::SeqStart,
            1i64.into(),
            2i64.into(),
            3i64.into(),
            4i64.into(),
            Event::SeqEnd
        ]
    );

    let events = capture_events(b"Hello");
    assert_eq!(
        events,
        vec![Event::Atom(Atom::Bytes(Cow::Borrowed(b"Hello")))]
    );
}

#[test]
fn test_chars() {
    let events = capture_events(&'x');
    assert_eq!(events, vec!['x'.into()]);
}

#[test]
fn test_refs() {
    let events = capture_events(&&&&42u64);
    assert_eq!(events, vec![42u64.into()]);
}

#[test]
fn test_box() {
    let events = capture_events(&Box::new(true));
    assert_eq!(events, vec![true.into()]);
}

#[test]
fn test_set() {
    let mut set = BTreeSet::new();
    set.insert("foo");
    set.insert("bar");
    let events = capture_events(&set);
    assert_eq!(
        events,
        vec![Event::SeqStart, "bar".into(), "foo".into(), Event::SeqEnd]
    );
}
