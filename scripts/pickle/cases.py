# Handwritten cases for the pickle test data of deser-pickle.
#
# `VALUES` are pickled with every protocol (and optimized with pickletools),
# `INPUTS` are raw pickles for the corners the pickler never writes.  The
# classes are pickled by reference to this module (`cases`), the generator
# never imports them when it reads the pickles back.
import collections
import dataclasses
import datetime
import decimal
import enum


class Plain:
    def __init__(self, **kwargs):
        self.__dict__.update(kwargs)


class Slots:
    __slots__ = ("x", "y")

    def __init__(self, x, y):
        self.x = x
        self.y = y


class SlotsAndDict:
    __slots__ = ("x", "__dict__")

    def __init__(self, x, **kwargs):
        self.x = x
        self.__dict__.update(kwargs)


class Node:
    def __init__(self, name, parent=None):
        self.name = name
        self.parent = parent
        self.children = []
        if parent is not None:
            parent.children.append(self)


class CustomState:
    def __init__(self, value):
        self.value = value

    def __getstate__(self):
        return [self.value, "extra"]

    def __setstate__(self, state):
        self.value = state[0]


class Reduced:
    def __init__(self, *args):
        self.args = args

    def __reduce__(self):
        return (Reduced, self.args)


class ReducedOne:
    def __init__(self, value):
        self.value = value

    def __reduce__(self):
        return (ReducedOne, (self.value,))


class MyList(list):
    pass


class MyDict(dict):
    pass


class MySet(set):
    pass


class MyTuple(tuple):
    pass


class NewArgs:
    def __new__(cls, a, b):
        self = object.__new__(cls)
        self.a = a
        self.b = b
        return self

    def __getnewargs__(self):
        return (self.a, self.b)


class NewArgsEx:
    def __new__(cls, a, *, b):
        self = object.__new__(cls)
        self.a = a
        self.b = b
        return self

    def __getnewargs_ex__(self):
        return ((self.a,), {"b": self.b})


@dataclasses.dataclass
class Point:
    x: int
    y: int


@dataclasses.dataclass
class Tree:
    value: int
    children: list


class Color(enum.Enum):
    RED = 1
    GREEN = "green"


Pair = collections.namedtuple("Pair", "left right")


def _recursive_list():
    x = []
    x.append(x)
    return x


def _recursive_dict():
    x = {}
    x["self"] = x
    return x


def _recursive_tuple_and_list():
    x = ([],)
    x[0].append(x)
    return x


def _recursive_tuple_and_dict():
    x = ({},)
    x[0][1] = x
    return x


def _recursive_dict_key():
    class_ = Plain()
    x = {}
    x[class_] = x
    class_.d = x
    return x


def _recursive_inst():
    x = Plain()
    x.attr = x
    return x


def _recursive_multi():
    lst = []
    d = {1: lst}
    i = Plain(d=d)
    lst.append(i)
    return lst


def _recursive_set_and_inst():
    s = set()
    i = Plain(s=s)
    s.add(i)
    return s


def _recursive_frozenset_and_inst():
    i = Plain()
    s = frozenset([i])
    i.s = s
    return s


def _recursive_list_subclass():
    x = MyList()
    x.append(x)
    return x


def _recursive_dict_subclass():
    x = MyDict()
    x[1] = x
    return x


def _recursive_inst_state():
    x = CustomState(None)
    x.value = x
    return x


def _tree():
    root = Node("root")
    a = Node("a", root)
    Node("b", root)
    Node("c", a)
    return root


def _shared():
    shared_list = [1, 2]
    shared_dict = {"k": "v"}
    shared_tuple = (1, "x")
    shared_obj = Point(1, 2)
    return {
        "lists": [shared_list, shared_list],
        "dicts": (shared_dict, shared_dict),
        "tuples": [shared_tuple, shared_tuple],
        "objects": [shared_obj, shared_obj, shared_obj],
        "strings": ["same", "same"],
    }


def _create_data():
    # create_data() of CPython's pickletester
    c = Plain()
    c.foo = 1
    c.bar = 2
    x = [0, 1, 2.0, 3.0 + 0j]
    x.extend([1, -1, 0xFF, -0xFF, -0xFF - 1, 0xFFFF, -0xFFFF, -0xFFFF - 1,
              0x7FFFFFFF, -0x7FFFFFFF, -0x7FFFFFFF - 1])
    y = ("abc", "abc", c, c)
    x.append(y)
    x.append(y)
    x.append(5)
    return x


