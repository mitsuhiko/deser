"""Captures the INI inputs of CPython's configparser test suite.

Runs Lib/test/test_configparser.py of a CPython source tree (which has to
match the version of the running interpreter) with configparser's reader
patched to record every input it is given together with the options of the
parser that reads it.  Every distinct (input, options) pair is then parsed
again with a fresh parser and the result is stored next to the input:

    NNNN.ini   the input, exactly as the test handed it to the parser
    NNNN.json  {"tests": [...], "parser": "...", "options": {...},
                "result": {"sections": [[name, [[key, value], ...]], ...]}}
               or {"error": {"type": ..., "errors": [[lineno, line], ...]}}

Inputs larger than 64 KiB (generated stress tests) are left out.  To keep
the results independent of configparser's key folding, the fresh
parser preserves the case of keys (optionxform is the identity).  Sections
come in file order, the default section is reported under its name if it
has keys, the unnamed section (allow_unnamed_section) is reported as null.
A value of null is a key without a delimiter (allow_no_value).

Usage: python capture-configparser.py <cpython-src> <out-dir>
"""

import configparser
import inspect
import json
import os
import sys
import tempfile
import unittest

SRC, OUT = sys.argv[1], sys.argv[2]

# The constructor arguments that change how the input is parsed.
OPTIONS = (
    "defaults",
    "allow_no_value",
    "delimiters",
    "comment_prefixes",
    "inline_comment_prefixes",
    "strict",
    "empty_lines_in_values",
    "default_section",
    "allow_unnamed_section",
)

PARSER_CLASSES = {
    cls.__name__: cls
    for cls in (configparser.RawConfigParser, configparser.ConfigParser)
}

MAX_SIZE = 64 * 1024

captured = {}
current_test = [None]


original_init = configparser.RawConfigParser.__init__
init_signature = inspect.signature(original_init)


def patched_init(self, *args, **kwargs):
    bound = init_signature.bind(self, *args, **kwargs)
    bound.apply_defaults()
    options = {}
    for name in OPTIONS:
        value = bound.arguments[name]
        if isinstance(value, (set, frozenset)):
            value = sorted(value)
        elif isinstance(value, (tuple, list)):
            value = list(value)
        elif name == "defaults" and value is not None:
            value = {str(k): str(v) for k, v in value.items()}
        options[name] = value
    self._captured_options = options
    original_init(self, *args, **kwargs)


configparser.RawConfigParser.__init__ = patched_init


def is_stock(parser):
    """Only stock parsers can be replayed, subclasses can change anything."""
    return (
        type(parser) in PARSER_CLASSES.values()
        and "SECTCRE" not in vars(parser)
    )


original_read = configparser.RawConfigParser._read


def patched_read(self, fp, fpname):
    lines = list(fp)
    if is_stock(self) and current_test[0] is not None:
        text = "".join(lines)
        options = self._captured_options
        key = (type(self).__name__, text, json.dumps(options, sort_keys=True))
        captured.setdefault(key, []).append(current_test[0])
    return original_read(self, iter(lines), fpname)


configparser.RawConfigParser._read = patched_read


class Recorder(unittest.TextTestResult):
    def startTest(self, test):
        current_test[0] = test.id().removeprefix("test.test_configparser.")
        super().startTest(test)

    def stopTest(self, test):
        current_test[0] = None
        super().stopTest(test)


def replay(cls_name, text, options):
    parser = PARSER_CLASSES[cls_name](interpolation=None, **options)
    parser.optionxform = str
    try:
        parser.read_string(text)
    except configparser.Error as exc:
        error = {"type": type(exc).__name__}
        errors = getattr(exc, "errors", None)
        if errors:
            error["errors"] = [[lineno, line] for lineno, line in errors]
        for attr in ("lineno", "section", "option"):
            value = getattr(exc, attr, None)
            if value is not None:
                error[attr] = section_name(value) if attr == "section" else value
        return {"error": error}
    sections = []
    if parser._defaults:
        sections.append([parser.default_section, list(parser._defaults.items())])
    for name, values in parser._sections.items():
        sections.append([section_name(name), list(values.items())])
    return {"result": {"sections": sections}}


def section_name(name):
    if name is getattr(configparser, "UNNAMED_SECTION", object()):
        return None
    return name


def to_json(doc):
    """JSON with one line per test, option, section and error."""

    def dump(value):
        return json.dumps(value, ensure_ascii=False)

    def block(items, indent):
        pad = "  " * indent
        return ",\n".join(pad + item for item in items)

    parts = [
        '  "tests": [\n%s\n  ]' % block(map(dump, doc["tests"]), 2),
        '  "parser": %s' % dump(doc["parser"]),
        '  "options": {\n%s\n  }'
        % block((f"{dump(k)}: {dump(v)}" for k, v in doc["options"].items()), 2),
    ]
    if "result" in doc:
        sections = block(map(dump, doc["result"]["sections"]), 3)
        sections = "[\n%s\n    ]" % sections if sections else "[]"
        parts.append('  "result": {\n    "sections": %s\n  }' % sections)
    else:
        parts.append('  "error": %s' % dump(doc["error"]))
    return "{\n%s\n}\n" % ",\n".join(parts)


def main():
    with tempfile.TemporaryDirectory() as tmp:
        os.symlink(os.path.join(SRC, "Lib", "test"), os.path.join(tmp, "test"))
        sys.path.insert(0, tmp)
        suite = unittest.defaultTestLoader.loadTestsFromName("test.test_configparser")
        runner = unittest.TextTestRunner(
            resultclass=Recorder, verbosity=0, stream=open(os.devnull, "w")
        )
        result = runner.run(suite)
        if not result.wasSuccessful():
            sys.exit(f"test_configparser failed: {result}")

    os.makedirs(OUT, exist_ok=True)
    # Generated stress tests are left out.
    items = sorted(
        (item for item in captured.items() if len(item[0][1].encode()) <= MAX_SIZE),
        key=lambda item: (item[1][0], item[0]),
    )
    for index, ((cls_name, text, options), tests) in enumerate(items, 1):
        options = json.loads(options)
        doc = {
            "tests": sorted(set(tests)),
            "parser": cls_name,
            "options": options,
        }
        doc.update(replay(cls_name, text, options))
        base = os.path.join(OUT, f"{index:04}")
        with open(base + ".ini", "w", encoding="utf-8", newline="") as f:
            f.write(text)
        with open(base + ".json", "w", encoding="utf-8") as f:
            f.write(to_json(doc))
    print(
        f"captured {len(items)} inputs from {result.testsRun} tests"
        f" ({len(captured) - len(items)} larger than {MAX_SIZE} left out)"
    )


main()
