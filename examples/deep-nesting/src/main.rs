//! Deeply nested data does not overflow the stack.
//!
//! Deser does not use the call stack to process nested values: sinks and
//! emitters for nested values are handed back to a driver which keeps them
//! on the heap.  This means that a million levels of nesting deserialize
//! and serialize just fine.
//!
//! For untrusted input it can still make sense to reject such data (for
//! instance because it's processed recursively later).  The `Limits`
//! layer limits the depth and the size of the input.
use deser::de::Limits;
use deser::{Deserialize, Serialize};

/// A recursive type.
#[derive(Default, Serialize, Deserialize)]
pub struct Tree {
    #[deser(default)]
    children: Vec<Tree>,
}

impl Tree {
    fn depth(&self) -> usize {
        let mut depth = 1;
        let mut node = self;
        while let Some(child) = node.children.first() {
            node = child;
            depth += 1;
        }
        depth
    }
}

/// The drop glue that Rust generates is recursive.  Deser does not need
/// the stack but Rust would, so trees are dropped iteratively.
impl Drop for Tree {
    fn drop(&mut self) {
        let mut stack = std::mem::take(&mut self.children);
        while let Some(mut node) = stack.pop() {
            stack.append(&mut node.children);
        }
    }
}

const DEPTH: usize = 1_000_000;

fn main() {
    // {"children":[{"children":[...]}]} a million levels deep
    let json = r#"{"children":["#.repeat(DEPTH) + &"]}".repeat(DEPTH);

    let tree: Tree = deser_json::from_str(&json).unwrap();
    println!("depth: {}", tree.depth());
    assert_eq!(tree.depth(), DEPTH);

    // serializing does not need the stack either, in any format
    let cbor = deser_cbor::to_vec(&tree).unwrap();
    let tree: Tree = deser_cbor::from_slice(&cbor).unwrap();
    assert_eq!(deser_json::to_string(&tree).unwrap(), json);
    println!("JSON: {} bytes, CBOR: {} bytes", json.len(), cbor.len());

    // untrusted input can be limited
    let err = deser_json::Deserializer::from_str(&json)
        .deserialize_with::<Tree, _>(|driver| driver.push_layer(Limits::new().max_depth(64)))
        .err()
        .unwrap();
    println!("with limits: {}", err);
}