def _slots_and_dict():
    return SlotsAndDict(1, y=2)


VALUES = {
    "none": lambda: None,
    "true": lambda: True,
    "false": lambda: False,
    "int-zero": lambda: 0,
    "int-small": lambda: [1, -1, 255, 256, -256, 65535, 65536, -65536],
    "int-32": lambda: [2**31 - 1, -(2**31), 2**31, -(2**31) - 1],
    "int-64": lambda: [2**63 - 1, -(2**63), 2**63, 2**64 - 1, 2**64, -(2**63) - 1],
    "int-128": lambda: [2**127 - 1, -(2**127), 2**127, 2**128, -(2**127) - 1],
    "int-big": lambda: [2**1000, -(2**1000) + 1, 10**100],
    "int-huge": lambda: 2**8000 + 12345,
    "floats": lambda: [0.0, -0.0, 1.5, -2.25, 1e300, 5e-324, 0.1, float("inf"), float("-inf")],
    "float-nan": lambda: float("nan"),
    "str-empty": lambda: "",
    "str-ascii": lambda: "hello",
    "str-unicode": lambda: "Gr\u00fc\u00dfe \u20ac \U0001f600",
    "str-escapes": lambda: "back\\slash\nnew line\r\t\x00 \u0085 \u2028",
    "str-long": lambda: "x" * 300,
    "bytes-empty": lambda: b"",
    "bytes": lambda: b"abc",
    "bytes-binary": lambda: bytes(range(256)),
    "bytes-long": lambda: b"y" * 300,
    "bytearray": lambda: bytearray(b"ab\xff"),
    "bytearray-empty": lambda: bytearray(),
    "list-empty": lambda: [],
    "list": lambda: [1, "two", 3.0, None, True],
    "list-long": lambda: list(range(1001)),
    "tuple-empty": lambda: (),
    "tuple-1": lambda: (1,),
    "tuple-2": lambda: (1, 2),
    "tuple-3": lambda: (1, 2, 3),
    "tuple-4": lambda: (1, 2, 3, 4),
    "tuple-nested": lambda: ((), ((),), (1, (2, (3,)))),
    "dict-empty": lambda: {},
    "dict": lambda: {"a": 1, "b": [1, 2], "c": {"d": None}},
    "dict-keys": lambda: {1: "int", -5: "neg", "s": "str", b"b": "bytes", (1, "t"): "tuple",
                          None: "none", 1.5: "float", frozenset([1]): "frozenset"},
    "dict-long": lambda: {str(i): i for i in range(1001)},
    "set-empty": lambda: set(),
    "set": lambda: {1, 2, 3},
    "set-strings": lambda: {"a", "b"},
    "set-long": lambda: set(range(1001)),
    "frozenset-empty": lambda: frozenset(),
    "frozenset": lambda: frozenset([1, "a"]),
    "nested": lambda: {"list": [[], [[]], {"x": ({"y": [1]},)}], "set": {(1, 2)}},
    "deep": lambda: [[[[[[[[[[["deep"]]]]]]]]]]],
    "object": lambda: Plain(name="Jane", age=42, tags=["a", "b"]),
    "object-empty": lambda: Plain(),
    "object-slots": lambda: Slots(1, "y"),
    "object-slots-dict": _slots_and_dict,
    "object-custom-state": lambda: CustomState(5),
    "object-reduce": lambda: Reduced(1, "a", None),
    "object-reduce-one": lambda: ReducedOne([1, 2]),
    "object-reduce-none": lambda: Reduced(),
    "object-newargs": lambda: NewArgs(1, 2),
    "object-newargs-ex": lambda: NewArgsEx(1, b=2),
    "dataclass": lambda: Point(1, 2),
    "dataclass-nested": lambda: Tree(1, [Tree(2, []), Tree(3, [Tree(4, [])])]),
    "list-subclass": lambda: MyList([1, 2, 3]),
    "list-subclass-attrs": lambda: _with_attrs(MyList([1]), flag=True),
    "dict-subclass": lambda: MyDict(a=1, b=2),
    "set-subclass": lambda: MySet([1, 2]),
    "tuple-subclass": lambda: MyTuple((1, 2)),
    "namedtuple": lambda: Pair(1, "r"),
    "enum": lambda: [Color.RED, Color.GREEN],
    "ordereddict": lambda: collections.OrderedDict([("b", 1), ("a", 2)]),
    "defaultdict": lambda: collections.defaultdict(list, {"x": [1]}),
    "counter": lambda: collections.Counter("abca"),
    "deque": lambda: collections.deque([1, 2, 3]),
    "complex": lambda: 1 + 2j,
    "decimal": lambda: decimal.Decimal("1.50"),
    "date": lambda: datetime.date(2024, 2, 29),
    "datetime": lambda: datetime.datetime(2024, 2, 29, 12, 30, 15, 123456),
    "datetime-tz": lambda: datetime.datetime(2024, 1, 1, tzinfo=datetime.timezone.utc),
    "timedelta": lambda: datetime.timedelta(days=1, seconds=2),
    "range": lambda: range(1, 10, 2),
    "slice": lambda: slice(1, None, 2),
    "class": lambda: [Point, int, len, collections.OrderedDict],
    "builtin-globals": lambda: [set, frozenset, bytearray, bytes, object],
    "recursive-list": _recursive_list,
    "recursive-dict": _recursive_dict,
    "recursive-tuple-and-list": _recursive_tuple_and_list,
    "recursive-tuple-and-dict": _recursive_tuple_and_dict,
    "recursive-dict-key": _recursive_dict_key,
    "recursive-inst": _recursive_inst,
    "recursive-multi": _recursive_multi,
    "recursive-set-and-inst": _recursive_set_and_inst,
    "recursive-frozenset-and-inst": _recursive_frozenset_and_inst,
    "recursive-list-subclass": _recursive_list_subclass,
    "recursive-dict-subclass": _recursive_dict_subclass,
    "recursive-inst-state": _recursive_inst_state,
    "tree": _tree,
    "shared": _shared,
    "create-data": _create_data,
}


