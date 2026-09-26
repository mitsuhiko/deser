use std::collections::BTreeMap;

use deser::ext::{Datetime, Number};
use deser::hints::Compact;
use deser::{Serialize, State};
use deser_value::{Value, from_value, to_value, value};

#[test]
fn test_json_round_trip() {
    let input = r#"{"z":1,"a":[true,null,-1,1.5,"x"],"m":{"2":{}},"big":123456789012345678901234567890,"exact":0.10000000000000000001}"#;
    let value: Value = deser_json::from_str(input).unwrap();
    // exact numbers are retained as extension values
    assert_eq!(
        value["exact"]
            .downcast_ext_value::<Number>()
            .unwrap()
            .as_str(),
        "0.10000000000000000001"
    );
    assert_eq!(deser_json::to_string(&value).unwrap(), input);
    assert_eq!(deser_json::to_string(&value.clone()).unwrap(), input);
}

#[test]
fn test_f32_precision() {
    // values keep the precision of floats for the formats
    let value = to_value(&vec![0.1f32, 1.5]).unwrap();
    assert_eq!(deser_json::to_string(&value).unwrap(), "[0.1,1.5]");
    assert_eq!(deser_json::to_string(&value.clone()).unwrap(), "[0.1,1.5]");
    assert_eq!(deser_yaml::to_string(&value).unwrap(), "- 0.1\n- 1.5\n");
    let value = to_value(&vec![0.1f64]).unwrap();
    assert_eq!(deser_json::to_string(&value).unwrap(), "[0.1]");
}

#[test]
fn test_toml_round_trip() {
    let input = "title = \"x\"\nwhen = 1979-05-27T07:32:00Z\n\n[owner]\nname = \"Tom\"\n";
    let value: Value = deser_toml::from_str(input).unwrap();
    // date-times are retained as extension values
    assert!(value["when"].downcast_ext::<Datetime>().is_some());
    assert_eq!(deser_toml::to_string(&value).unwrap(), input);
    // and fall back to strings in other formats
    assert_eq!(
        deser_json::to_string(&value).unwrap(),
        r#"{"title":"x","when":"1979-05-27T07:32:00Z","owner":{"name":"Tom"}}"#
    );
}

#[test]
fn test_cbor_tags() {
    let tagged = vec![
        deser_cbor::Tagged::new(1234, "a"),
        deser_cbor::Tagged::new(5678, "b"),
    ];
    let bytes = deser_cbor::to_vec(&tagged).unwrap();
    let value: Value = deser_cbor::from_slice(&bytes).unwrap();
    // the value does not see the tags
    assert_eq!(value, value!(["a", "b"]));
    // but they are retained
    assert_eq!(deser_cbor::to_vec(&value).unwrap(), bytes);
    assert_eq!(deser_cbor::to_vec(&value.clone()).unwrap(), bytes);
    // and can be picked up again
    let back: Vec<deser_cbor::Tagged<String>> = from_value(&value).unwrap();
    assert_eq!(back[0].tag, Some(1234));
    assert_eq!(back[1].tag, Some(5678));
    // also when values are converted
    assert_eq!(
        deser_cbor::to_vec(&to_value(&tagged).unwrap()).unwrap(),
        bytes
    );
}

#[test]
fn test_cbor_keys() {
    let map = BTreeMap::from([(1u32, "one"), (2, "two")]);
    let bytes = deser_cbor::to_vec(&map).unwrap();
    let value: Value = deser_cbor::from_slice(&bytes).unwrap();
    assert_eq!(value.as_map().unwrap().get(&1), Some(&value!("one")));
    assert_eq!(deser_cbor::to_vec(&value).unwrap(), bytes);
    let back: BTreeMap<u32, String> = from_value(&value).unwrap();
    assert_eq!(back[&2], "two");

    let bytes = deser_cbor::to_vec(&Value::bytes(*b"\x00\x01")).unwrap();
    let value: Value = deser_cbor::from_slice(&bytes).unwrap();
    assert_eq!(value.as_bytes(), Some(&b"\x00\x01"[..]));
}

#[test]
fn test_yaml_tags() {
    let input = "a: !custom value\nb: [1, 2]\n";
    let value: Value = deser_yaml::from_str(input).unwrap();
    assert_eq!(value, value!({"a": "value", "b": [1, 2]}));
    // the tag and the flow style are retained
    assert_eq!(deser_yaml::to_string(&value).unwrap(), input);
}

