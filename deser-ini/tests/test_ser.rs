use std::collections::BTreeMap;

use deser::{Deserialize, ErrorKind, Serialize};
use deser_ini::{
    Continuation, DeserializerConfig, InlineComments, Quotes, Serializer, SerializerConfig,
    from_str, to_string,
};

#[test]
fn test_struct() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Config {
        server: Server,
        name: String,
        debug: bool,
        ratio: f64,
        port: Option<u16>,
        tags: Vec<String>,
        empty: BTreeMap<String, String>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Server {
        host: String,
        workers: u32,
    }

    let config = Config {
        server: Server {
            host: "localhost".into(),
            workers: 4,
        },
        name: "shop".into(),
        debug: false,
        ratio: 0.5,
        port: None,
        tags: vec!["a".into(), "b".into()],
        empty: BTreeMap::new(),
    };
    let ini = to_string(&config).unwrap();
    // the keys come before the sections
    assert_eq!(
        ini,
        "name = shop\ndebug = false\nratio = 0.5\ntags = a\ntags = b\n\n\
         [server]\nhost = localhost\nworkers = 4\n\n[empty]\n"
    );
    assert_eq!(from_str::<Config>(&ini).unwrap(), config);

    assert_eq!(to_string(&BTreeMap::<String, u32>::new()).unwrap(), "");
    assert_eq!(to_string(&None::<BTreeMap<String, u32>>).unwrap(), "");
}

#[test]
fn test_quoting() {
    let values = BTreeMap::from([
        ("a", " leading"),
        ("b", "x ; y"),
        ("c", "x #y"),
        ("d", "\"quoted\""),
        ("e", "#fff"),
        ("f", ";x"),
        ("g", "1;2"),
        ("h", "back\\slash \"q\" "),
        ("i", ""),
    ]);
    let ini = to_string(&values).unwrap();
    assert_eq!(
        ini,
        "a = \" leading\"\nb = \"x ; y\"\nc = \"x #y\"\nd = \"\\\"quoted\\\"\"\ne = #fff\n\
         f =;x\ng = 1;2\nh = \"back\\\\slash \\\"q\\\" \"\ni =\n"
    );
    let read: BTreeMap<String, String> = from_str(&ini).unwrap();
    for (key, value) in &values {
        assert_eq!(read[*key], *value, "{}", key);
    }

    // without quotes values that need them are an error
    let config = SerializerConfig::builder().quotes(Quotes::None).build();
    let err = config
        .to_string(&BTreeMap::from([("a", " x")]))
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);
    assert_eq!(
        config.to_string(&BTreeMap::from([("a", "\"x\"")])).unwrap(),
        "a = \"x\"\n"
    );

    // without inline comments, comment characters need no quotes
    let config = SerializerConfig::builder()
        .inline_comments(InlineComments::None)
        .build();
    assert_eq!(
        config.to_string(&BTreeMap::from([("a", "x ; y")])).unwrap(),
        "a = x ; y\n"
    );
}

#[test]
fn test_multiline() {
    let value = BTreeMap::from([("deps", "pytest\n\nruff")]);
    let ini = to_string(&value).unwrap();
    assert_eq!(ini, "deps = pytest\n\n    ruff\n");
    assert_eq!(
        from_str::<BTreeMap<String, String>>(&ini).unwrap()["deps"],
        "pytest\n\nruff"
    );

    for value in ["\nx", "x\n", "x\n  y", "x\n;y", "a\r\nb"] {
        assert!(
            to_string(&BTreeMap::from([("a", value)])).is_err(),
            "{:?}",
            value
        );
    }
    let config = SerializerConfig::builder()
        .continuation(Continuation::None)
        .build();
    assert!(config.to_string(&BTreeMap::from([("a", "x\ny")])).is_err());

    let config = SerializerConfig::builder()
        .continuation(Continuation::Backslash)
        .build();
    let ini = config.to_string(&BTreeMap::from([("a", "x\\")])).unwrap();
    assert_eq!(ini, "a = \"x\\\\\"\n");
    let de = DeserializerConfig::builder()
        .continuation(Continuation::Backslash)
        .build();
    assert_eq!(
        de.from_str::<BTreeMap<String, String>>(&ini).unwrap()["a"],
        "x\\"
    );
}

