use std::collections::BTreeMap;

use deser::adapters::Flag;
use deser::de::{Deserializer as _, DuplicateKeys};
use deser::{Context, Deserialize, ErrorKind};
use deser_ini::{
    Continuation, Deserializer, DeserializerConfig, InlineComments, Quotes, Syntax, from_slice,
    from_str,
};
use deser_path::{Path, PathLayer};

type Sections = BTreeMap<String, BTreeMap<String, String>>;
type Flat = BTreeMap<String, String>;

#[test]
fn test_basics() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Config<'a> {
        name: &'a str,
        debug: bool,
        server: Server,
        #[deser(default)]
        empty: BTreeMap<String, String>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Server {
        port: u16,
        ratio: f32,
        timeout: Option<u32>,
    }

    let input = String::from(
        "; a comment\n# another one\nname = shop\ndebug = yes\n\n[server]\nport: 8080\n\
         ratio = 0.5\ntimeout =\n\n[empty]\n",
    );
    let config: Config = from_str(&input).unwrap();
    assert_eq!(
        config,
        Config {
            name: "shop",
            debug: true,
            server: Server {
                port: 8080,
                ratio: 0.5,
                timeout: None,
            },
            empty: BTreeMap::new(),
        }
    );
    // values that are not changed are borrowed
    assert!(
        input
            .as_bytes()
            .as_ptr_range()
            .contains(&config.name.as_ptr())
    );

    let value: BTreeMap<String, String> = from_str("").unwrap();
    assert!(value.is_empty());
}

#[test]
fn test_repeated_keys_and_sections() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Section {
        tag: Vec<String>,
        name: String,
        #[deser(default)]
        missing: Vec<String>,
    }

    let input = "[a]\ntag = x\nname = first\n[b]\nname = b\n[a]\ntag = y\nname = second\n";
    let value: BTreeMap<String, Section> = from_str(input).unwrap();
    assert_eq!(
        value["a"],
        Section {
            tag: vec!["x".into(), "y".into()],
            name: "second".into(),
            missing: vec![],
        }
    );

    let strict = DeserializerConfig::builder()
        .context(Context::with(DuplicateKeys::Error))
        .build();
    let err = strict
        .from_str::<BTreeMap<String, Section>>(input)
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DuplicateKey);

    let err = from_str::<Sections>("a = 1\n[a]\nb = 2").unwrap_err();
    assert_eq!(err.message(), "`a` is a key and a section");
    assert_eq!(err.line(), Some(2));
}

