#!/usr/bin/env python3
# Checks that CPython reads what deser-pickle writes as the same value.
#
# The input is written by the `test_corpus_dump_serialized` test of
# deser-pickle (run it with `DESER_PICKLE_DUMP=<file>`).  It has a line for
# every case of the test data and protocol with the input and what the
# serializer wrote for the value deserialized from it.  Both are read with
# the stand-ins of generate.py and have to result in the same value (ids
# are renumbered, the items of sets sorted).
#
# The values of cases.py are also read with Python's unpickler (which
# imports the classes) and compared.
#
# Usage: python check-serializer.py <dump>
import json
import pickle
import sys

import generate


# values of cases.py that are known to read back differently and why
KNOWN = {
    # emitted as their state, the arguments of `__getnewargs__` are lost
    "object-newargs": "arguments and state",
    "object-newargs-ex": "arguments and state",
    # `cls()` without arguments is written as `cls.__new__(cls)`
    "object-reduce-none": "call without arguments",
    # protocols 0 and 1 create tuple subclasses with `tuple.__new__(cls,
    # items)`, written back as `cls(items)`
    "namedtuple": "tuple subclass of protocols 0 and 1",
}


def same(a, b, seen):
    """Compares Python values, including objects without `__eq__`."""
    if (id(a), id(b)) in seen:
        return True
    seen.add((id(a), id(b)))
    if type(a) is not type(b):
        return False
    if isinstance(a, float):
        return a == b or (a != a and b != b)
    if isinstance(a, (list, tuple)):
        return len(a) == len(b) and all(same(x, y, seen) for x, y in zip(a, b))
    if isinstance(a, dict):
        return len(a) == len(b) and all(
            same(ka, kb, seen) and same(va, vb, seen)
            for (ka, va), (kb, vb) in zip(a.items(), b.items())
        )
    if isinstance(a, (set, frozenset)):
        return len(a) == len(b)
    if type(a).__eq__ is not object.__eq__ and not hasattr(a, "__dict__"):
        return a == b
    state = lambda x: (getattr(x, "__dict__", None), [getattr(x, k, None) for k in getattr(type(x), "__slots__", ())])
    return same(state(a), state(b), seen) and (type(a).__eq__ is object.__eq__ or a == b)


def normalize(tagged):
    ids = {}

    def walk(value):
        kind = value[0]
        if kind == "ref":
            return ["ref", ids.setdefault(value[1], len(ids))]
        if kind == "shared":
            return ["shared", ids.setdefault(value[1], len(ids)), walk(value[2])]
        if kind == "object":
            inner = value[3]
            # objects that are empty sequences are written as empty maps
            if inner[0] == "list" and not inner[1]:
                inner = ["dict", []]
            return ["object", value[1], value[2], walk(inner)]
        if kind in ("list", "tuple"):
            return [kind, [walk(x) for x in value[1]]]
        if kind in ("set", "frozenset"):
            items = {json.dumps(walk(x)): walk(x) for x in value[1]}
            return [kind, [items[k] for k in sorted(items)]]
        if kind == "dict":
            return [kind, [[walk(k), walk(v)] for k, v in value[1]]]
        # the strings of Python 2 are bytes in the dump and written back as
        # text if they are UTF-8
        if kind == "bytes":
            try:
                return ["str", bytes.fromhex(value[1]).decode("utf-8")]
            except UnicodeDecodeError:
                return value
        return value

    return walk(tagged)


def main():
    failures = 0
    checked = 0
    with open(sys.argv[1], encoding="utf-8") as f:
        for line in f:
            case = json.loads(line)
            if case["name"].startswith("deser:value:"):
                if case["name"].split(":")[2] in KNOWN:
                    continue
                original = pickle.loads(bytes.fromhex(case["input"]))
                try:
                    written = pickle.loads(bytes.fromhex(case["output"]))
                except Exception as e:  # noqa: BLE001
                    failures += 1
                    print("%s (%d): %s: %s" % (case["name"], case["protocol"], type(e).__name__, e))
                    continue
                if not same(original, written, set()):
                    failures += 1
                    print("%s (%d): different object" % (case["name"], case["protocol"]))
                    print("  expected: %r" % (original,))
                    print("  got:      %r" % (written,))
            expected = generate.run_case(bytes.fromhex(case["input"]))
            got = generate.run_case(bytes.fromhex(case["output"]))
            checked += 1
            if "value" not in expected:
                continue
            if "value" not in got:
                failures += 1
                print("%s (%d): %s" % (case["name"], case["protocol"], got.get("error")))
                continue
            a = normalize(expected["value"])
            b = normalize(got["value"])
            if json.dumps(a) != json.dumps(b):
                failures += 1
                print("%s (%d): different value" % (case["name"], case["protocol"]))
                print("  expected: %s" % json.dumps(a)[:300])
                print("  got:      %s" % json.dumps(b)[:300])
    print("%d checked, %d failures" % (checked, failures))
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    sys.setrecursionlimit(100000)
    main()
