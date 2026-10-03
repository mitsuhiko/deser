#!/usr/bin/env python3
# Generates the pickle test data for deser-pickle.
#
# This runs inside the official Python docker image (see
# scripts/update-pickle-test-data.sh).  It collects inputs and records how
# CPython's unpickler reads them:
#
# * the raw pickles of cases.py (`INPUTS`)
# * the values of cases.py (`VALUES`) pickled with every protocol, also
#   optimized with pickletools and, for a few, cut off after every byte
# * the bytes literals of CPython's pickle tests (Lib/test/pickletester.py,
#   test_pickle.py and test_pickletools.py)
#
# No code of the pickles runs and no module is imported: globals are
# replaced by stand-in classes that record how they are called and what
# is set on their instances.  The names of Python 2 are read as the ones of
# Python 3 like Python does (before protocol 3).  A handful of globals that stand for builtin
# types (`set`, `frozenset`, `bytearray`, `bytes`, `_codecs.encode` and
# `copyreg._reconstructor`) are understood, like deser-pickle does.
#
# The expected result is the value as deser-pickle emits it, written as
# nested JSON arrays (see `tag`):
#
# * atoms: ["none"], ["bool", b], ["int", "123"], ["float", "1.5"],
#   ["str", "text"], ["strsur", "<hex of utf-8 with surrogates>"],
#   ["bytes", "<hex>"], ["bytearray", "<hex>"] and ["global", module, name]
# * containers: ["list", [...]], ["tuple", [...]], ["set", [...]],
#   ["frozenset", [...]] and ["dict", [[key, value], ...]]
# * objects: ["object", module, name, value] where the value is what the
#   object is emitted as (see `object_shape`)
# * values that are reached more than once: ["shared", id, value] at every
#   place, and ["ref", id] where a value contains itself (a cycle)
#
# Python 2 strings are read with `encoding="bytes"` and are bytes here.
#
# Usage: python generate.py <cpython> <output.json>
import _compat_pickle
import ast
import io
import json
import os
import pickle
import pickletools
import sys
import threading
import warnings

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import cases  # noqa: E402

MAX_INPUT = 1 << 20
TEST_FILES = [
    "Lib/test/pickletester.py",
    "Lib/test/test_pickle.py",
    "Lib/test/test_pickletools.py",
]
TRUNCATED = ["create-data", "tree", "recursive-multi"]


# -- stand-ins for globals ---------------------------------------------------


class Rec:
    """What happened to an instance of a stand-in class."""

    __slots__ = ("args", "kwargs", "state", "list_items", "dict_items")

    def __init__(self, args, kwargs):
        self.args = args
        self.kwargs = kwargs
        self.state = None
        self.list_items = []
        self.dict_items = []


class Stub:
    """The base of the stand-ins for globals."""

    __slots__ = ("_rec",)

    def __new__(cls, *args, **kwargs):
        self = object.__new__(cls)
        self._rec = Rec(args, kwargs)
        return self

    def __init__(self, *args, **kwargs):
        pass

    def __setstate__(self, state):
        self._rec.state = state

    def append(self, item):
        self._rec.list_items.append(item)

    def extend(self, items):
        self._rec.list_items.extend(items)

    def add(self, item):
        self._rec.list_items.append(item)

    def __setitem__(self, key, value):
        self._rec.dict_items.append((key, value))


class SetMarker:
    """A set made by the `set` global (its items are not hashed)."""

    __slots__ = ("items",)

    def __init__(self, items):
        self.items = items

    def add(self, item):
        self.items.append(item)


class FrozenSetMarker:
    """A frozenset made by the `frozenset` global."""

    __slots__ = ("items",)

    def __init__(self, items):
        self.items = items


class Known:
    """The base of the globals that are understood."""

    __slots__ = ()


def _set_items(args, kwargs):
    if kwargs or len(args) > 1:
        raise TypeError("unsupported arguments")
    if not args:
        return []
    arg = args[0]
    if isinstance(arg, (SetMarker, FrozenSetMarker)):
        return list(arg.items)
    if type(arg) in (list, tuple, set, frozenset):
        return list(arg)
    raise TypeError("unsupported arguments")


class KnownSet(Known):
    __slots__ = ()

    def __new__(cls, *args, **kwargs):
        return SetMarker(_set_items(args, kwargs))


class KnownFrozenSet(Known):
    __slots__ = ()

    def __new__(cls, *args, **kwargs):
        return FrozenSetMarker(_set_items(args, kwargs))


