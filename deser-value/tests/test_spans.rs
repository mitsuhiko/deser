use deser::Deserialize;
use deser_location::Spanned;
use deser_path::{Path, PathLayer};
use deser_value::{Deserializer, Value, from_value};

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Config {
    name: String,
    servers: Vec<Server>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Server {
    host: String,
    port: u16,
}

fn json(input: &str) -> Value {
    deser_json::DeserializerConfig::new()
        .track_locations(true)
        .from_str(input)
        .unwrap()
}

fn toml(input: &str) -> Value {
    deser_toml::DeserializerConfig::new()
        .track_locations(true)
        .from_str(input)
        .unwrap()
}

fn yaml(input: &str) -> Value {
    deser_yaml::DeserializerConfig::new()
        .track_locations(true)
        .from_str(input)
        .unwrap()
}

fn location(err: &deser::Error) -> (Option<usize>, Option<usize>) {
    (err.line(), err.column())
}

#[test]
fn test_spans() {
    let input = "{\n  \"name\": \"app\",\n  \"servers\": [{\"host\": \"a\", \"port\": 80}]\n}";
    let value = json(input);
    let span = value["name"].span().unwrap();
    assert_eq!(span.text(), Some("\"app\""));
    assert_eq!(span.line_column(), (2, 11));
    let span = value["servers"][0].span().unwrap();
    assert_eq!(span.text(), Some("{\"host\": \"a\", \"port\": 80}"));
    assert_eq!(value.span().unwrap().text(), Some(input));

    // without location tracking there is no meta data
    let value: Value = deser_json::from_str(input).unwrap();
    assert!(value["name"].meta().is_none());
}

#[test]
fn test_error_locations() {
    let cases = [
        // wrong type
        (
            "json",
            "{\n  \"name\": \"app\",\n  \"servers\": [{\"host\": \"a\", \"port\": \"80\"}]\n}",
        ),
        // missing field
        (
            "json",
            "{\n  \"name\": \"app\",\n  \"servers\": [{\"host\": \"a\"}\n  ]\n}",
        ),
        (
            "toml",
            "name = \"app\"\n\n[[servers]]\nhost = \"a\"\nport = 100000\n",
        ),
        ("toml", "name = \"app\"\n\n[[servers]]\nhost = \"a\"\n"),
        ("yaml", "name: app\nservers:\n  - host: a\n    port: [1]\n"),
        ("yaml", "name: app\nservers:\n  - host: a\n"),
    ];
    for (format, input) in cases {
        let (direct, value) = match format {
            "json" => (
                deser_json::from_str::<Config>(input).unwrap_err(),
                json(input),
            ),
            "toml" => (
                deser_toml::from_str::<Config>(input).unwrap_err(),
                toml(input),
            ),
            "yaml" => (
                deser_yaml::from_str::<Config>(input).unwrap_err(),
                yaml(input),
            ),
            _ => unreachable!(),
        };
        let err = from_value::<Config>(&value).unwrap_err();
        assert!(direct.line().is_some(), "{}", direct);
        assert_eq!(
            location(&err),
            location(&direct),
            "{format}: {err} vs {direct}"
        );
        assert_eq!(err.to_string(), direct.to_string());
    }
}

#[test]
fn test_merged_sources() {
    // values from different sources report errors in their source
    let mut value = toml("name = \"app\"\n");
    let servers = json("[\n  {\"host\": \"a\", \"port\": true}\n]");
    value["servers"] = servers;
    let err = from_value::<Config>(&value).unwrap_err();
    assert_eq!(location(&err), (Some(2), Some(25)));
}

#[test]
fn test_path_and_spanned() {
    let value = yaml("name: app\nservers:\n  - host: a\n    port: x\n");
    let err = Deserializer::new(&value)
        .deserialize_with::<Config, _>(|driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    assert_eq!(
        err.attachment::<Path>().unwrap().to_string(),
        "servers[0].port"
    );
    assert_eq!(location(&err), (Some(4), Some(11)));

    #[derive(Deserialize)]
    struct WithSpans {
        name: Spanned<String>,
    }
    let spanned: WithSpans = from_value(&value).unwrap();
    let span = spanned.name.span.unwrap();
    assert_eq!((span.start.line, span.start.column), (1, 7));
}

#[test]
fn test_duplicate_key_location() {
    let input = "{\"a\": 1,\n \"a\": 2}";
    let err = deser_json::DeserializerConfig::new()
        .track_locations(true)
        .from_str::<Value>(input)
        .unwrap_err();
    assert_eq!(location(&err), (Some(2), Some(2)));
}
