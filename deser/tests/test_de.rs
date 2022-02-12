use std::borrow::Cow;
use std::sync::atomic::{self, AtomicUsize};

use deser::de::{DeserializeDriver, Sink, SinkHandle};
use deser::{make_slot_wrapper, Atom, Deserialize, Event};

fn deserialize<T: Deserialize>(events: Vec<Event<'_>>) -> T {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in events {
            driver.emit(event).unwrap();
        }
    }
    out.unwrap()
}

#[test]
fn test_optional() {
    let mut out = None::<Option<usize>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit(Event::Atom(Atom::U64(42))).unwrap();
    }
    assert_eq!(out, Some(Some(42)));

    let mut out = None::<Option<usize>>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.emit(Event::Atom(Atom::Null)).unwrap();
    }
    assert_eq!(out, Some(None));
}

#[test]
fn test_tuples() {
    let s: (u32, u32) = deserialize(vec![
        Event::SeqStart,
        1u64.into(),
        2u64.into(),
        Event::SeqEnd,
    ]);
    assert_eq!(s.0, 1);
    assert_eq!(s.1, 2);
}

#[test]
#[should_panic = "too many elements in tuple"]
fn test_tuples_too_many_elements() {
    let _: (u32, u32) = deserialize(vec![
        Event::SeqStart,
        1u64.into(),
        2u64.into(),
        "extra".into(),
        Event::SeqEnd,
    ]);
}

#[test]
#[should_panic = "not enough elements in tuple"]
fn test_tuples_not_enough_elements() {
    let _: (u32, u32) = deserialize(vec![Event::SeqStart, 1u64.into(), Event::SeqEnd]);
}

#[test]
fn test_array_basic() {
    let arr: [u16; 4] = deserialize(vec![
        Event::SeqStart,
        1u64.into(),
        2u64.into(),
        3u64.into(),
        4u64.into(),
        Event::SeqEnd,
    ]);
    assert_eq!(arr, [1, 2, 3, 4]);
}

#[test]
#[should_panic = "too many elements in array"]
fn test_array_too_many_elements() {
    let _: [u16; 4] = deserialize(vec![
        Event::SeqStart,
        1u64.into(),
        2u64.into(),
        3u64.into(),
        4u64.into(),
        5u64.into(),
        Event::SeqEnd,
    ]);
}

#[test]
#[should_panic = "not enough elements in array"]
fn test_array_not_enough_elements() {
    let _: [u16; 4] = deserialize(vec![
        Event::SeqStart,
        1u64.into(),
        2u64.into(),
        3u64.into(),
        Event::SeqEnd,
    ]);
}

#[test]
fn test_array_dropping_on_error() {
    // this is important since we're doing unsafe shit in the array serializer.  If stuff gets
    // wrong it needs to make sure drop is called.
    static DROP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct X;

    impl Drop for X {
        fn drop(&mut self) {
            DROP_COUNTER.fetch_add(1, atomic::Ordering::Relaxed);
        }
    }

    make_slot_wrapper!(SlotWrapper);

    impl Deserialize for X {
        fn deserialize_into(out: &mut Option<Self>) -> SinkHandle {
            SlotWrapper::make_handle(out)
        }
    }

    impl Sink for SlotWrapper<X> {
        fn atom(
            &mut self,
            _atom: Atom,
            _state: &deser::de::DeserializerState,
        ) -> Result<(), deser::Error> {
            **self = Some(X);
            Ok(())
        }
    }

    std::panic::catch_unwind(|| {
        let _: [X; 4] = deserialize(vec![
            Event::SeqStart,
            1u64.into(),
            2u64.into(),
            3u64.into(),
            Event::SeqEnd,
        ]);
    })
    .ok();

    assert_eq!(DROP_COUNTER.load(atomic::Ordering::Relaxed), 3);
}

#[test]
fn test_byte_array() {
    let x: [u8; 4] = deserialize(vec![
        Event::SeqStart,
        0u64.into(),
        1u64.into(),
        2u64.into(),
        3u64.into(),
        Event::SeqEnd,
    ]);
    assert_eq!(x, [0, 1, 2, 3]);

    let x: [u8; 4] = deserialize(vec![Event::Atom(Atom::Bytes(Cow::Borrowed(
        &b"\x00\x01\x02\x03"[..],
    )))]);
    assert_eq!(x, [0, 1, 2, 3]);
}

#[test]
#[should_panic = "byte array of wrong length"]
fn test_byte_array_wrong_length() {
    let _: [u8; 4] = deserialize(vec![Event::Atom(Atom::Bytes(Cow::Borrowed(&b"012"[..])))]);
}

#[test]
fn test_chars() {
    let x: char = deserialize(vec!['x'.into()]);
    assert_eq!(x, 'x');
    let x: char = deserialize(vec!["x".into()]);
    assert_eq!(x, 'x');
}

#[test]
#[should_panic = "unexpected string, expected char"]
fn test_chars_long_string() {
    let _: char = deserialize(vec!["Harry".into()]);
}

#[test]
fn test_box() {
    let x: Box<u64> = deserialize(vec![0u64.into()]);
    assert_eq!(*x, 0);
}
