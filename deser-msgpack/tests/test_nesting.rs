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
    let depth = if cfg!(miri) { 150 } else { 5000 };
    let node = make_nested(depth);
    let bytes = deser_msgpack::to_vec(&node).unwrap();
    // {"name": "node-...", "child": ...}
    assert_eq!(&bytes[..6], b"\x82\xa4name");

    let rv: Node = deser_msgpack::from_slice(&bytes).unwrap();
    assert_eq!(deser_msgpack::to_vec(&rv).unwrap(), bytes);
    assert_eq!(
        deser_msgpack::SerializerConfig::builder()
            .canonical(true)
            .build()
            .to_vec(&rv)
            .unwrap()
            .len(),
        bytes.len()
    );

    drop_nested(rv);
    drop_nested(node);
}

#[test]
fn test_deep_ignored_nesting() {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Simple {
        a: u32,
    }

    let depth = if cfg!(miri) { 150 } else { 5000 };
    // {"ignored": [{"x": [{"x": ... null ...}]}], "a": 42} with a mix of
    // short and long headers
    let mut bytes = b"\x82\xa7ignored".to_vec();
    for idx in 0..depth {
        if idx % 2 == 0 {
            bytes.extend_from_slice(b"\x91\x81\xa1x");
        } else {
            bytes.extend_from_slice(b"\xdc\x00\x01\xdf\x00\x00\x00\x01\xa1x");
        }
    }
    bytes.push(0xc0);
    bytes.extend_from_slice(b"\xa1a\x2a");
    let rv: Simple = deser_msgpack::from_slice(&bytes).unwrap();
    assert_eq!(rv, Simple { a: 42 });
}
