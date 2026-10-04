"""Extracts git config inputs from string literals in test sources.

For test suites that are impractical to run (JGit needs Maven, isomorphic-git
jest), the inputs are taken from the arguments of the calls that parse
config text, as long as the argument is made of string literals only:

* java: parse("...") and fromText("..." + "...") calls and the helpers of
  JGit's ConfigTest that build the text they parse from a value or
  subsection (with the same template), the test is the enclosing method.
* js: GitConfig.from(`...`) calls, the test is the enclosing it('...').

The inputs are appended to a JSON lines file as {"test": ..., "input": <base64>}.

Usage: python extract-literals.py java|js <out.jsonl> <file>...
"""

import base64
import json
import os
import re
import sys

JAVA_ESCAPES = {
    "b": "\b", "t": "\t", "n": "\n", "f": "\f", "r": "\r", "s": " ",
    '"': '"', "'": "'", "\\": "\\",
}
JS_ESCAPES = {
    "b": "\b", "t": "\t", "n": "\n", "f": "\f", "r": "\r", "v": "\v", "0": "\0",
    '"': '"', "'": "'", "\\": "\\", "`": "`", "$": "$", "\n": "",
}


class NotALiteral(Exception):
    pass


def skip_space(src, pos):
    while True:
        while pos < len(src) and src[pos].isspace():
            pos += 1
        if src.startswith("//", pos):
            pos = src.index("\n", pos)
        elif src.startswith("/*", pos):
            pos = src.index("*/", pos) + 2
        else:
            return pos


def read_escape(src, pos, escapes):
    """Reads the escape sequence after a backslash at pos."""
    c = src[pos]
    if c == "u":
        return chr(int(src[pos + 1 : pos + 5], 16)), pos + 5
    if c == "x" and escapes is JS_ESCAPES:
        return chr(int(src[pos + 1 : pos + 3], 16)), pos + 3
    if c in "01234567" and escapes is JAVA_ESCAPES:
        m = re.match(r"[0-3][0-7]{0,2}|[0-7]{1,2}", src[pos:])
        return chr(int(m.group(), 8)), pos + m.end()
    if c in escapes:
        return escapes[c], pos + 1
    raise NotALiteral(f"unknown escape \\{c}")


def read_string(src, pos, escapes):
    """Reads a "...", '...' or `...` literal starting at pos."""
    quote = src[pos]
    pos += 1
    out = []
    while True:
        c = src[pos]
        if c == quote:
            return "".join(out), pos + 1
        if c == "\\":
            value, pos = read_escape(src, pos + 1, escapes)
            out.append(value)
            continue
        if quote == "`" and src.startswith("${", pos):
            raise NotALiteral("template literal with substitutions")
        if c == "\n" and quote != "`":
            raise NotALiteral("unterminated string")
        out.append(c)
        pos += 1


def read_argument(src, pos, escapes, quotes):
    """Reads a concatenation of string literals up to the end of the argument."""
    parts = []
    while True:
        pos = skip_space(src, pos)
        if pos >= len(src) or src[pos] not in quotes:
            raise NotALiteral("not a string literal")
        part, pos = read_string(src, pos, escapes)
        parts.append(part)
        pos = skip_space(src, pos)
        if src[pos] in ",)":
            return "".join(parts), pos
        if src[pos] != "+":
            raise NotALiteral("not a string literal")
        pos += 1


def skip_argument(src, pos, escapes, quotes):
    """Skips any expression up to the end of the argument."""
    depth = 0
    while True:
        c = src[pos]
        if c in quotes:
            _, pos = read_string(src, pos, escapes)
            continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                return pos
            depth -= 1
        elif c == "," and depth == 0:
            return pos
        pos += 1


def read_arguments(src, pos, escapes, quotes):
    """Reads the arguments of a call, None for the ones that are not literals."""
    args = []
    while True:
        try:
            arg, pos = read_argument(src, pos, escapes, quotes)
        except NotALiteral:
            arg, pos = None, skip_argument(src, pos, escapes, quotes)
        args.append(arg)
        if src[pos] == ")":
            return args
        pos += 1


def value(arg):
    return None if arg is None else "[foo]\nbar=" + arg


def subsection(arg):
    return None if arg is None else "[foo " + arg + "]\nbar = value"


# (call, text parsed by the call given its arguments)
JAVA_CALLS = [
    (r"(?<![\w.])parse\(", lambda args: args[0]),
    (r"\.fromText\(", lambda args: args[0]),
    (r"\bparseEscapedValue\(", lambda args: value(args[0])),
    (r"\bassertValueRoundTrip\(", lambda args: value(args[-1])),
    (r"\bassertInvalidValue\(", lambda args: value(args[1])),
    (r"\bparseEscapedSubsection\(", lambda args: subsection(args[0])),
    (r"\bassertSubsectionRoundTrip\(", lambda args: subsection(args[1])),
    (r"\bassertInvalidSubsection\(", lambda args: subsection(args[1])),
]
JS_CALLS = [(r"\bGitConfig\.from\(", lambda args: args[0])]


def enclosing(src, pos, pattern):
    matches = list(re.finditer(pattern, src[:pos]))
    return matches[-1].group(1) if matches else ""


def extract(lang, path):
    with open(path, encoding="utf-8") as f:
        src = f.read()
    if lang == "java":
        calls, escapes, quotes = JAVA_CALLS, JAVA_ESCAPES, '"'
        test = r"void\s+(\w+)\s*\("
    else:
        calls, escapes, quotes = JS_CALLS, JS_ESCAPES, "\"'`"
        test = r"\bit\(\s*['\"`](.*?)['\"`]\s*,"
    name = os.path.splitext(os.path.basename(path))[0]
    found = []
    for pattern, template in calls:
        for m in re.finditer(pattern, src):
            text = template(read_arguments(src, m.end(), escapes, quotes))
            if text is not None:
                found.append((m.start(), f"{name}.{enclosing(src, m.start(), test)}", text))
    for _, test_name, text in sorted(found):
        yield test_name, text


def main():
    lang, out, files = sys.argv[1], sys.argv[2], sys.argv[3:]
    count = 0
    with open(out, "a") as f:
        for path in files:
            for test, text in extract(lang, path):
                data = base64.b64encode(text.encode("utf-8")).decode()
                f.write(json.dumps({"test": test, "input": data}) + "\n")
                count += 1
    print(f"extracted {count} inputs from {len(files)} files")


main()
