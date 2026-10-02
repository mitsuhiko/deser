//! Tests of open enums (`#[deser::open_enum]` and `#[deser::variant]`).
use std::fmt::Debug;
use std::sync::Arc;

use deser::de::{DeserializeDriver, DeserializeOwned};
use deser::ser::{Describe, SerializeDriver, SerializeRef, Variant, VariantKind, VariantRepr};
use deser::{ContainerShape, Context, Deserialize, Error, Event, OpenEnums, Serialize};

/// Returns a context with the variants of the open enums of the tests.
fn context() -> Context {
    let mut variants = OpenEnums::new();
    variants
        .register::<dyn Shape, Circle>()
        .unwrap()
        .register::<dyn Shape, Rect>()
        .unwrap()
        .register::<dyn Shape, Point>()
        .unwrap()
        .register::<dyn Action, SendEmail>()
        .unwrap()
        .register::<dyn Action, Wait>()
        .unwrap()
        .register::<dyn Message, Ping>()
        .unwrap()
        .register::<dyn Message, Text>()
        .unwrap()
        .register::<dyn Plugin, Other>()
        .unwrap();
    Context::with(variants)
}

fn deserialize<T: DeserializeOwned>(events: Vec<Event<'_>>) -> Result<T, Error> {
    deserialize_in(events, &context())
}

fn deserialize_in<T: DeserializeOwned>(
    events: Vec<Event<'_>>,
    context: &Context,
) -> Result<T, Error> {
    let mut out = None;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        driver.set_context(context.clone());
        for event in events {
            driver.emit(event)?;
        }
    }
    Ok(out.unwrap())
}

fn serialize<T: Serialize + ?Sized>(value: &T) -> Vec<Event<'static>> {
    let mut events = Vec::new();
    let mut driver = SerializeDriver::new(&value);
    while let Some((event, _, _)) = driver.next().unwrap() {
        events.push(event.to_static());
    }
    events
}

// externally tagged (the default)

#[deser::open_enum]
trait Shape: Debug + Send + Sync {
    fn area(&self) -> u32;
}

#[derive(Debug, Serialize, Deserialize)]
struct Circle {
    radius: u32,
}

