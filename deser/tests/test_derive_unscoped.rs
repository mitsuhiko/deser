#[test]
fn test_unscoped() {
    #[derive(deser::Serialize, deser::Deserialize)]
    #[deser(skip_serializing_optionals)]
    pub struct Root {
        flag: Option<bool>,
        #[deser(flatten)]
        attrs: Attrs,
    }

    #[derive(deser::Serialize, deser::Deserialize)]
    #[deser(skip_serializing_optionals)]
    pub struct Attrs {
        is_active: bool,
        #[deser(skip_serializing_if = is_false)]
        is_stuff: bool,
    }

    fn is_false(value: &bool) -> bool {
        !*value
    }

    let root = Root {
        flag: None,
        attrs: Attrs {
            is_active: true,
            is_stuff: false,
        },
    };

    // roundtrip through the event stream to make sure the generated code
    // works without deser being imported.
    let mut events = Vec::new();
    let mut driver = deser::ser::SerializeDriver::new(&root);
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(event.to_static());
    }
    assert_eq!(
        events,
        vec![
            deser::Event::MapStart,
            "is_active".into(),
            true.into(),
            deser::Event::MapEnd,
        ]
    );

    let mut out = None::<Root>;
    {
        let mut driver = deser::de::DeserializeDriver::new(&mut out);
        for event in [
            deser::Event::MapStart,
            "is_active".into(),
            true.into(),
            "is_stuff".into(),
            false.into(),
            deser::Event::MapEnd,
        ] {
            driver.emit(event).unwrap();
        }
    }
    let rv = out.unwrap();
    assert_eq!(rv.flag, None);
    assert!(rv.attrs.is_active);
    assert!(!rv.attrs.is_stuff);
}