#[test]
fn test_keys_without_value() {
    #[derive(Debug, Deserialize)]
    struct Mysqld {
        #[deser(as = Flag)]
        skip_networking: bool,
        #[deser(as = Flag)]
        local_infile: bool,
        bind: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct MyCnf {
        mysqld: Mysqld,
    }

    let cnf: MyCnf = from_str("[mysqld]\nskip_networking ; comment\nbind\n").unwrap();
    assert!(cnf.mysqld.skip_networking);
    assert!(!cnf.mysqld.local_infile);
    assert_eq!(cnf.mysqld.bind, None);

    let strict = DeserializerConfig::builder().allow_no_value(false).build();
    let err = strict.from_str::<Sections>("[a]\nb\n").unwrap_err();
    assert_eq!(err.message(), "expected `=` or `:` after the key");
    assert_eq!((err.line(), err.column()), (Some(2), Some(1)));
}

#[test]
fn test_inline_comments() {
    let input = "a = 1;2 ; one\nb = x #y\nc = #fff\nd = ; empty\ne =;x\nf = A;B;\ng=x\t;y";
    let value: Flat = from_str(input).unwrap();
    assert_eq!(value["a"], "1;2");
    assert_eq!(value["b"], "x");
    assert_eq!(value["c"], "#fff");
    assert_eq!(value["d"], "");
    assert_eq!(value["e"], ";x");
    assert_eq!(value["f"], "A;B;");
    assert_eq!(value["g"], "x");

    let config = DeserializerConfig::builder()
        .inline_comments(InlineComments::None)
        .build();
    let value: Flat = config.from_str(input).unwrap();
    assert_eq!(value["a"], "1;2 ; one");
    assert_eq!(value["d"], "; empty");

    let config = DeserializerConfig::builder()
        .inline_comments(InlineComments::Anywhere)
        .build();
    let value: Flat = config.from_str(input).unwrap();
    assert_eq!(value["a"], "1");
    assert_eq!(value["c"], "");
    // comments after sections and keys without value
    let value: deser_value::Value = from_str("[a] ; x\nb # y\n").unwrap();
    assert!(value["a"]["b"].is_null());
}

#[test]
fn test_quotes() {
    let input = "a = \" x ; y \" ; comment\nb = \"say \\\"hi\\\"\"\nc = \"a\" \"b\"\n\
                 d = 'single \\'\ne = \"C:\\Windows\"\nf = \"unclosed\ng = \"\"";
    let value: Flat = from_str(input).unwrap();
    assert_eq!(value["a"], " x ; y ");
    assert_eq!(value["b"], "say \"hi\"");
    assert_eq!(value["c"], "\"a\" \"b\"");
    assert_eq!(value["d"], "single \\");
    assert_eq!(value["e"], "C:\\Windows");
    assert_eq!(value["f"], "\"unclosed");
    assert_eq!(value["g"], "");

    let config = DeserializerConfig::builder().quotes(Quotes::None).build();
    let value: Flat = config.from_str(input).unwrap();
    assert_eq!(value["a"], "\" x");
    assert_eq!(value["g"], "\"\"");
}

#[test]
fn test_indented_continuation() {
    let input = "[options]\ninstall_requires =\n    deser\n\n    # a comment\n    requests\n\
                 python_requires = >=3.9\nname = x\n  y ; z\n\n\nnext = 1\n";
    let value: Sections = from_str(input).unwrap();
    assert_eq!(value["options"]["install_requires"], "deser\n\nrequests");
    assert_eq!(value["options"]["python_requires"], ">=3.9");
    assert_eq!(value["options"]["name"], "x\ny");
    assert_eq!(value["options"]["next"], "1");

    // keys that are indented like the key before them are keys
    let value: Sections = from_str("[a]\n\tb = 1\n\tc = 2\n").unwrap();
    assert_eq!(value["a"]["c"], "2");

    // values on continuation lines use the Separated adapter
    #[derive(Deserialize)]
    struct Options {
        #[deser(as = deser::adapters::Separated<'\n'>)]
        install_requires: Vec<String>,
    }
    #[derive(Deserialize)]
    struct Setup {
        options: Options,
    }
    let setup: Setup = from_str("[options]\ninstall_requires =\n  a\n  b\n").unwrap();
    assert_eq!(setup.options.install_requires, ["a", "b"]);

    let err = from_str::<Sections>("[a]\nb\n  c\n").unwrap_err();
    assert_eq!(
        err.message(),
        "a key without value cannot be continued on the next line"
    );
    assert_eq!(err.line(), Some(3));

    let config = DeserializerConfig::builder()
        .continuation(Continuation::None)
        .build();
    let value: Flat = config.from_str("a = 1\n  b = 2").unwrap();
    assert_eq!((&*value["a"], &*value["b"]), ("1", "2"));
}

#[test]
fn test_backslash_continuation() {
    let config = DeserializerConfig::builder()
        .continuation(Continuation::Backslash)
        .build();
    let value: Flat = config
        .from_str("a = x \\\n  y \\\n  z\nb = 1\nc = \\")
        .unwrap();
    assert_eq!(value["a"], "x   y   z");
    assert_eq!(value["b"], "1");
    assert_eq!(value["c"], "");
}

#[test]
fn test_names() {
    let value: Sections = from_str("[ spaced ]\nKey = 1\n[a]b]\nc = 2\n[]\n").unwrap();
    assert_eq!(value[" spaced "]["Key"], "1");
    assert_eq!(value["a]b"]["c"], "2");
    assert!(value[""].is_empty());

    let config = DeserializerConfig::builder().lowercase_names(true).build();
    let value: Sections = config
        .from_str("[Server]\nPort = 1\n[server]\nhost = x")
        .unwrap();
    assert_eq!(value["server"]["port"], "1");
    assert_eq!(value["server"]["host"], "x");

    let config = DeserializerConfig::builder().colon_delimiter(false).build();
    let value: Flat = config.from_str("url: http://x = 1\nhost = a:b").unwrap();
    assert_eq!(value["url: http://x"], "1");
    assert_eq!(value["host"], "a:b");
    let value: Flat = from_str("url: http://x = 1").unwrap();
    assert_eq!(value["url"], "http://x = 1");
}

#[test]
fn test_syntax_errors() {
    for (input, message, line) in [
        ("[a\nb = 1", "missing `]` of the section header", 1),
        ("[a] b = 1", "unexpected text after the section header", 1),
        ("a = 1\n= 2", "missing key before the delimiter", 2),
    ] {
        let err = from_str::<Sections>(input).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Syntax, "{}", input);
        assert_eq!(err.message(), message, "{}", input);
        assert_eq!(err.line(), Some(line), "{}", input);
    }
    let err = from_slice::<Flat>(b"a = \xff").unwrap_err();
    assert_eq!(err.message(), "input is not valid UTF-8");
}

#[test]
fn test_bom_and_line_endings() {
    let value: Sections = from_slice(b"\xef\xbb\xbf[a]\r\nb = 1\r\nc = 2\rd =\r\n  x\r\n").unwrap();
    assert_eq!(value["a"]["b"], "1");
    assert_eq!(value["a"]["c"], "2");
    assert_eq!(value["a"]["d"], "x");
}

