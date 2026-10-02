use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::atomic::{self, AtomicUsize};

use deser::de::{DeserializeDriver, DeserializeOwned, Sink, SinkHandle, Slot, default_atom};
use deser::{Atom, Deserialize, Event, State, Text};

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> T {
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
fn test_floats() {
    // single precision floats are accepted by both float types
    assert_eq!(
        deserialize::<f32>(vec![Event::Atom(Atom::F32(0.1))]),
        0.1f32
    );
    assert_eq!(
        deserialize::<f64>(vec![Event::Atom(Atom::F32(0.1))]),
        f64::from(0.1f32)
    );
    assert_eq!(
        deserialize::<f32>(vec![Event::Atom(Atom::F64(0.5))]),
        0.5f32
    );
    assert_eq!(deserialize::<f32>(vec![0.25f32.into()]), 0.25f32);
}

#[test]
fn test_float_atom_paths() {
    use deser::ext::{ExtValue, Number};

    let number = Number::parse("1.25").unwrap();
    for (atom, expected) in [
        (Atom::U64(42), 42.0),
        (Atom::I64(-42), -42.0),
        (Atom::F32(0.25), 0.25),
        (Atom::F64(0.5), 0.5),
        (Atom::F64(f64::INFINITY), f64::INFINITY),
        (Atom::Lexical(Text::borrowed("1.5")), 1.5),
        (Atom::Lexical(Text::owned("-1.5")), -1.5),
        (Atom::Ext(ExtValue::owned(42u128)), 42.0),
        (Atom::Ext(ExtValue::owned(-42i128)), -42.0),
        (Atom::Ext(ExtValue::borrowed_value::<Number>(&number)), 1.25),
        (
            Atom::Ext(ExtValue::owned_value::<Number>(number.clone())),
            1.25,
        ),
    ] {
        // Exercise both the root sink and the inlined container atom path.
        assert_eq!(
            deserialize::<f32>(vec![atom.clone().into()]),
            expected as f32
        );
        assert_eq!(deserialize::<f64>(vec![atom.clone().into()]), expected);
        let events = vec![Event::seq_start(), atom.into(), Event::SeqEnd];
        assert_eq!(deserialize::<Vec<f32>>(events.clone()), [expected as f32]);
        assert_eq!(deserialize::<Vec<f64>>(events), [expected]);
    }
    for atom in [Atom::F32(-0.0), Atom::F64(-0.0)] {
        assert!(deserialize::<f32>(vec![atom.clone().into()]).is_sign_negative());
        assert!(deserialize::<f64>(vec![atom.into()]).is_sign_negative());
    }
    for atom in [Atom::F32(f32::NAN), Atom::F64(f64::NAN)] {
        assert!(deserialize::<f32>(vec![atom.clone().into()]).is_nan());
        assert!(deserialize::<f64>(vec![atom.into()]).is_nan());
    }

    for atom in [
        Atom::Lexical(Text::owned("not a float")),
        Atom::Str(Text::owned("1.5")),
        Atom::Bool(true),
        Atom::Null,
    ] {
        let mut out = None::<f32>;
        assert!(DeserializeDriver::new(&mut out).emit(atom.clone()).is_err());
        assert!(out.is_none());
        let mut out = None::<f64>;
        assert!(DeserializeDriver::new(&mut out).emit(atom).is_err());
        assert!(out.is_none());
    }
}

#[test]
fn test_f32_fallback() {
    // sinks that only know `F64` get single precision floats widened
    struct F64Only(f64);

    impl<'de> Deserialize<'de> for F64Only {
        fn deserialize_atom(
            slot: &mut Slot<Self>,
            atom: Atom,
            state: &mut deser::State,
        ) -> Result<(), deser::Error> {
            match atom {
                Atom::F64(value) => {
                    slot.set(F64Only(value));
                    Ok(())
                }
                other => default_atom(slot, other, state),
            }
        }
    }

    let value = deserialize::<F64Only>(vec![Event::Atom(Atom::F32(1.5))]);
    assert_eq!(value.0, 1.5);

    // without `expecting` errors name the type
    let mut out = None::<F64Only>;
    let mut driver = DeserializeDriver::new(&mut out);
    let err = driver.emit(Event::Atom(Atom::Bool(true))).unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidType: unexpected bool, expected F64Only"
    );

    // integers do not accept floats of either precision
    let mut out = None::<u32>;
    let mut driver = DeserializeDriver::new(&mut out);
    let err = driver.emit(Event::Atom(Atom::F32(1.0))).unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidType: unexpected float, expected u32"
    );
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
        Event::seq_start(),
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
        Event::seq_start(),
        1u64.into(),
        2u64.into(),
        "extra".into(),
        Event::SeqEnd,
    ]);
}