LATIN1 = ("latin1", "latin-1")


def _bytes_arg(args, kwargs):
    if kwargs:
        raise TypeError("unsupported arguments")
    if not args:
        return b""
    if len(args) == 1 and type(args[0]) in (bytes, bytearray):
        return bytes(args[0])
    if len(args) == 2 and type(args[0]) is str and args[1] in LATIN1:
        return args[0].encode("latin-1")
    raise TypeError("unsupported arguments")


class KnownBytes(Known):
    __slots__ = ()

    def __new__(cls, *args, **kwargs):
        if args and type(args[0]) is str:
            raise TypeError("unsupported arguments")
        return _bytes_arg(args, kwargs)


class KnownByteArray(Known):
    __slots__ = ()

    def __new__(cls, *args, **kwargs):
        return bytearray(_bytes_arg(args, kwargs))


class KnownEncode(Known):
    __slots__ = ()

    def __new__(cls, *args, **kwargs):
        if len(args) != 2 or type(args[0]) is not str:
            raise TypeError("unsupported arguments")
        return _bytes_arg(args, kwargs)


def _is_class(value):
    return isinstance(value, type) and issubclass(value, Stub) and value is not Stub


class KnownReconstructor(Known):
    __slots__ = ()

    def __new__(cls, *args, **kwargs):
        if kwargs or len(args) != 3:
            raise TypeError("unsupported arguments")
        target, base, state = args
        if not _is_class(target) or not (_is_class(base) or _is_known(base)):
            raise TypeError("unsupported arguments")
        base_name = base._pk_name if base._pk_module in ("builtins", "__builtin__") else None
        if base_name == "object":
            return target()
        # `list.__init__` and `dict.__init__` add the state as items
        if base_name == "list" and type(state) is list:
            obj = target()
            obj._rec.list_items.extend(state)
            return obj
        if base_name == "dict" and type(state) is dict:
            obj = target()
            obj._rec.dict_items.extend(state.items())
            return obj
        return target(state)


def _is_known(value):
    return isinstance(value, type) and issubclass(value, Known) and value is not Known


KNOWN = {}
for module in ("builtins", "__builtin__"):
    KNOWN[module, "set"] = KnownSet
    KNOWN[module, "frozenset"] = KnownFrozenSet
    KNOWN[module, "bytearray"] = KnownByteArray
    KNOWN[module, "bytes"] = KnownBytes
KNOWN["_codecs", "encode"] = KnownEncode
KNOWN["copyreg", "_reconstructor"] = KnownReconstructor
KNOWN["copy_reg", "_reconstructor"] = KnownReconstructor

_classes = {}


def find_class(module, name):
    key = (module, name)
    cls = _classes.get(key)
    if cls is None:
        base = KNOWN.get(key, Stub)
        cls = type("Global", (base,), {"__slots__": (), "_pk_module": module, "_pk_name": name})
        _classes[key] = cls
    return cls


# the size of the argument of the opcodes with fixed size arguments
FIXED = {
    b"K": 1, b"q": 1, b"h": 1, b"\x80": 1, b"\x82": 1, b"M": 2, b"\x83": 2,
    b"J": 4, b"r": 4, b"j": 4, b"\x84": 4, b"G": 8, b"\x95": 8,
}
# the opcodes with a line (or two) as argument
LINES = {b"I": 1, b"L": 1, b"F": 1, b"S": 1, b"V": 1, b"P": 1, b"p": 1, b"g": 1, b"c": 2, b"i": 2}
# the opcodes with a counted argument: size of the count and if it's signed
COUNTED = {
    b"U": (1, False), b"\x8c": (1, False), b"C": (1, False), b"\x8a": (1, False),
    b"T": (4, True), b"\x8b": (4, True), b"X": (4, False), b"B": (4, False),
    b"\x8d": (8, False), b"\x8e": (8, False), b"\x96": (8, False),
}


def global_protocols(data):
    """The protocol in effect for every global (`GLOBAL`, `INST` and
    `STACK_GLOBAL`) of a pickle, in their order.

    Python reads the names of Python 2 as the ones of Python 3 before
    protocol 3 (`fix_imports`), which happens in `find_class`.
    """
    protocols = []
    proto = 0
    pos = 0
    while pos < len(data):
        op = data[pos : pos + 1]
        pos += 1
        if op == b".":
            break
        if op == b"\x80" and pos < len(data):
            proto = data[pos]
        if op in (b"c", b"i", b"\x93"):
            protocols.append(proto)
        if op in FIXED:
            pos += FIXED[op]
        elif op in LINES:
            for _ in range(LINES[op]):
                end = data.find(b"\n", pos)
                if end < 0:
                    return protocols
                pos = end + 1
        elif op in COUNTED:
            size, signed = COUNTED[op]
            count = int.from_bytes(data[pos : pos + size], "little", signed=signed)
            pos += size + max(count, 0)
    return protocols


