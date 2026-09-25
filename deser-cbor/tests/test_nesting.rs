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
    let bytes = deser_cbor::to_vec(&node).unwrap();
    // {"name": "node-...", "child": ...}
    assert_eq!(&bytes[..6], b"\xa2\x64name");

    let rv: Node = deser_cbor::from_slice(&bytes).unwrap();
    assert_eq!(deser_cbor::to_vec(&rv).unwrap(), bytes);
    assert_eq!(
        deser_cbor::SerializerConfig::new()
            .canonical(true)
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

    let depth = if cfg!(miri) { 300 } else { 5000 };
    // {"ignored": [{"x": [{"x": ... null ...}]}], "a": 42} with a mix of
    // definite and indefinite containers
    let mut bytes = b"\xa2\x67ignored".to_vec();
    for idx in 0..depth {
        if idx % 2 == 0 {
            bytes.extend_from_slice(b"\x81\xa1\x61x");
        } else {
            bytes.extend_from_slice(b"\x9f\xbf\x61x");
        }
    }
    bytes.push(0xf6);
    for idx in (0..depth).rev() {
        if idx % 2 != 0 {
            bytes.extend_from_slice(b"\xff\xff");
        }
    }
    bytes.extend_from_slice(b"\x61a\x18\x2a");
    let rv: Simple = deser_cbor::from_slice(&bytes).unwrap();
    assert_eq!(rv, Simple { a: 42 });
}
