use deser::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Node {
    name: String,
    child: Option<Box<Node>>,
}

fn make_nested(depth: usize) -> Node {
    let mut node = Node {
        name: "leaf".into(),
        child: None,
    };
    for idx in 0..depth {
        node = Node {
            name: format!("node-{}", idx),
            child: Some(Box::new(node)),
        };
    }
    node
}

fn drop_nested(mut node: Node) {
    // avoid a recursive drop
    while let Some(child) = node.child.take() {
        node = *child;
    }
}

#[test]
fn test_deep_nesting_roundtrip() {
    // deeper than the preallocated stacks of the drivers
    let depth = if cfg!(miri) { 300 } else { 5000 };
    let node = make_nested(depth);
    let json = deser_json::to_string(&node).unwrap();
    assert!(json.starts_with(r#"{"name":"node-"#));
    assert_eq!(json.matches("\"child\"").count(), depth + 1);

    let rv: Node = deser_json::from_str(&json).unwrap();
    assert_eq!(deser_json::to_string(&rv).unwrap(), json);

    drop_nested(rv);
    drop_nested(node);
}

#[test]
fn test_deep_ignored_nesting() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Simple {
        a: u32,
    }

    let depth = if cfg!(miri) { 300 } else { 5000 };
    let json = format!(
        r#"{{"ignored": {}null{}, "a": 42}}"#,
        "[{\"x\": ".repeat(depth),
        "}]".repeat(depth)
    );
    let rv: Simple = deser_json::from_str(&json).unwrap();
    assert_eq!(rv, Simple { a: 42 });
}