#[test]
fn test_unsupported() {
    let err = to_string(&vec![1, 2]).unwrap_err();
    assert_eq!(err.message(), "INI files hold maps (like structs)");

    let nested = BTreeMap::from([("a", BTreeMap::from([("b", BTreeMap::from([("c", 1)]))]))]);
    let err = to_string(&nested).unwrap_err();
    assert_eq!(err.message(), "sections of INI files cannot hold maps");

    let err = to_string(&BTreeMap::from([("a", vec![vec![1]])])).unwrap_err();
    assert_eq!(
        err.message(),
        "INI files cannot hold sequences of maps or sequences"
    );

    for key in ["", " a", "a=b", "[a", ";a", "#a", "a\nb", "a:b"] {
        assert!(to_string(&BTreeMap::from([(key, 1)])).is_err(), "{:?}", key);
    }
    let config = SerializerConfig::builder().colon_delimiter(false).build();
    assert_eq!(
        config.to_string(&BTreeMap::from([("a:b", 1)])).unwrap(),
        "a:b = 1\n"
    );

    let section = |name: &str| BTreeMap::from([(name.to_string(), BTreeMap::from([("a", 1)]))]);
    assert!(to_string(&section("a] ;b")).is_err());
    assert_eq!(to_string(&section("a]b")).unwrap(), "[a]b]\na = 1\n");
}

#[test]
fn test_nulls_in_sequences() {
    let value = BTreeMap::from([("a", vec![Some("x"), None])]);
    let ini = to_string(&value).unwrap();
    assert_eq!(ini, "a = x\na\n");
    let read: BTreeMap<String, Vec<Option<String>>> = from_str(&ini).unwrap();
    assert_eq!(read["a"], [Some("x".to_string()), None]);
}

#[test]
fn test_git() {
    let value = BTreeMap::from([
        (
            "remote",
            BTreeMap::from([(
                "origin \"x\"",
                BTreeMap::from([("url", "git@x:y.git"), ("fetch", "+refs/*:refs/*")]),
            )]),
        ),
        (
            "alias",
            BTreeMap::from([("lg", BTreeMap::from([("x", "y")]))]),
        ),
    ]);
    let config = SerializerConfig::git();
    let ini = config.to_string(&value).unwrap();
    assert_eq!(
        ini,
        "[alias \"lg\"]\n\tx = y\n\n[remote \"origin \\\"x\\\"\"]\n\tfetch = +refs/*:refs/*\n\turl = git@x:y.git\n"
    );
    let read: BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>> =
        DeserializerConfig::git().from_str(&ini).unwrap();
    assert_eq!(read["remote"]["origin \"x\""]["url"], "git@x:y.git");

    let values = BTreeMap::from([(
        "alias",
        BTreeMap::from([
            ("a", " x"),
            ("b", "x;y"),
            ("c", "tab\there\nnew \"q\" \\"),
            ("d", ""),
        ]),
    )]);
    let ini = config.to_string(&values).unwrap();
    assert_eq!(
        ini,
        "[alias]\n\ta = \" x\"\n\tb = \"x;y\"\n\tc = tab\\there\\nnew \\\"q\\\" \\\\\n\td =\n"
    );
    let read: BTreeMap<String, BTreeMap<String, String>> =
        DeserializerConfig::git().from_str(&ini).unwrap();
    assert_eq!(
        read,
        values
            .iter()
            .map(|(k, v)| (
                k.to_string(),
                v.iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            ))
            .collect()
    );

    for key in ["1a", "a_b", "a.b"] {
        let value = BTreeMap::from([("s", BTreeMap::from([(key, 1)]))]);
        assert!(config.to_string(&value).is_err(), "{}", key);
    }
    let value = BTreeMap::from([("s.x", BTreeMap::from([("a", 1)]))]);
    assert!(config.to_string(&value).is_err());
}

#[test]
fn test_serializer() {
    let mut serializer = Serializer::with_config(SerializerConfig::python());
    serializer.serialize(&BTreeMap::from([("a", 1)])).unwrap();
    assert_eq!(serializer.as_str(), "a = 1\n");
    assert!(serializer.serialize(&BTreeMap::from([("b", 2)])).is_err());
    assert_eq!(serializer.finish(), "a = 1\n");
}