#[deser::variant]
impl Shape for Circle {
    fn area(&self) -> u32 {
        3 * self.radius * self.radius
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Rect {
    width: u32,
    height: u32,
}

#[deser::variant(alias = "rectangle")]
impl Shape for Rect {
    fn area(&self) -> u32 {
        self.width * self.height
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Rectangle;

#[deser::variant(alias = "rectangle")]
impl Shape for Rectangle {
    fn area(&self) -> u32 {
        0
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Point;

#[deser::variant]
impl Shape for Point {
    fn area(&self) -> u32 {
        0
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Scene {
    shapes: Vec<Box<dyn Shape>>,
    #[deser(default)]
    main: Option<Arc<dyn Shape>>,
}

#[test]
fn test_external() {
    let shape: Box<dyn Shape> = Box::new(Circle { radius: 2 });
    assert_eq!(
        serialize(&shape),
        vec![
            Event::MapStart(ContainerShape::with_len(1)),
            "Circle".into(),
            Event::MapStart(ContainerShape::with_len(1)),
            "radius".into(),
            2u64.into(),
            Event::MapEnd,
            Event::MapEnd,
        ]
    );
    let shape: Box<dyn Shape> = deserialize(serialize(&shape)).unwrap();
    assert_eq!(format!("{:?}", shape), "Circle { radius: 2 }");
    assert_eq!(shape.area(), 12);

    // aliases
    let shape: Box<dyn Shape> = deserialize(vec![
        Event::map_start(),
        "rectangle".into(),
        Event::map_start(),
        "width".into(),
        2u64.into(),
        "height".into(),
        3u64.into(),
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(shape.area(), 6);

    // unit structs can be given by their name
    let shape: Box<dyn Shape> = deserialize(vec!["Point".into()]).unwrap();
    assert_eq!(format!("{:?}", shape), "Point");
}

#[test]
fn test_containers() {
    let scene = Scene {
        shapes: vec![
            Box::new(Circle { radius: 1 }),
            Box::new(Rect {
                width: 1,
                height: 2,
            }),
        ],
        main: Some(Arc::new(Point)),
    };
    let scene: Scene = deserialize(serialize(&scene)).unwrap();
    assert_eq!(
        format!("{:?}", scene),
        "Scene { shapes: [Circle { radius: 1 }, Rect { width: 1, height: 2 }], main: Some(Point) }"
    );

    let shape: Arc<dyn Shape> =
        deserialize(serialize(&Circle { radius: 1 } as &dyn Shape)).unwrap();
    assert_eq!(shape.area(), 3);
}

#[test]
fn test_unknown_variant() {
    let err = deserialize::<Box<dyn Shape>>(vec![
        Event::map_start(),
        "Triangle".into(),
        Event::map_start(),
        Event::MapEnd,
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "UnknownVariant: unknown variant `Triangle` of Shape, expected one of `Circle`, `Point`, \
         `Rect`"
    );
}

// internally tagged with names in another style

#[deser::open_enum(
    tag = "type",
    tag_alias = "kind",
    rename_all = "snake_case",
    alias_all = "kebab-case"
)]
trait Action: Debug + Send + Sync {}

#[derive(Debug, Serialize, Deserialize)]
#[deser(deny_unknown_fields)]
struct SendEmail {
    to: String,
}

#[deser::variant]
impl Action for SendEmail {}

#[derive(Debug, Serialize, Deserialize)]
struct Wait {
    seconds: u32,
}

#[deser::variant(rename = "sleep")]
impl Action for Wait {}

#[test]
fn test_internal() {
    let action: Box<dyn Action> = Box::new(SendEmail { to: "x".into() });
    assert_eq!(
        serialize(&action),
        vec![
            Event::map_start(),
            "type".into(),
            "send_email".into(),
            "to".into(),
            "x".into(),
            Event::MapEnd,
        ]
    );
    let action: Box<dyn Action> = deserialize(serialize(&action)).unwrap();
    assert_eq!(format!("{:?}", action), "SendEmail { to: \"x\" }");

    // the tag can be anywhere, by its alias and with names in the style of
    // `alias_all`
    let action: Box<dyn Action> = deserialize(vec![
        Event::map_start(),
        "to".into(),
        "y".into(),
        "kind".into(),
        "send-email".into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(format!("{:?}", action), "SendEmail { to: \"y\" }");

    // renamed variants
    let action: Box<dyn Action> = Box::new(Wait { seconds: 1 });
    assert_eq!(
        serialize(&action),
        vec![
            Event::map_start(),
            "type".into(),
            "sleep".into(),
            "seconds".into(),
            1u64.into(),
            Event::MapEnd,
        ]
    );

    // the variants reject unknown fields
    let err = deserialize::<Box<dyn Action>>(vec![
        Event::map_start(),
        "type".into(),
        "send_email".into(),
        "to".into(),
        "y".into(),
        "cc".into(),
        "z".into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert!(err.to_string().contains("unknown field `cc`"), "{}", err);

    let err = deserialize::<Box<dyn Action>>(vec![
        Event::map_start(),
        "to".into(),
        "y".into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(err.to_string(), "MissingField: missing tag `type`");
}

// adjacently tagged with names that are not strings

#[deser::open_enum(tag = "t", content = "c", deny_unknown_fields)]
trait Message: Debug + Send + Sync {}

#[derive(Debug, Serialize, Deserialize)]
struct Ping;

#[deser::variant(rename = 1)]
impl Message for Ping {}

#[derive(Debug, Serialize, Deserialize)]
struct Text(String);

#[deser::variant(rename = 2, alias = "text")]
impl Message for Text {}

#[test]
fn test_adjacent() {
    let message: Box<dyn Message> = Box::new(Text("hi".into()));
    assert_eq!(
        serialize(&message),
        vec![
            Event::MapStart(ContainerShape::with_len(2)),
            "t".into(),
            2u64.into(),
            "c".into(),
            "hi".into(),
            Event::MapEnd,
        ]
    );
    let message: Box<dyn Message> = deserialize(serialize(&message)).unwrap();
    assert_eq!(format!("{:?}", message), "Text(\"hi\")");

    let message: Box<dyn Message> = deserialize(vec![
        Event::map_start(),
        "c".into(),
        "x".into(),
        "t".into(),
        "text".into(),
        Event::MapEnd,
    ])
    .unwrap();
    assert_eq!(format!("{:?}", message), "Text(\"x\")");

    let err = deserialize::<Box<dyn Message>>(vec![
        Event::map_start(),
        "t".into(),
        1u64.into(),
        "c".into(),
        ().into(),
        "x".into(),
        ().into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert!(err.to_string().contains("unknown field `x`"), "{}", err);

    let err = deserialize::<Box<dyn Message>>(vec![
        Event::map_start(),
        "t".into(),
        3u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "UnknownVariant: unknown variant `3` of Message, expected `1` or `2`"
    );
}

// the same name for two types

#[deser::open_enum(tag = "type")]
trait Plugin: Debug + Send + Sync {}

mod first {
    #[derive(Debug, deser::Serialize, deser::Deserialize)]
    pub struct Thing;

    #[deser::variant]
    impl super::Plugin for Thing {}
}

mod second {
    #[derive(Debug, deser::Serialize, deser::Deserialize)]
    pub struct Thing;

    #[deser::variant]
    impl crate::test_open_enums::Plugin for Thing {}
}

#[derive(Debug, Serialize, Deserialize)]
struct Other;

#[deser::variant]
impl Plugin for Other {}

#[test]
fn test_duplicate_names() {
    let mut variants = OpenEnums::new();
    variants.register::<dyn Plugin, first::Thing>().unwrap();
    // registering a type again does nothing
    variants.register::<dyn Plugin, first::Thing>().unwrap();
    let err = variants
        .register::<dyn Plugin, second::Thing>()
        .err()
        .unwrap();
    assert_eq!(err.open_enum(), "Plugin");
    assert_eq!(err.name(), "Thing");
    assert!(err.types()[0].ends_with("first::Thing"));
    assert!(err.types()[1].ends_with("second::Thing"));
    assert!(
        err.to_string()
            .starts_with("duplicate variant `Thing` of Plugin: `")
    );

    // aliases are names as well
    let mut variants = OpenEnums::new();
    variants.register::<dyn Shape, Rect>().unwrap();
    let err = variants.register::<dyn Shape, Rectangle>().err().unwrap();
    assert_eq!(err.name(), "rectangle");
    assert_eq!(format!("{:?}", variants), r#"{"Shape": ["Rect"]}"#);
}

#[test]
fn test_unregistered() {
    // without a registry
    let err = deserialize_in::<Box<dyn Shape>>(vec!["Point".into()], &Context::new()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "UnsupportedType: no variants of Shape are registered (register them in a \
         deser::OpenEnums that is given in the context)"
    );

    // the variants of other open enums
    let mut variants = OpenEnums::new();
    variants.register::<dyn Plugin, Other>().unwrap();
    let context = Context::with(variants);
    let err = deserialize_in::<Box<dyn Shape>>(vec!["Point".into()], &context).unwrap_err();
    assert!(
        err.to_string()
            .contains("no variants of Shape are registered")
    );

    // only registered variants are deserialized
    let mut variants = OpenEnums::new();
    variants.register::<dyn Shape, Circle>().unwrap();
    let context = Context::with(variants);
    let err = deserialize_in::<Box<dyn Shape>>(vec!["Point".into()], &context).unwrap_err();
    assert_eq!(
        err.to_string(),
        "UnknownVariant: unknown variant `Point` of Shape, expected `Circle`"
    );
    // but they are all serialized (unit structs are content like the ones
    // of newtype variants)
    assert_eq!(
        serialize(&Point as &dyn Shape),
        vec![
            Event::MapStart(ContainerShape::with_len(1)),
            "Point".into(),
            ().into(),
            Event::MapEnd
        ]
    );
}

#[test]
fn test_internal_unit() {
    // unit structs are the tag alone
    let plugin: Box<dyn Plugin> = Box::new(Other);
    assert_eq!(
        serialize(&plugin),
        vec![
            Event::map_start(),
            "type".into(),
            "Other".into(),
            Event::MapEnd,
        ]
    );
    let plugin: Box<dyn Plugin> = deserialize(serialize(&plugin)).unwrap();
    assert_eq!(format!("{:?}", plugin), "Other");

    let err = deserialize::<Box<dyn Plugin>>(vec![
        Event::map_start(),
        "type".into(),
        "Other".into(),
        "x".into(),
        1u64.into(),
        Event::MapEnd,
    ])
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidType: unexpected map, expected Other"
    );
}

#[derive(Default)]
struct Variants(Vec<String>);

impl Describe for Variants {
    fn variant(&mut self, variant: &Variant<'_>) {
        let repr = match variant.repr {
            VariantRepr::External => "external".to_string(),
            VariantRepr::Internal { tag } => format!("internal {}", tag),
            VariantRepr::Adjacent { tag, content } => format!("adjacent {} {}", tag, content),
            _ => unreachable!(),
        };
        assert_eq!(variant.kind, VariantKind::Newtype);
        self.0.push(format!(
            "{}::{} ({})",
            variant.enum_name, variant.name, repr
        ));
    }
}

#[test]
fn test_describe() {
    let mut d = Variants::default();
    let shape: Box<dyn Shape> = Box::new(Circle { radius: 1 });
    SerializeRef::new(&shape).describe(&mut d);
    let action: &dyn Action = &Wait { seconds: 1 };
    SerializeRef::new(&action).describe(&mut d);
    let message: &dyn Message = &Ping;
    SerializeRef::new(&message).describe(&mut d);
    assert_eq!(
        d.0,
        [
            "Shape::Circle (external)",
            "Action::sleep (internal type)",
            "Message::1 (adjacent t c)",
        ]
    );
}