def _with_attrs(obj, **attrs):
    obj.__dict__.update(attrs)
    return obj


INPUTS = {
    # constants and integers in all encodings
    "none": b"N.",
    "true": b"\x88.",
    "false": b"\x89.",
    "int-text-true": b"I01\n.",
    "int-text-false": b"I00\n.",
    "int-text": b"I42\n.",
    "int-text-negative": b"I-42\n.",
    "int-text-zero": b"I0\n.",
    "int-text-minus-zero": b"I-0\n.",
    "int-text-plus-one": b"I+1\n.",
    "int-text-padded": b"I007\n.",
    "int-text-underscore": b"I1_000\n.",
    "int-text-double-underscore": b"I1__000\n.",
    "int-text-trailing-underscore": b"I1_\n.",
    "int-text-space": b"I 5 \n.",
    "int-text-tab": b"I\t5\n.",
    "int-text-big": b"I123456789012345678901234567890\n.",
    "int-text-hex": b"I0x10\n.",
    "int-text-empty": b"I\n.",
    "int-text-sign-only": b"I-\n.",
    "int-text-letters": b"Iabc\n.",
    "int-text-nul": b"I1\x002\n.",
    "int-text-unicode-digit": "I\u0661\n.".encode(),
    "long-text": b"L12345678901234567890L\n.",
    "long-text-no-suffix": b"L42\n.",
    "long-text-negative": b"L-42L\n.",
    "long-text-empty": b"L\n.",
    "long-text-only-suffix": b"LL\n.",
    "long-text-underscore": b"L1_0L\n.",
    "long-text-lowercase-suffix": b"L10l\n.",
    "binint": b"J\xff\xff\xff\xff.",
    "binint1": b"K\xff.",
    "binint2": b"M\xff\xff.",
    "long1-empty": b"\x8a\x00.",
    "long1": b"\x8a\x02\xff\x00.",
    "long1-negative": b"\x8a\x01\xff.",
    "long1-big": b"\x8a\x09\x00\x00\x00\x00\x00\x00\x00\x00\x01.",
    "long4": b"\x8b\x02\x00\x00\x00\x00\x80.",
    "long4-negative-length": b"\x8b\xff\xff\xff\xff.",
    # floats
    "float-text": b"F1.5\n.",
    "float-text-exp": b"F1e10\n.",
    "float-text-inf": b"Finf\n.",
    "float-text-negative-inf": b"F-inf\n.",
    "float-text-infinity": b"FInfinity\n.",
    "float-text-nan": b"Fnan\n.",
    "float-text-space": b"F 1.5 \n.",
    "float-text-underscore": b"F1_0.5\n.",
    "float-text-hex": b"F0x1p3\n.",
    "float-text-empty": b"F\n.",
    "float-text-dot": b"F.\n.",
    "float-text-int": b"F7\n.",
    "float-text-leading-dot": b"F.5\n.",
    "float-text-trailing-dot": b"F5.\n.",
    "binfloat": b"G\x3f\xf8\x00\x00\x00\x00\x00\x00.",
    "binfloat-nan": b"G\x7f\xf8\x00\x00\x00\x00\x00\x01.",
    # strings
    "string-single-quotes": b"S'abc'\n.",
    "string-double-quotes": b'S"abc"\n.',
    "string-escapes": b"S'a\\nb\\tc\\\\d\\'e\\x41\\101\\0'\n.",
    "string-bad-x-escape": b"S'\\x4'\n.",
    "string-unknown-escape": b"S'\\q'\n.",
    "string-line-continuation": b"S'a\\\nb'\n.",
    "string-trailing-backslash": b"S'a\\'\n.",
    "string-unquoted": b"Sabc\n.",
    "string-mismatched-quotes": b"S'abc\"\n.",
    "string-one-quote": b"S'\n.",
    "string-non-utf8": b"S'\\xff'\n.",
    "string-raw-non-utf8": b"S'\xff'\n.",
    "string-utf8": b"S'\xc3\xa9'\n.",
    "binstring": b"T\x03\x00\x00\x00abc.",
    "binstring-negative": b"T\xff\xff\xff\xffabc.",
    "short-binstring": b"U\x03abc.",
    "short-binstring-latin1": b"U\x01\xe9.",
    "unicode-text": b"Vabc\n.",
    "unicode-text-escapes": b"V\\u00e9\\U0001f600\\u000a\\\\x\n.",
    "unicode-text-latin1": b"V\xe9\n.",
    "unicode-text-short-escape": b"V\\u00e\n.",
    "unicode-text-backslash": b"Va\\b\n.",
    "unicode-text-surrogate": b"V\\ud800\n.",
    "unicode-text-surrogate-pair": b"V\\ud83d\\ude00\n.",
    "unicode-text-out-of-range": b"V\\U00110000\n.",
    "binunicode": b"X\x02\x00\x00\x00\xc3\xa9.",
    "binunicode-invalid": b"X\x01\x00\x00\x00\xff.",
    "binunicode-surrogate": b"X\x03\x00\x00\x00\xed\xa0\x80.",
    "short-binunicode": b"\x8c\x03abc.",
    "binunicode8": b"\x8d\x03\x00\x00\x00\x00\x00\x00\x00abc.",
    "binbytes": b"B\x03\x00\x00\x00abc.",
    "short-binbytes": b"C\x03abc.",
    "binbytes8": b"\x8e\x03\x00\x00\x00\x00\x00\x00\x00abc.",
    "bytearray8": b"\x96\x03\x00\x00\x00\x00\x00\x00\x00abc.",
    "binbytes8-huge": b"\x8e\xff\xff\xff\xff\xff\xff\xff\x7fabc.",
    "readonly-buffer": b"C\x03abc\x98.",
    "readonly-buffer-bytearray": b"\x96\x01\x00\x00\x00\x00\x00\x00\x00a\x98.",
    "readonly-buffer-int": b"K\x01\x98.",
    "next-buffer": b"\x97.",
    # containers
    "list-text": b"(I1\nI2\nl.",
    "list-append": b"]K\x01a.",
    "list-appends-empty": b"](e.",
    "tuple-text": b"(I1\nI2\nt.",
    "tuple-empty-mark": b"(t.",
    "tuple1": b"K\x01\x85.",
    "tuple2": b"K\x01K\x02\x86.",
    "tuple3": b"K\x01K\x02K\x03\x87.",
    "dict-text": b"(S'a'\nI1\nd.",
    "dict-odd": b"(S'a'\nd.",
    "dict-setitem": b"}K\x01K\x02s.",
    "dict-setitems-odd": b"}(K\x01u.",
    "dict-setitems-empty": b"}(u.",
    "dict-duplicate-keys": b"}(K\x01K\x02K\x03K\x04K\x01K\x05u.",
    "dict-duplicate-keys-int-bool": b"}(K\x01K\x02\x88K\x03u.",
    "dict-unhashable-key": b"}(]K\x01u.",
    "set-additems": b"\x8f(K\x01K\x02\x90.",
    "set-duplicates": b"\x8f(K\x01K\x01\x90.",
    "set-additems-empty": b"\x8f(\x90.",
    "frozenset-opcode": b"(K\x01K\x02\x91.",
    "frozenset-empty-opcode": b"(\x91.",
    "additems-on-list": b"](K\x01\x90.",
    "additems-on-frozenset": b"(\x91(K\x01\x90.",
    "additems-empty-on-int": b"K\x01(\x90.",
    "appends-empty-on-int": b"K\x01(e.",
    "setitems-empty-on-int": b"K\x01(u.",
    "append-on-dict": b"}K\x01a.",
    "append-on-tuple": b")K\x01a.",
    "setitem-on-list": b"]K\x01K\x02s.",
    "setitem-on-list-in-range": b"]K\x01aK\x00K\x02s.",
    "setitem-on-tuple": b")K\x01K\x02s.",
    # stack and marks
    "pop": b"K\x01K\x020.",
    "pop-mark": b"K\x01(K\x02K\x031.",
    "pop-pops-mark": b"K\x01(0.",
    "dup": b"]2a.",
    "stop-with-mark": b"(.",
    "stop-with-mark-and-value": b"K\x01(K\x02.",
    "stop-with-extra-stack": b"K\x01K\x02.",
    "empty": b"",
    "no-stop": b"N",
    "trailing-data": b"N.N.",
    "trailing-garbage": b"N.\xff",
    "unknown-opcode": b"\xff.",
    "unknown-opcode-z": b"z.",
    # protocol and frames
    "proto-0": b"\x80\x00N.",
    "proto-5": b"\x80\x05N.",
    "proto-6": b"\x80\x06N.",
    "proto-twice": b"\x80\x02\x80\x04N.",
    "proto-late": b"N\x80\x02.",
    "frame": b"\x80\x04\x95\x02\x00\x00\x00\x00\x00\x00\x00N..",
    "frame-empty": b"\x80\x04\x95\x00\x00\x00\x00\x00\x00\x00\x00N.",
    "frame-too-long": b"\x80\x04\x95\x10\x00\x00\x00\x00\x00\x00\x00N.",
    "frame-crossing": b"\x80\x04\x95\x02\x00\x00\x00\x00\x00\x00\x00K\x01K\x02\x86.",
    "frame-nested": b"\x80\x04\x95\x0b\x00\x00\x00\x00\x00\x00\x00\x95\x01\x00\x00\x00\x00\x00\x00\x00N.",
    "frame-huge": b"\x80\x04\x95\xff\xff\xff\xff\xff\xff\xff\xffN.",
    # memo
    "put-get": b"]p0\n0g0\n.",
    "put-get-big": b"]p123456\n0g123456\n.",
    "put-negative": b"]p-1\n.",
    "put-text-garbage": b"]pabc\n.",
    "get-missing": b"g0\n.",
    "binput-binget": b"]q\x000h\x00.",
    "long-binput-binget": b"]r\x00\x00\x01\x000j\x00\x00\x01\x00.",
    "long-binput-negative": b"]r\xff\xff\xff\xff.",
    "long-binput-large": b"]r\x00\x00\x00\x100j\x00\x00\x00\x10.",
    "memoize": b"\x80\x04]\x94K\x01\x94h\x00h\x01\x86.",
    "memoize-after-put": b"]q\x05K\x01\x94h\x01.",
    "memoize-after-overwrite": b"]q\x00K\x01q\x00K\x02\x94h\x01.",
    "memo-overwrite": b"K\x01q\x00K\x02q\x00h\x00.",
    "memo-shared-list-mutated": b"]q\x00(h\x00h\x00l0h\x00K\x01a(h\x00h\x00t.",
    # globals and objects
    "global": b"cmodule\nname\n.",
    "global-dotted": b"cmodule.sub\nOuter.Inner\n.",
    "global-empty": b"c\n\n.",
    "global-truncated": b"cmodule\n.",
    "global-non-utf8": b"cmod\xff\nname\n.",
    "stack-global": b"\x8c\x06module\x8c\x04name\x93.",
    "stack-global-not-str": b"K\x01K\x02\x93.",
    "stack-global-bytes": b"C\x01aC\x01b\x93.",
    "reduce": b"cmod\nCls\nK\x01\x85R.",
    "reduce-no-args": b"cmod\nCls\n)R.",
    "reduce-args-not-tuple": b"cmod\nCls\n]R.",
    "reduce-not-callable": b"K\x01)R.",
    "reduce-object-callable": b"cmod\nCls\n)R)R.",
    "reduce-two-args": b"cmod\nCls\nK\x01K\x02\x86R.",
    "reduce-kwargs-like": b"cmod\nCls\n}\x85R.",
    "newobj": b"cmod\nCls\n)\x81.",
    "newobj-args": b"cmod\nCls\nK\x01\x85\x81.",
    "newobj-args-not-tuple": b"cmod\nCls\n]\x81.",
    "newobj-not-class": b"K\x01)\x81.",
    "newobj-ex": b"cmod\nCls\n)}(\x8c\x01kK\x01u\x92.",
    "newobj-ex-args": b"cmod\nCls\nK\x01\x85}\x92.",
    "newobj-ex-kwargs-not-dict": b"cmod\nCls\n)]\x92.",
    "newobj-ex-args-not-tuple": b"cmod\nCls\n]}\x92.",
    "obj": b"(cmod\nCls\nK\x01K\x02o.",
    "obj-no-args": b"(cmod\nCls\no.",
    "obj-not-class": b"(K\x01o.",
    "inst": b"(K\x01imod\nCls\n.",
    "inst-no-args": b"(imod\nCls\n.",
    "build-dict": b"cmod\nCls\n)\x81}(\x8c\x01aK\x01ub.",
    "build-none": b"cmod\nCls\n)\x81Nb.",
    "build-slots": b"cmod\nCls\n)\x81N}(\x8c\x01aK\x01u\x86b.",
    "build-dict-and-slots": b"cmod\nCls\n)\x81}(\x8c\x01aK\x01u}(\x8c\x01bK\x02u\x86b.",
    "build-list-state": b"cmod\nCls\n)\x81]K\x01ab.",
    "build-int-state": b"cmod\nCls\n)\x81K\x05b.",
    "build-twice": b"cmod\nCls\n)\x81}(\x8c\x01aK\x01ub}(\x8c\x01bK\x02ub.",
    "build-on-list": b"]}b.",
    "build-none-on-list": b"]Nb.",
    "build-on-global": b"cmod\nCls\n}b.",
    "object-appends": b"cmod\nCls\n)\x81(K\x01K\x02e.",
    "object-append": b"cmod\nCls\n)\x81K\x01a.",
    "object-setitems": b"cmod\nCls\n)\x81(K\x01K\x02u.",
    "object-setitem": b"cmod\nCls\n)\x81K\x01K\x02s.",
    "object-additems": b"cmod\nCls\n)\x81(K\x01K\x02\x90.",
    "object-items-and-state": b"cmod\nCls\n)\x81(K\x01K\x02e}(\x8c\x01aK\x01ub.",
    "object-args-object": b"cmod\nCls\ncmod\nOther\n)\x81\x85R.",
    "object-args-global": b"cmod\nCls\ncmod\nOther\n\x85R.",
    "object-shared-args": b"]q\x00cmod\nCls\nh\x00\x85Rh\x00\x86.",
    "object-cycle-via-args-list": b"cmod\nCls\n]q\x00\x85Rq\x01h\x00h\x01a0h\x01.",
    "object-self-state": b"cmod\nCls\n)\x81q\x00]h\x00ab.",
    "persid": b"Pabc\n.",
    "binpersid": b"K\x01Q.",
    "ext1": b"\x82\x01.",
    "ext2": b"\x83\x01\x00.",
    "ext4": b"\x84\x01\x00\x00\x00.",
    "ext1-zero": b"\x82\x00.",
    # the reducers that are understood
    "set-reduce": b"cbuiltins\nset\n]K\x01a\x85R.",
    "set-reduce-py2": b"c__builtin__\nset\n(K\x01K\x02l\x85R.",
    "set-reduce-tuple": b"cbuiltins\nset\nK\x01K\x02\x86\x85R.",
    "set-reduce-empty": b"cbuiltins\nset\n)R.",
    "set-reduce-not-iterable": b"cbuiltins\nset\nK\x01\x85R.",
    "set-reduce-str": b"cbuiltins\nset\n\x8c\x02ab\x85R.",
    "set-reduce-two-args": b"cbuiltins\nset\n]]\x86R.",
    "set-newobj": b"cbuiltins\nset\n]K\x01a\x85\x81.",
    "set-reduce-additems": b"cbuiltins\nset\n)R(K\x01\x90.",
    "set-reduce-build": b"cbuiltins\nset\n)R}b.",
    "frozenset-reduce": b"cbuiltins\nfrozenset\n]K\x01a\x85R.",
    "frozenset-reduce-empty": b"cbuiltins\nfrozenset\n)R.",
    "bytearray-reduce": b"cbuiltins\nbytearray\nC\x02ab\x85R.",
    "bytearray-reduce-latin1": b"c__builtin__\nbytearray\nX\x02\x00\x00\x00\xc3\xa9X\x07\x00\x00\x00latin-1\x86R.",
    "bytearray-reduce-latin1-wide": b"c__builtin__\nbytearray\nX\x03\x00\x00\x00\xe2\x82\xacX\x07\x00\x00\x00latin-1\x86R.",
    "bytearray-reduce-utf8": b"cbuiltins\nbytearray\nX\x02\x00\x00\x00\xc3\xa9X\x05\x00\x00\x00utf-8\x86R.",
    "bytearray-reduce-empty": b"cbuiltins\nbytearray\n)R.",
    "bytearray-reduce-int": b"cbuiltins\nbytearray\nK\x03\x85R.",
    "bytes-reduce-empty": b"c__builtin__\nbytes\n)R.",
    "bytes-reduce": b"cbuiltins\nbytes\nC\x02ab\x85R.",
    "bytes-reduce-list": b"cbuiltins\nbytes\n]K\x01a\x85R.",
    "codecs-encode": b"c_codecs\nencode\nX\x02\x00\x00\x00\xc3\xa9X\x06\x00\x00\x00latin1\x86R.",
    "codecs-encode-latin-1": b"c_codecs\nencode\nX\x01\x00\x00\x00aX\x07\x00\x00\x00latin-1\x86R.",
    "codecs-encode-wide": b"c_codecs\nencode\nX\x03\x00\x00\x00\xe2\x82\xacX\x06\x00\x00\x00latin1\x86R.",
    "codecs-encode-utf8": b"c_codecs\nencode\nX\x01\x00\x00\x00aX\x05\x00\x00\x00utf-8\x86R.",
    "codecs-encode-one-arg": b"c_codecs\nencode\nX\x01\x00\x00\x00a\x85R.",
    "reconstructor": b"ccopy_reg\n_reconstructor\n(cmod\nCls\nc__builtin__\nobject\nNtR}(S'a'\nI1\nub.",
    "reconstructor-list": b"ccopyreg\n_reconstructor\n(cmod\nCls\ncbuiltins\nlist\n(I1\nI2\nltR.",
    "reconstructor-two-args": b"ccopyreg\n_reconstructor\n(cmod\nCls\ncbuiltins\nobject\ntR.",
    "reconstructor-not-class": b"ccopyreg\n_reconstructor\n(I1\ncbuiltins\nobject\nNtR.",
    "reconstructor-base-not-class": b"ccopyreg\n_reconstructor\n(cmod\nCls\nI1\nNtR.",
    "reconstructor-known-class": b"ccopyreg\n_reconstructor\n(cbuiltins\nset\ncbuiltins\nobject\nNtR.",
    "global-as-value-set": b"cbuiltins\nset\n.",
    "global-as-value-reconstructor": b"ccopyreg\n_reconstructor\n.",
}
