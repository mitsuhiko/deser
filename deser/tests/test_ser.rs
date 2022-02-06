use deser::ser::for_each_event;
use deser::{Atom, Event, Serialize};

fn capture_events(s: &dyn Serialize) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    for_each_event(s, |event, _, _| {
        events.push(event.to_static());
        Ok(())
    })
    .unwrap();
    events
}

#[test]
fn test_optional() {
    let events = capture_events(&None::<usize>);
    assert_eq!(events, vec![Event::Atom(Atom::Null)]);

    let events = capture_events(&Some(42usize));
    assert_eq!(events, vec![Event::Atom(Atom::U64(42))]);
}
