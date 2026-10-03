"""The Python side of the example: writes pickles and reads them.

    python exchange.py tasks       writes a pickle of tasks to stdout
    python exchange.py categories  writes a pickle of a tree to stdout
    python exchange.py show        reads a pickle from stdin and shows it
"""
import pickle
import sys

import todo


def tasks():
    # both tasks share the list of tags
    tags = ["docs", "rust"]
    return [
        todo.Task(1, "Write the docs", todo.Status.OPEN, tags),
        todo.Task(2, "Release 1.0", todo.Status.DONE, tags),
    ]


def categories():
    # the children refer to their parent
    root = todo.Category("Work")
    todo.Category("Docs", root)
    todo.Category("Releases", root)
    return root


def show(value):
    if isinstance(value, todo.Category):
        print("python: category %r" % value.name)
        for child in value.children:
            print(
                "python:   child %r, parent is the root: %s"
                % (child.name, child.parent is value)
            )
    else:
        print("python: %r" % (value,))


def main():
    command = sys.argv[1]
    if command == "show":
        show(pickle.loads(sys.stdin.buffer.read()))
    else:
        value = {"tasks": tasks, "categories": categories}[command]()
        sys.stdout.buffer.write(pickle.dumps(value))


if __name__ == "__main__":
    main()
