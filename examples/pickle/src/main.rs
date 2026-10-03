//! Python's pickle format with `deser-pickle`, exchanged with Python.
//!
//! Python programs keep data in pickles: caches, job queues, model
//! metadata.  This runs Python (`python/exchange.py` with the classes of
//! `python/todo.py`) to write pickles, reads tasks (dataclasses with an
//! enum) into types, looks at their classes, reads a tree whose children
//! refer to their parent (a cycle), keeps the cycle through a dynamic value
//! and hands pickles back to Python, which reads them as instances of its
//! classes.
//!
//! Python is run as `python3` (or what the `PYTHON` environment variable
//! names).
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use deser::{Deserialize, Serialize};
use deser_pickle::{Global, Object};
use deser_value::Value;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Task {
    id: u64,
    title: String,
    status: Status,
    tags: Vec<String>,
}

/// An enum member is pickled as a call of its class with its value
/// (`Status("open")`), which is the value with the class.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename_all = "lowercase")]
enum Status {
    Open,
    Done,
}

#[derive(Debug, Deserialize)]
struct Category {
    name: String,
    parent: Option<Box<Category>>,
    children: Vec<Category>,
}

/// Runs `python/exchange.py` with a command and the input on stdin and
/// returns what it writes to stdout.
fn python(command: &str, input: &[u8]) -> Vec<u8> {
    let python = std::env::var("PYTHON").unwrap_or_else(|_| "python3".into());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("python/exchange.py");
    let mut child = Command::new(&python)
        .arg(script)
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("cannot run {python} (set PYTHON): {err}"));
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{python} failed");
    output.stdout
}

fn main() {
    // Python writes the tasks.  The objects are never created on this
    // side: they are their attributes, the classes are ignored.
    let pickle = python("tasks", b"");
    let tasks: Vec<Task> = deser_pickle::from_slice(&pickle).unwrap();
    for task in &tasks {
        println!(
            "rust: #{} {:?} {:?} {:?}",
            task.id, task.title, task.status, task.tags
        );
    }
    // the list of tags that both tasks share is in both tasks
    assert_eq!(tasks[0].tags, tasks[1].tags);

    // `Object<T>` keeps the class and how the object is created from it
    let tasks: Vec<Object<Task>> = deser_pickle::from_slice(&pickle).unwrap();
    let class = tasks[0].class.as_ref().unwrap();
    println!("rust: class {} ({:?})", class, tasks[0].form);

    // The children refer to their parent, which contains them: where a
    // value is reached again from within itself it's a reference, which is
    // `None` to an `Option`.
    let pickle = python("categories", b"");
    let root: Category = deser_pickle::from_slice(&pickle).unwrap();
    for child in &root.children {
        println!(
            "rust: {} > {} (parent: {:?})",
            root.name,
            child.name,
            child.parent.as_ref().map(|x| &x.name)
        );
    }
    assert!(root.parent.is_none() && root.children[0].parent.is_none());

    // A dynamic value keeps the classes, ids and references.  JSON has no
    // cycles, the reference is null.  Written back as pickle it has the
    // cycle: Python reads a category whose children have it as parent.
    let value: Value = deser_pickle::from_slice(&pickle).unwrap();
    println!("rust: as JSON {}", deser_json::to_string(&value).unwrap());
    let output = deser_pickle::to_vec(&value).unwrap();
    print!("{}", String::from_utf8(python("show", &output)).unwrap());

    // Writing objects for Python: a `todo.Task` and the status as a call
    // of `todo.Status` with its value.
    #[derive(Serialize)]
    struct NewTask {
        id: u64,
        title: String,
        status: Object<&'static str>,
        tags: Vec<String>,
    }
    let task = Object::new(
        Global::new("todo", "Task"),
        NewTask {
            id: 3,
            title: "Read pickles from Rust".into(),
            status: Object::new(Global::new("todo", "Status"), "open"),
            tags: vec!["rust".into()],
        },
    );
    let output = deser_pickle::to_vec(&task).unwrap();
    print!("{}", String::from_utf8(python("show", &output)).unwrap());
}