#[test]
#[should_panic = "not enough elements in tuple"]
fn test_tuples_not_enough_elements() {
    let _: (u32, u32) = deserialize(vec![Event::seq_start(), 1u64.into(), Event::SeqEnd]);
}

#[test]
fn test_array_basic() {
    let arr: [u16; 4] = deserialize(vec![
        Event::seq_start(),
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
        Event::seq_start(),
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
        Event::seq_start(),
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

    impl<'de> Deserialize<'de> for X {
        fn deserialize_atom(
            slot: &mut Slot<Self>,
            _atom: Atom,
            _state: &mut deser::State,
        ) -> Result<(), deser::Error> {
            slot.set(X);
            Ok(())
        }
    }

    std::panic::catch_unwind(|| {
        let _: [X; 4] = deserialize(vec![
            Event::seq_start(),
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
        Event::seq_start(),
        0u64.into(),
        1u64.into(),
        2u64.into(),
        3u64.into(),
        Event::SeqEnd,
    ]);
    assert_eq!(x, [0, 1, 2, 3]);

    let x: [u8; 4] = deserialize(vec![Event::Atom(Atom::Bytes(deser::Bytes::new(
        Cow::Borrowed(&b"\x00\x01\x02\x03"[..]),
    )))]);
    assert_eq!(x, [0, 1, 2, 3]);
}

#[test]
#[should_panic = "byte array of wrong length"]
fn test_byte_array_wrong_length() {
    let _: [u8; 4] = deserialize(vec![Event::Atom(Atom::Bytes(deser::Bytes::new(
        Cow::Borrowed(&b"012"[..]),
    )))]);
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

#[test]
fn test_set() {
    let x: BTreeSet<String> = deserialize(vec![
        Event::seq_start(),
        "foo".into(),
        "bar".into(),
        Event::SeqEnd,
    ]);
    let mut set = BTreeSet::new();
    set.insert("foo".into());
    set.insert("bar".into());
    assert_eq!(x, set);
}

#[test]
fn test_string_lengths() {
    for len in 0..40 {
        for base in ["abcdefghij", "äöü日本"] {
            let s: String = base.chars().cycle().take(len).collect();
            let borrowed: String = deserialize(vec![Event::from(s.as_str())]);
            assert_eq!(borrowed, s);
            let owned: String = deserialize(vec![Event::from(s.clone())]);
            assert_eq!(owned, s);
        }
    }
}

/// Runs events through a driver, stops at the first error.
///
/// If `plain` is set, the root sink is wrapped in a sink that only forwards
/// the public methods, the elements of the sequence are then not built
/// inline.
fn run_events<T: DeserializeOwned + std::fmt::Debug>(
    events: &[Event<'static>],
    plain: bool,
    collect: bool,
) -> String {
    struct Plain<'a, 'de>(SinkHandle<'a, 'de>);

    impl<'de> Sink<'de> for Plain<'_, 'de> {
        fn atom(&mut self, atom: Atom, state: &mut State) -> Result<(), deser::Error> {
            self.0.atom(atom, state)
        }
        fn seq(&mut self, state: &mut State) -> Result<(), deser::Error> {
            self.0.seq(state)
        }
        fn map(&mut self, state: &mut State) -> Result<(), deser::Error> {
            self.0.map(state)
        }
        fn next_value(&mut self, state: &mut State) -> Result<SinkHandle<'_, 'de>, deser::Error> {
            self.0.next_value(state)
        }
        fn finish(&mut self, state: &mut State) -> Result<(), deser::Error> {
            self.0.finish(state)
        }
        fn recover(&mut self, err: deser::Error, state: &mut State) -> Result<(), deser::Error> {
            self.0.recover(err, state)
        }
        fn expecting(&self) -> Cow<'_, str> {
            self.0.expecting()
        }
    }

    let mut out = None::<T>;
    let mut log = Vec::new();
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.state_mut().set_collect_errors(collect);
        if plain {
            driver.wrap_sink(|sink, _| SinkHandle::heap(Plain(sink)));
        }
        for (idx, event) in events.iter().enumerate() {
            driver.state_mut().set_input_range(idx, idx + 1);
            if let Err(err) = driver.emit(event.clone()) {
                log.push(format!("{idx}: {:?} {err:#}", err.kind()));
                break;
            }
        }
    }
    log.push(format!("{out:?}"));
    log.join("\n")
}

#[test]
fn test_inline_elements() {
    fn seq(items: Vec<Event<'static>>) -> Vec<Event<'static>> {
        let mut rv = vec![Event::seq_start()];
        rv.extend(items);
        rv.push(Event::SeqEnd);
        rv
    }
    let pair = |a: Event<'static>, b: Event<'static>| seq(vec![a, b]);
    let ok = pair(1u64.into(), 2u64.into());
    let cases: Vec<Vec<Event<'static>>> = vec![
        vec![],
        ok.clone(),
        [ok.clone(), ok.clone()].concat(),
        // too many and not enough elements
        seq(vec![1u64.into(), 2u64.into(), 3u64.into()]),
        seq(vec![1u64.into()]),
        seq(vec![]),
        // atoms of the wrong type or out of range
        pair("x".into(), 2u64.into()),
        pair(1u64.into(), 300u64.into()),
        pair(Atom::Null.into(), 2u64.into()),
        pair(Atom::Lexical(Text::borrowed("7")).into(), 2u64.into()),
        // containers as items
        pair(seq(vec![1u64.into()])[0].clone(), 2u64.into()),
        seq(vec![
            Event::seq_start(),
            1u64.into(),
            Event::SeqEnd,
            2u64.into(),
        ]),
        seq(vec![
            Event::map_start(),
            "a".into(),
            1u64.into(),
            Event::MapEnd,
            2u64.into(),
        ]),
        seq(vec![
            1u64.into(),
            2u64.into(),
            Event::seq_start(),
            Event::SeqEnd,
        ]),
        // elements that are not sequences
        vec![Event::map_start(), Event::MapEnd],
        vec![Event::from(42u64)],
        vec![Event::from("x")],
        vec![Event::Atom(Atom::Null)],
    ];
    for case in cases {
        // the element is followed by a valid one, miri (which is slow) only
        // checks it between valid ones
        let wrapped = [
            seq([ok.clone(), case.clone(), ok.clone()].concat()),
            seq(case.clone()),
            seq([case.clone(), ok.clone()].concat()),
        ];
        for events in &wrapped[..if cfg!(miri) { 1 } else { 3 }] {
            for collect in [false, true] {
                let inline = run_events::<Vec<[u32; 2]>>(events, false, collect);
                let plain = run_events::<Vec<[u32; 2]>>(events, true, collect);
                assert_eq!(inline, plain, "{events:?}");
                let inline = run_events::<Vec<(u32, u8)>>(events, false, collect);
                let plain = run_events::<Vec<(u32, u8)>>(events, true, collect);
                assert_eq!(inline, plain, "{events:?}");
            }
        }
    }
    assert_eq!(
        run_events::<Vec<(u32, u8)>>(&seq([ok.clone(), ok.clone()].concat()), false, false),
        "Some([(1, 2), (1, 2)])"
    );
}
