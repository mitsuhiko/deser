"""Prints how Python's configparser parses files, for scripts/ini/references.

For every file named on stdin (one per line, relative to the data
directory) it prints a JSON line per variant:

    {"file": ..., "reference": "configparser", "variant": ...,
     "result": {"sections": [...], "entries": [[section, key, value], ...]}}
    or "result": {"error": {"type": ..., "line": ...}}

The parsers have no interpolation and preserve the case of keys.  Files are
decoded as UTF-8 (a BOM is skipped) with universal newlines, like a file
opened in text mode.  Sections are listed in file order including the
default section and the unnamed section (allow_unnamed_section, null) if
they have keys.  A value is null for a key without a delimiter (allow_no_value).

Usage: python run-configparser.py <data-dir> < inputs
"""

import configparser
import io
import json
import os
import sys

VARIANTS = {
    # The stock parser.
    "default": {},
    # The relaxations needed for most real world files.
    "lenient": {"strict": False, "allow_no_value": True, "allow_unnamed_section": True},
}


def parse(text, options):
    parser = configparser.RawConfigParser(interpolation=None, **options)
    parser.optionxform = str
    try:
        parser.read_file(io.StringIO(text, newline=None))
    except configparser.Error as exc:
        error = {"type": type(exc).__name__}
        line = getattr(exc, "lineno", None)
        if line is None and getattr(exc, "errors", None):
            line = exc.errors[0][0]
        if line is not None:
            error["line"] = line
        return {"error": error}
    sections, entries = [], []
    groups = list(parser._sections.items())
    if parser._defaults:
        groups.insert(0, (parser.default_section, parser._defaults))
    for name, values in groups:
        if name is getattr(configparser, "UNNAMED_SECTION", object()):
            # configparser always creates it when allowed.
            if not values:
                continue
            name = None
        sections.append(name)
        entries.extend([name, key, value] for key, value in values.items())
    return {"sections": sections, "entries": entries}


def main():
    root = sys.argv[1]
    for line in sys.stdin:
        path = line.strip()
        if not path:
            continue
        with open(os.path.join(root, path), "rb") as f:
            data = f.read()
        try:
            text = data.decode("utf-8-sig")
        except UnicodeDecodeError:
            text = None
        for variant, options in VARIANTS.items():
            if text is None:
                result = {"error": {"type": "UnicodeDecodeError"}}
            else:
                result = parse(text, options)
            record = {"file": path, "reference": "configparser", "variant": variant, "result": result}
            print(json.dumps(record, ensure_ascii=False))


main()
