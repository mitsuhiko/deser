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