class Unpickler(pickle.Unpickler):
    def __init__(self, file, data, **kwargs):
        super().__init__(file, **kwargs)
        self._protocols = global_protocols(data)

    def find_class(self, module, name):
        proto = self._protocols.pop(0) if self._protocols else 0
        if proto < 3:
            if (module, name) in _compat_pickle.NAME_MAPPING:
                module, name = _compat_pickle.NAME_MAPPING[module, name]
            elif module in _compat_pickle.IMPORT_MAPPING:
                module = _compat_pickle.IMPORT_MAPPING[module]
        return find_class(module, name)


# -- the value as deser-pickle emits it --------------------------------------


def is_global(value):
    return _is_class(value) or _is_known(value)


def has_identity(value):
    """Values that are reported as shared or as a cycle."""
    kind = type(value)
    if kind in (list, dict, set) or isinstance(value, (Stub, SetMarker)):
        return True
    if kind in (tuple, frozenset):
        return len(value) > 0
    if isinstance(value, FrozenSetMarker):
        return len(value.items) > 0
    return False


def is_slots_state(state):
    return (
        type(state) is tuple
        and len(state) == 2
        and all(x is None or type(x) is dict for x in state)
    )


def plain_shape(value):
    """The shape of a value: ("atom", value), ("seq", kind, items) or ("map", pairs)."""
    kind = type(value)
    if kind is list:
        return ("seq", "list", value)
    if kind is tuple:
        return ("seq", "tuple", value)
    if kind is set:
        return ("seq", "set", list(value))
    if kind is frozenset:
        return ("seq", "frozenset", list(value))
    if kind is dict:
        return ("map", list(value.items()))
    if isinstance(value, SetMarker):
        return ("seq", "set", value.items)
    if isinstance(value, FrozenSetMarker):
        return ("seq", "frozenset", value.items)
    return ("atom", value)


def inline_shape(value):
    """The shape of a value that an object is emitted as."""
    if isinstance(value, Stub):
        return ("seq", "tuple", [value])
    return plain_shape(value)


def object_shape(obj):
    """What an object is emitted as.

    Its items if items were added (`APPENDS`, `SETITEMS`, `ADDITEMS`), else
    its state (dicts and the `(dict, slots)` tuples are merged into a map),
    else its arguments: none are an empty map (or the keyword arguments),
    one is the argument, more are a tuple.
    """
    rec = obj._rec
    if rec.dict_items:
        return ("map", rec.dict_items)
    if rec.list_items:
        return ("seq", "list", rec.list_items)
    state = rec.state
    if state is not None:
        if type(state) is dict:
            return ("map", list(state.items()))
        if is_slots_state(state):
            pairs = []
            for part in state:
                if part is not None:
                    pairs.extend(part.items())
            return ("map", pairs)
        return inline_shape(state)
    if not rec.args:
        return ("map", list(rec.kwargs.items()))
    if len(rec.args) == 1:
        return inline_shape(rec.args[0])
    return ("seq", "tuple", list(rec.args))


def shape(value):
    if isinstance(value, Stub):
        return object_shape(value)
    return plain_shape(value)


def children(shape_):
    if shape_[0] == "seq":
        return shape_[2]
    if shape_[0] == "map":
        return [x for pair in shape_[1] for x in pair]
    return []


def tag_atom(value):
    if value is None:
        return ["none"]
    if value is True or value is False:
        return ["bool", value]
    kind = type(value)
    if kind is int:
        return ["int", str(value)]
    if kind is float:
        return ["float", repr(value)]
    if kind is str:
        try:
            value.encode("utf-8")
        except UnicodeEncodeError:
            return ["strsur", value.encode("utf-8", "surrogatepass").hex()]
        return ["str", value]
    if kind is bytes:
        return ["bytes", value.hex()]
    if kind is bytearray:
        return ["bytearray", value.hex()]
    if kind is memoryview:
        return ["bytes", bytes(value).hex()]
    if is_global(value):
        return ["global", value._pk_module, value._pk_name]
    if kind in (tuple, frozenset):
        return [kind.__name__, []]
    if isinstance(value, FrozenSetMarker):
        return ["frozenset", []]
    raise TypeError("unexpected value %r" % (value,))


