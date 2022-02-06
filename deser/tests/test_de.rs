use deser::de::Driver;
use deser::{Atom, Event};

#[test]
fn test_optional() {
    let mut out = None::<Option<usize>>;
    {
        let mut driver = Driver::new(&mut out);
        driver.emit(Event::Atom(Atom::U64(42))).unwrap();
    }
    assert_eq!(out, Some(Some(42)));

    let mut out = None::<Option<usize>>;
    {
        let mut driver = Driver::new(&mut out);
        driver.emit(Event::Atom(Atom::Null)).unwrap();
    }
    assert_eq!(out, Some(None));
}
