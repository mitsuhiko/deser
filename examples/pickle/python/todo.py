"""The data of the Python side of the example."""
import dataclasses
import enum


class Status(enum.Enum):
    OPEN = "open"
    DONE = "done"


@dataclasses.dataclass
class Task:
    id: int
    title: str
    status: Status
    tags: list


class Category:
    def __init__(self, name, parent=None):
        self.name = name
        self.parent = parent
        self.children = []
        if parent is not None:
            parent.children.append(self)