#[test]
fn test_error_locations() {
    #[derive(Debug, Deserialize)]
    struct Server {
        #[allow(dead_code)]
        port: u16,
    }

    #[derive(Debug, Deserialize)]
    struct Config {
        #[allow(dead_code)]
        server: Server,
    }

    let err = Deserializer::from_str("[server]\n\nport = http\n")
        .deserialize_with::<Config, _>(|driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    assert_eq!(err.message(), "invalid value \"http\", expected u16");
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "server.port");
    assert_eq!((err.line(), err.column()), (Some(3), Some(8)));

    let err = from_str::<Config>("[server]\nport = 1\nport = x\n").unwrap_err();
    assert_eq!(err.line(), Some(3));
}

#[test]
fn test_flatten_and_enums() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Paginate {
        limit: u32,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    #[deser(tag = "kind", rename_all = "lowercase")]
    enum Backend {
        Postgres {
            port: u16,
            #[deser(flatten)]
            paginate: Paginate,
        },
        Sqlite {
            path: String,
        },
    }

    let value: BTreeMap<String, Backend> = from_str(
        "[db]\nport = 5432\nkind = postgres\nlimit = 10\n[cache]\nkind = sqlite\npath = x.db",
    )
    .unwrap();
    assert_eq!(
        value["db"],
        Backend::Postgres {
            port: 5432,
            paginate: Paginate { limit: 10 }
        }
    );
    assert_eq!(
        value["cache"],
        Backend::Sqlite {
            path: "x.db".into()
        }
    );
}

#[test]
fn test_update() {
    #[derive(Debug, Deserialize)]
    struct Server {
        host: String,
        port: u16,
    }

    #[derive(Debug, Deserialize)]
    struct Config {
        server: Server,
    }

    let mut config = Config {
        server: Server {
            host: "localhost".into(),
            port: 80,
        },
    };
    Deserializer::from_str("[server]\nport = 8080")
        .update(&mut config)
        .unwrap();
    assert_eq!(config.server.host, "localhost");
    assert_eq!(config.server.port, 8080);
}

#[test]
fn test_python() {
    let config = DeserializerConfig::python();
    let value: Sections = config
        .from_str("[tox]\nenvlist = py312 ; py313\nname = \"quoted\"\n[testenv]\ndeps =\n    pytest\n    ruff\n")
        .unwrap();
    assert_eq!(value["tox"]["envlist"], "py312 ; py313");
    assert_eq!(value["tox"]["name"], "\"quoted\"");
    assert_eq!(value["testenv"]["deps"], "pytest\nruff");
}

#[test]
fn test_git() {
    type Git = BTreeMap<String, BTreeMap<String, deser_value::Value>>;

    let config = DeserializerConfig::git();
    assert_eq!(config.syntax(), Syntax::Git);
    let input = "# comment\n[core]\n\tbare = false\n\tFileMode\n[remote \"Origin\"]\n\
                 \turl = git@x:y.git ; comment\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n\
                 [alias]\n\tlg = \"log --graph\" # c\n\tq = a\\\"b\\\\c\\td\n\tcont = a\\\n  b\n\
                 [Branch.Main] remote = origin\n";
    let value: Git = config.from_str(input).unwrap();
    assert_eq!(value["core"]["bare"].as_str(), Some("false"));
    assert!(value["core"]["filemode"].is_null());
    let origin = value["remote"]["Origin"].as_map().unwrap();
    assert_eq!(origin.get("url").unwrap().as_str(), Some("git@x:y.git"));
    assert_eq!(value["alias"]["lg"].as_str(), Some("log --graph"));
    assert_eq!(value["alias"]["q"].as_str(), Some("a\"b\\c\td"));
    assert_eq!(value["alias"]["cont"].as_str(), Some("a  b"));
    let main = value["branch"]["main"].as_map().unwrap();
    assert_eq!(main.get("remote").unwrap().as_str(), Some("origin"));

    for (input, message, line) in [
        (
            "[core]\n\tbare = \"x\n",
            "missing closing quote of the value",
            2,
        ),
        (
            "[core]\n\tbare = \\x\n",
            "invalid escape sequence in the value",
            2,
        ),
        ("[core]\n\t1bare = x\n", "invalid line in config file", 2),
        ("[core\n", "invalid line in config file", 1),
        ("[core]\n\tbare # x\n", "invalid line in config file", 2),
    ] {
        let err = config.from_str::<Git>(input).unwrap_err();
        assert_eq!(err.message(), message, "{:?}", input);
        assert_eq!(err.line(), Some(line), "{:?}", input);
    }
}

#[test]
fn test_value() {
    let value: deser_value::Value = from_str("top = 1\n[a]\nb = 1\nb = 2\nc\n").unwrap();
    assert_eq!(value["top"].as_str(), Some("1"));
    let b = value["a"]["b"].as_seq().unwrap();
    assert!(b.is_repeated());
    assert_eq!(b.len(), 2);
    assert!(value["a"]["c"].is_null());
    assert_eq!(
        deser_json::to_string(&value).unwrap(),
        r#"{"top":"1","a":{"b":["1","2"],"c":null}}"#
    );
}
