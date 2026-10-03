# pickle

```
cargo run -p pickle
```

The example runs Python (`python3`, or what `PYTHON` names) with
[`python/exchange.py`](python/exchange.py), which writes pickles of the
classes in [`python/todo.py`](python/todo.py) and shows the pickles the
example hands back.

## Why

Python programs keep data in pickles: caches, job queues and the
metadata of models.  Pickles are programs that Python runs to build the
value, they import classes and call them, which makes unpickling untrusted
data dangerous in Python and the values graphs rather than trees: values
can be shared and contain themselves.

## What it shows

- Tasks written by Python (dataclasses with an enum member and a list of
  tags that both tasks share) read into a `Vec<Task>`.  Nothing is imported or called:
  an instance is its attributes and an enum member its value.  The shared
  list is in both tasks.
- `Object<T>` keeps the class of an object (`todo.Task`) and the form it
  is created in (from its state).
- A tree of categories whose children refer to their parent.  Where the
  parent is reached again from within itself it's a reference, which is
  `None` to an `Option`.
- The same tree as dynamic value, which keeps classes, ids and references:
  as JSON the reference is `null`, written back it's a pickle with the
  cycle and Python reads children whose parent is the root again.
- A new task written for Python with `Object` and `Global`: Python reads it
  as a `todo.Task` with a `todo.Status` member.
