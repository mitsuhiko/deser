use deser::ser::for_each_event;
use deser::{Event, Serialize};

fn serialize<T: Serialize>(value: &T) -> Vec<Event<'static>> {
    let mut rv = Vec::new();
    for_each_event(value, |event, _, _| {
        rv.push(event.to_static());
        Ok(())
    })
    .unwrap();
    rv
}

#[test]
fn test_skip_serializing_if() {
    #[derive(Serialize)]
    struct Test {
        required: usize,
        #[deser(skip_serializing_if = "Option::is_none")]
        optional: Option<usize>,
    }

    assert_eq!(
        serialize(&Test {
            required: 42,
            optional: None
        }),
        vec![
            Event::MapStart,
            "required".into(),
            42u64.into(),
            Event::MapEnd,
        ]
    );
}

#[test]
fn test_skip_serializing_optionals_all() {
    #[derive(Serialize)]
    #[deser(skip_serializing_optionals)]
    struct Test {
        a: Option<usize>,
        b: Option<usize>,
        c: Option<usize>,
    }

    assert_eq!(
        serialize(&Test {
            a: Some(1),
            b: Some(2),
            c: Some(3),
        }),
        vec![
            Event::MapStart,
            "a".into(),
            1u64.into(),
            "b".into(),
            2u64.into(),
            "c".into(),
            3u64.into(),
            Event::MapEnd,
        ]
    );
}

#[test]
fn test_skip_serializing_optionals_some() {
    #[derive(Serialize)]
    #[deser(skip_serializing_optionals)]
    struct Test {
        a: Option<usize>,
        b: Option<usize>,
        c: Option<usize>,
        d: (),
    }

    assert_eq!(
        serialize(&Test {
            a: None,
            b: Some(2),
            c: None,
            d: (),
        }),
        vec![Event::MapStart, "b".into(), 2u64.into(), Event::MapEnd]
    );
}

#[test]
fn test_flatten_basics() {
    #[derive(Serialize)]
    struct Test {
        a: usize,
        b: usize,
        #[deser(flatten)]
        inner1: Inner1,
        #[deser(flatten)]
        inner2: Inner2,
        c: usize,
    }

    #[derive(Serialize)]
    struct Inner1 {
        inner1_a: usize,
        inner1_b: usize,
    }

    #[derive(Serialize)]
    struct Inner2 {
        inner2_a: usize,
        inner2_b: usize,
    }

    assert_eq!(
        serialize(&Test {
            a: 1,
            b: 2,
            c: 3,
            inner1: Inner1 {
                inner1_a: 99,
                inner1_b: 100,
            },
            inner2: Inner2 {
                inner2_a: 199,
                inner2_b: 200,
            },
        }),
        vec![
            Event::MapStart,
            "a".into(),
            1u64.into(),
            "b".into(),
            2u64.into(),
            "inner1_a".into(),
            99u64.into(),
            "inner1_b".into(),
            100u64.into(),
            "inner2_a".into(),
            199u64.into(),
            "inner2_b".into(),
            200u64.into(),
            "c".into(),
            3u64.into(),
            Event::MapEnd,
        ]
    );
}

#[test]
fn test_flatten_skip_optionals() {
    #[derive(Serialize)]
    #[deser(skip_serializing_optionals)]
    struct Outer {
        required: bool,
        first: Option<usize>,
        #[deser(flatten)]
        inner: Inner,
    }

    #[derive(Serialize)]
    struct Inner {
        second: Option<usize>,
    }

    assert_eq!(
        serialize(&Outer {
            required: true,
            first: None,
            inner: Inner { second: None }
        }),
        vec![
            Event::MapStart,
            "required".into(),
            true.into(),
            Event::MapEnd,
        ]
    );

    assert_eq!(
        serialize(&Outer {
            required: true,
            first: None,
            inner: Inner { second: Some(111) }
        }),
        vec![
            Event::MapStart,
            "required".into(),
            true.into(),
            "second".into(),
            111u64.into(),
            Event::MapEnd,
        ]
    );
}

#[test]
fn test_flatten_skip_serializing_if() {
    fn not_inner(inner: &Inner) -> bool {
        inner.second == 42
    }

    #[derive(Serialize)]
    struct Outer {
        required: bool,
        #[deser(flatten, skip_serializing_if = "not_inner")]
        inner: Inner,
    }

    #[derive(Serialize)]
    struct Inner {
        second: usize,
    }

    assert_eq!(
        serialize(&Outer {
            required: true,
            inner: Inner { second: 42 }
        }),
        vec![
            Event::MapStart,
            "required".into(),
            true.into(),
            Event::MapEnd,
        ]
    );

    assert_eq!(
        serialize(&Outer {
            required: true,
            inner: Inner { second: 23 }
        }),
        vec![
            Event::MapStart,
            "required".into(),
            true.into(),
            "second".into(),
            23u64.into(),
            Event::MapEnd,
        ]
    );
}