#[test]
fn test_hints() {
    #[derive(Serialize)]
    struct Config {
        #[deser(as = Compact)]
        point: BTreeMap<String, u32>,
    }
    let config = Config {
        point: BTreeMap::from([("x".into(), 1), ("y".into(), 2)]),
    };
    let expected = deser_toml::to_string(&config).unwrap();
    assert_eq!(expected, "point = { x = 1, y = 2 }\n");
    let value = to_value(&config).unwrap();
    assert_eq!(deser_toml::to_string(&value).unwrap(), expected);

    // adapters set hints for values
    #[derive(Serialize)]
    struct Wrapper {
        #[deser(as = Compact)]
        value: Value,
    }
    let wrapper = Wrapper {
        value: value!({"x": 1}),
    };
    assert_eq!(
        deser_toml::to_string(&wrapper).unwrap(),
        "value = { x = 1 }\n"
    );

    // but like for other types, the hints of the value take precedence
    let mut value = value!({"x": 1});
    value
        .meta_mut()
        .event_data_mut()
        .insert(deser::hints::Layout::Expanded);
    assert_eq!(
        deser_toml::to_string(&Wrapper { value }).unwrap(),
        "[value]\nx = 1\n"
    );
}

#[test]
fn test_event_data_is_attached() {
    #[derive(Debug, Default, Clone, PartialEq)]
    struct Marker(u32);

    let mut value = value!([1]);
    value[0].meta_mut().event_data_mut().insert(Marker(42));

    // when serialized
    let mut markers = Vec::new();
    deser::ser::SerializeDriver::new(&value)
        .drive(|_, state: &mut State| {
            markers.push(state.event::<Marker>().cloned());
            Ok(())
        })
        .unwrap();
    assert_eq!(markers, [None, Some(Marker(42)), None]);

    // and when deserialized from it
    struct Probe(Option<Marker>);
    impl<'de> deser::de::Sink<'de> for Probe {
        fn atom(&mut self, _atom: deser::Atom, state: &mut State) -> Result<(), deser::Error> {
            self.0 = state.event::<Marker>().cloned();
            Ok(())
        }
    }
    let mut probe = Probe(None);
    let mut driver = deser::de::DeserializeDriver::from_sink(deser::de::SinkHandle::to(&mut probe));
    deser::de::Deserializer::drive(&mut deser_value::Deserializer::new(&value[0]), &mut driver)
        .unwrap();
    drop(driver);
    assert_eq!(probe.0, Some(Marker(42)));
}

#[test]
fn test_lexical() {
    use deser_value::{Kind, Map};

    let mut map = Map::new();
    map.insert(Kind::Lexical("port".into()), Kind::Lexical("8080".into()));
    map.insert(Kind::Lexical("debug".into()), Kind::Lexical("true".into()));
    let value = Value::from(Kind::Map(map));

    // lexical values compare and hash like strings and are found with them
    assert_eq!(value["port"], "8080");
    assert!(value["port"].is_str() && value["port"].is_lexical());
    assert_eq!(value["port"], Value::from("8080"));

    // they remain lexical and parse into other types
    #[derive(Debug, deser::Deserialize, PartialEq)]
    struct Config {
        port: u16,
        debug: bool,
    }
    assert_eq!(
        from_value::<Config>(&value).unwrap(),
        Config {
            port: 8080,
            debug: true
        }
    );
    assert_eq!(
        from_value::<Config>(&value.clone()).unwrap(),
        Config {
            port: 8080,
            debug: true
        }
    );
    assert!(to_value(&value).unwrap()["port"].is_lexical());

    // the formats write them as strings, YAML quotes what it would read
    // as another type
    assert_eq!(
        deser_json::to_string(&value).unwrap(),
        r#"{"port":"8080","debug":"true"}"#
    );
    assert_eq!(
        deser_yaml::to_string(&value).unwrap(),
        "port: '8080'\ndebug: 'true'\n"
    );
    assert_eq!(
        deser_toml::to_string(&value).unwrap(),
        "port = \"8080\"\ndebug = \"true\"\n"
    );
    let cbor = deser_cbor::to_vec(&value).unwrap();
    let back: Value = deser_cbor::from_slice(&cbor).unwrap();
    assert_eq!(back, value);
    assert!(!back["port"].is_lexical());
}

#[test]
fn test_repeated() {
    use deser::de::DeserializeDriver;
    use deser::{Atom, ContainerShape, Event};

    // the values of a repeated key stay repeated in values
    let mut out = None::<Value>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        for event in [
            Event::map_start(),
            Atom::Lexical("page".into()).into(),
            Event::SeqStart(ContainerShape::new().with_repeated(true)),
            Atom::Lexical("1".into()).into(),
            Atom::Lexical("2".into()).into(),
            Event::SeqEnd,
            Event::MapEnd,
        ] {
            driver.emit(event).unwrap();
        }
    }
    let value = out.unwrap();
    assert!(value["page"].as_seq().unwrap().is_repeated());
    assert!(value.clone()["page"].as_seq().unwrap().is_repeated());

    #[derive(Debug, deser::Deserialize, PartialEq)]
    struct Query {
        page: u32,
    }
    assert_eq!(from_value::<Query>(&value).unwrap(), Query { page: 2 });
    assert_eq!(
        from_value::<Query>(&to_value(&value).unwrap()).unwrap(),
        Query { page: 2 }
    );
}