def tag(root):
    # count how often every value with identity is reached
    counts = {}
    seen = set()
    if has_identity(root):
        counts[id(root)] = 1
    stack = [root]
    while stack:
        value = stack.pop()
        if id(value) in seen:
            continue
        seen.add(id(value))
        for child in children(shape(value)):
            if has_identity(child):
                counts[id(child)] = counts.get(id(child), 0) + 1
                if id(child) not in seen:
                    stack.append(child)

    ids = {}
    open_ = set()
    keep = []

    def ident(value):
        if id(value) not in ids:
            ids[id(value)] = len(ids)
            keep.append(value)
        return ids[id(value)]

    def emit(value):
        identity = has_identity(value)
        if identity and id(value) in open_:
            return ["ref", ident(value)]
        if identity:
            open_.add(id(value))
        shape_ = shape(value)
        if shape_[0] == "seq":
            tagged = [shape_[1], [emit(x) for x in shape_[2]]]
        elif shape_[0] == "map":
            tagged = ["dict", [[emit(k), emit(v)] for k, v in shape_[1]]]
        else:
            tagged = tag_atom(shape_[1])
        if isinstance(value, Stub):
            tagged = ["object", type(value)._pk_module, type(value)._pk_name, tagged]
        if identity:
            open_.discard(id(value))
            if counts.get(id(value), 0) > 1:
                return ["shared", ident(value), tagged]
        return tagged

    return emit(root)


# -- running cases -----------------------------------------------------------


def run_case(data):
    f = io.BytesIO(data)
    try:
        value = Unpickler(f, data, encoding="bytes").load()
    except RecursionError:
        return {"error": "RecursionError"}
    except Exception as e:  # noqa: BLE001
        return {"error": "%s: %s" % (type(e).__name__, e)}
    result = {}
    if f.tell() < len(data):
        result["trailing"] = True
    try:
        result["value"] = tag(value)
        json.dumps(result)
    except RecursionError:
        result.pop("value", None)
        result["value_omitted"] = True
    return result


def harvest(path):
    with open(path, encoding="utf-8") as f:
        tree = ast.parse(f.read())
    for node in ast.walk(tree):
        if isinstance(node, ast.Constant) and isinstance(node.value, bytes):
            if 0 < len(node.value) <= MAX_INPUT:
                yield node.value


def collect(cpython):
    inputs = {}

    def add(data, source):
        sources = inputs.setdefault(data, [])
        if source not in sources:
            sources.append(source)

    for name, data in cases.INPUTS.items():
        add(data, "deser:%s" % name)
    for name, make in cases.VALUES.items():
        for proto in range(pickle.HIGHEST_PROTOCOL + 1):
            try:
                data = pickle.dumps(make(), proto)
            except Exception:  # noqa: BLE001
                continue
            add(data, "deser:value:%s:%d" % (name, proto))
            add(pickletools.optimize(data), "deser:value:%s:%d:optimized" % (name, proto))
            if name in TRUNCATED and proto in (0, 4):
                for end in range(len(data)):
                    add(data[:end], "deser:value:%s:%d:truncated" % (name, proto))
    for rel in TEST_FILES:
        for data in harvest(os.path.join(cpython, rel)):
            add(data, "cpython/%s" % rel)
    return inputs


def main():
    cpython, output = sys.argv[1:]
    # huge memo indexes make the unpickler allocate a lot of memory
    try:
        import resource

        resource.setrlimit(resource.RLIMIT_AS, (4 << 30, 4 << 30))
    except (ImportError, ValueError, OSError):
        pass
    sys.setrecursionlimit(1_000_000)
    warnings.simplefilter("ignore")
    inputs = collect(cpython)
    results = []
    for data, sources in inputs.items():
        case = {"sources": sources, "input": data.hex()}
        case.update(run_case(data))
        results.append(case)
    with open(output, "w", encoding="utf-8") as f:
        f.write("[\n")
        f.write(",\n".join(json.dumps(case, ensure_ascii=False) for case in results))
        f.write("\n]\n")
    print("%d cases" % len(results), file=sys.stderr)


if __name__ == "__main__":
    threading.stack_size(512 * 1024 * 1024)
    thread = threading.Thread(target=main)
    thread.start()
    thread.join()
