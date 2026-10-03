//! This example shows open enums: a trait whose implementations are the
//! variants of its trait objects.  The trait `Step` is defined in the
//! `pipeline` crate, `pipeline-extras` adds more steps without `pipeline`
//! knowing about them.
//!
//! The program registers the steps it accepts and reads a pipeline from
//! JSON with them, runs it and writes it back.  Unknown steps are errors
//! that list the steps that are registered.
use deser::{Context, OpenEnums};
use deser_json::DeserializerConfig;
use pipeline::{Pipeline, Replace, Step, Trim};

const INPUT: &str = r#"{
    "name": "shout",
    "steps": [
        {"type": "trim"},
        {"type": "replace", "from": "world", "to": "deser"},
        {"type": "uppercase"},
        {"type": "repeat", "times": 2, "separator": " "}
    ]
}"#;

fn main() {
    // the steps this program accepts, registered once in the context of
    // the configuration that reads pipelines
    let mut steps = OpenEnums::new();
    pipeline::register(&mut steps).unwrap();
    pipeline_extras::register(&mut steps).unwrap();
    let config = DeserializerConfig::builder()
        .context(Context::with(steps))
        .build();

    let pipeline: Pipeline = config.from_str(INPUT).unwrap();
    println!("parsed: {:?}", pipeline);
    let output = pipeline.run("  hello world!  ");
    println!("output: {}", output);
    assert_eq!(output, "HELLO DESER! HELLO DESER!");

    // the steps are written with their names (the alias `uppercase` is
    // written as `upper`)
    let json = deser_json::to_string(&pipeline).unwrap();
    println!("json:   {}", json);
    assert_eq!(
        json,
        r#"{"name":"shout","steps":[{"type":"trim"},{"type":"replace","from":"world","to":"deser"},{"type":"upper"},{"type":"repeat","times":2,"separator":" "}]}"#
    );

    // without the registry the steps cannot be read (writing them does not
    // need it)
    let err = deser_json::from_str::<Pipeline>(INPUT).unwrap_err();
    println!("error:  {}", err);
    assert!(
        err.to_string()
            .contains("no variants of Step are registered")
    );

    // steps can be created in code as well
    let steps: Vec<Box<dyn Step>> = vec![
        Box::new(Trim),
        Box::new(Replace {
            from: "a".into(),
            to: "b".into(),
        }),
    ];
    println!("steps:  {}", deser_json::to_string(&steps).unwrap());

    // unknown steps are errors
    let err = config
        .from_str::<Pipeline>(r#"{"name": "x", "steps": [{"type": "reverse"}]}"#)
        .unwrap_err();
    println!("error:  {}", err);
    assert!(err.to_string().contains(
        "unknown variant `reverse` of Step, expected one of `repeat`, `replace`, `trim`, `upper`"
    ));
}
