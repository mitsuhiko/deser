#!/usr/bin/env python3
"""Generates the parsers and serializers of the JSON dialect crates from the template.

The template (`src/` next to this script) is Rust code where the code that
only some dialects have is marked with `#[cfg(...)]` attributes (and
`cfg!(...)` macros) on made up capabilities (see `CAPABILITIES`).  For every
dialect crate the capabilities are evaluated: code of capabilities that the
dialect has is kept (the attributes are removed), code of other
capabilities is removed.  Attributes with other conditions (like
`feature = "io"` or `test`) are kept, conditions that mix both are
simplified.

The template crate enables all capabilities with its build script, so it
compiles as the dialect with the most features and editors analyze all of
its code.

A `#[cfg]` attribute removes what it's attached to: the node starts after
the attribute (it includes further attributes) and ends at the first `;` or
`,` outside of brackets (commas do not end items like `fn` or `struct`), or
after a block which is not followed by `else`, `.` or `?`.  Comments and
attributes directly above a removed node are removed with it (a blank line
separates them).  If a block statement is kept, it is unwrapped:

    #[cfg(not(json5))]
    {
        return Err(...);
    }

becomes `return Err(...);` for dialects without `json5`.  A file with
`#![cfg(...)]` at the top is only generated for the dialects that match.
Lines with comments that start with `//#` are only in the template (for
instance to explain why code is conditional), they are removed.  Comments
that start with `//#(capability)` are only kept (as regular comments) for
the dialects with the capability.
The generated files are formatted with rustfmt.  `src/copy.rs` is not
generated, it's a symlink to `shared/copy.rs` (like in other formats).

Usage:

    generate.py           regenerates the files
    generate.py --check   fails if the generated files are not up to date
"""

import argparse
import difflib
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TEMPLATE = Path(__file__).resolve().parent / "src"
TESTS = Path(__file__).resolve().parent / "tests"
TEMPLATE_CRATE = "deser_template_json"

# the capabilities that the dialects are made of
CAPABILITIES = {
    # `//` and `/* */` comments
    "comments",
    # a comma after the last element of a sequence or map
    "trailing_commas",
    # strings in single quotes (and the escape `\'`), strings can contain
    # control characters other than line breaks
    "single_quotes",
    # the rest of JSON5: identifiers as keys, more number formats and
    # escapes, `Infinity` and `NaN`
    "json5",
    # the rest of Hjson: quoteless strings and keys, multiline strings, `#`
    # comments, optional commas and maps without braces at the root
    "hjson",
}

DIALECTS = {
    "deser-json": set(),
    "deser-jsonc": {"comments", "trailing_commas"},
    "deser-json5": {"comments", "trailing_commas", "single_quotes", "json5"},
    "deser-hj": {"comments", "trailing_commas", "single_quotes", "hjson"},
}

# the parser and the serializer (JSON is valid in every dialect, they share
# the serializer, only raw values and non-finite floats differ)
FILES = [
    "buf.rs", "de.rs", "escape.rs", "parser.rs", "pretty.rs", "raw.rs",
    "scan.rs", "ser.rs", "stream.rs", "trailing.rs",
]

# names in conditions which are not capabilities
OTHER_CFGS = {"test", "miri", "doc", "docsrs", "debug_assertions", "unix", "windows"}

ITEM_KEYWORDS = {
    "fn", "pub", "struct", "enum", "impl", "mod", "use", "const", "static",
    "type", "trait", "unsafe", "extern", "macro_rules", "let", "async",
}


# comments that only exist in the template
TEMPLATE_COMMENT_RE = re.compile(r"^[ \t]*//#.*\n", re.MULTILINE)
# comments that only exist in the dialects with a capability
CAPABILITY_COMMENT_RE = re.compile(r"^([ \t]*)//#\((\w+)\) ?(.*\n)", re.MULTILINE)


class TemplateError(Exception):
    pass


# -- tokenizer ---------------------------------------------------------------

TOKEN_RE = re.compile(
    r"""
    (?P<ws>\s+)
  | (?P<comment>//[^\n]*)
  | (?P<blockcomment>/\*)
  | (?P<rawstr>b?r(?P<hashes>\#*)")
  | (?P<str>b?"(?:\\.|[^"\\])*")
  | (?P<char>b?'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^'\\])')
  | (?P<lifetime>'[A-Za-z_][A-Za-z0-9_]*)
  | (?P<ident>[A-Za-z_][A-Za-z0-9_]*)
  | (?P<number>[0-9][0-9A-Za-z_]*(?:\.[0-9][0-9A-Za-z_]*)?)
  | (?P<punct>.)
    """,
    re.VERBOSE | re.DOTALL,
)


class Token:
    __slots__ = ("kind", "text", "start", "end")

    def __init__(self, kind, text, start, end):
        self.kind = kind
        self.text = text
        self.start = start
        self.end = end

    def __repr__(self):
        return f"Token({self.kind}, {self.text!r}, {self.start})"


def tokenize(source):
    tokens = []
    pos = 0
    while pos < len(source):
        m = TOKEN_RE.match(source, pos)
        kind = m.lastgroup if m.lastgroup != "hashes" else "rawstr"
        if m.group("rawstr") is not None:
            kind = "rawstr"
            end = source.find('"' + m.group("hashes"), m.end())
            if end < 0:
                raise TemplateError(f"unterminated raw string at {pos}")
            end += 1 + len(m.group("hashes"))
        elif kind == "blockcomment":
            depth, end = 1, m.end()
            while depth:
                nxt = min(
                    (i for i in (source.find("/*", end), source.find("*/", end)) if i >= 0),
                    default=-1,
                )
                if nxt < 0:
                    raise TemplateError(f"unterminated block comment at {pos}")
                depth += 1 if source.startswith("/*", nxt) else -1
                end = nxt + 2
            kind = "comment"
        else:
            end = m.end()
        if kind != "ws":
            if kind in ("rawstr", "str", "char", "number"):
                kind = "literal"
            tokens.append(Token(kind, source[pos:end], pos, end))
        pos = end
    return tokens


# -- conditions --------------------------------------------------------------


def parse_predicate(tokens, i):
    """Parses a cfg predicate starting at `tokens[i]`.

    Returns the predicate and the index after it.  Predicates are
    `("cap", name)`, `("other", text)`, `("not", p)`, `("all", [p])` and
    `("any", [p])`.
    """
    tok = tokens[i]
    if tok.kind != "ident":
        raise TemplateError(f"invalid cfg predicate at {tok.start}")
    if tok.text in ("all", "any", "not") and tokens[i + 1].text == "(":
        i += 2
        args = []
        while tokens[i].text != ")":
            pred, i = parse_predicate(tokens, i)
            args.append(pred)
            if tokens[i].text == ",":
                i += 1
        i += 1
        if tok.text == "not":
            if len(args) != 1:
                raise TemplateError(f"not() takes one predicate at {tok.start}")
            return ("not", args[0]), i
        return (tok.text, args), i
    if tokens[i + 1].text == "=":
        return ("other", f"{tok.text} = {tokens[i + 2].text}"), i + 3
    if tok.text in CAPABILITIES:
        return ("cap", tok.text), i + 1
    if tok.text in OTHER_CFGS:
        return ("other", tok.text), i + 1
    raise TemplateError(f"unknown cfg {tok.text!r} at {tok.start}")


def uses_capability(pred):
    kind, arg = pred
    if kind == "cap":
        return True
    if kind == "other":
        return False
    if kind == "not":
        return uses_capability(arg)
    return any(uses_capability(p) for p in arg)


def evaluate(pred, caps):
    """Evaluates the capabilities in a predicate.

    Returns `True`, `False` or the predicate with the other conditions.
    """
    kind, arg = pred
    if kind == "cap":
        return arg in caps
    if kind == "other":
        return pred
    if kind == "not":
        value = evaluate(arg, caps)
        return (not value) if isinstance(value, bool) else ("not", value)
    short = kind == "any"
    rest = []
    for p in arg:
        value = evaluate(p, caps)
        if value is short:
            return short
        if value is not (not short):
            rest.append(value)
    if not rest:
        return not short
    return rest[0] if len(rest) == 1 else (kind, rest)


def render(pred):
    kind, arg = pred
    if kind == "other":
        return arg
    if kind == "not":
        return f"not({render(arg)})"
    return f"{kind}({', '.join(render(p) for p in arg)})"


# -- generator ---------------------------------------------------------------


def matching(tokens, i):
    """Returns the index of the bracket closing the one at `tokens[i]`."""
    depth = 0
    for j in range(i, len(tokens)):
        if tokens[j].text in "([{" and tokens[j].kind == "punct":
            depth += 1
        elif tokens[j].text in ")]}" and tokens[j].kind == "punct":
            depth -= 1
            if depth == 0:
                return j
    raise TemplateError(f"unbalanced bracket at {tokens[i].start}")


def next_code(tokens, i):
    while i < len(tokens) and tokens[i].kind == "comment":
        i += 1
    return i


def node_end(tokens, i):
    """Returns the end offset of the node starting at `tokens[i]`."""
    first = next_code(tokens, i)
    # skip further attributes to find out what the node is
    while tokens[first].text == "#" and tokens[first + 1].text == "[":
        first = next_code(tokens, matching(tokens, first + 1) + 1)
    is_item = tokens[first].text in ITEM_KEYWORDS
    if tokens[first].text == "pub":
        # `pub field: Type,` is a field
        j = first + 1
        if tokens[j].text == "(":
            j = matching(tokens, j) + 1
        if tokens[j].kind == "ident" and tokens[j + 1].text == ":" and tokens[j + 2].text != ":":
            is_item = False

    depth = 0
    j = i
    while j < len(tokens):
        tok = tokens[j]
        if tok.kind == "punct":
            if tok.text in "([{":
                depth += 1
            elif tok.text in ")]}":
                if depth == 0:
                    # the enclosing node ends
                    return tokens[j - 1].end
                depth -= 1
                if depth == 0 and tok.text == "}":
                    k = next_code(tokens, j + 1)
                    if k < len(tokens):
                        nxt = tokens[k].text
                        if nxt in (",", ";"):
                            return tokens[k].end
                        if nxt in ("else", ".", "?"):
                            j += 1
                            continue
                    return tok.end
            elif depth == 0 and (tok.text == ";" or (tok.text == "," and not is_item)):
                return tok.end
        j += 1
    raise TemplateError(f"node at {tokens[i].start} does not end")


def node_start(source, tokens, i):
    """Extends the start of a node at `tokens[i]` over the comments and
    attributes directly above it."""
    start = tokens[i].start
    while i > 0:
        prev = tokens[i - 1]
        if prev.kind == "comment":
            j = i - 1
        elif prev.text == "]":
            depth, j = 0, i - 1
            while j >= 0:
                if tokens[j].text == "]":
                    depth += 1
                elif tokens[j].text == "[":
                    depth -= 1
                    if depth == 0:
                        break
                j -= 1
            j -= 1
            if j < 0 or tokens[j].text != "#":
                break
        else:
            break
        # a blank line separates the node from what is above it
        if source.count("\n", tokens[j].end, start) > 1:
            break
        start = tokens[j].start
        i = j
    return start


def expand_lines(source, start, end):
    """Expands a range to whole lines if nothing else is on them."""
    line_start = source.rfind("\n", 0, start) + 1
    if source[line_start:start].strip():
        return start, end
    line_end = source.find("\n", end)
    if line_end < 0:
        line_end = len(source)
    rest = source[end:line_end].strip()
    if rest and not rest.startswith("//"):
        return start, end
    return line_start, min(line_end + 1, len(source))


def generate(source, caps, crate):
    tokens = tokenize(source)
    edits = []  # (start, end, replacement)
    skip_file = False

    def add(start, end, text):
        # a removed node contains the edits within it
        edits[:] = [e for e in edits if not (start <= e[0] and e[1] <= end)]
        edits.append((start, end, text))

    i = 0
    while i < len(tokens):
        tok = tokens[i]
        # `cfg!(...)`
        if (
            tok.text == "cfg"
            and tok.kind == "ident"
            and tokens[i + 1].text == "!"
            and tokens[i + 2].text == "("
        ):
            pred, j = parse_predicate(tokens, i + 3)
            if uses_capability(pred):
                value = evaluate(pred, caps)
                text = str(value).lower() if isinstance(value, bool) else f"cfg!({render(value)})"
                add(tok.start, tokens[j].end, text)
            i = j + 1
            continue

        # `#[cfg(...)]`, `#![cfg(...)]` and `#[cfg_attr(..., ...)]`
        if tok.text != "#" or tok.kind != "punct":
            i += 1
            continue
        inner = tokens[i + 1].text == "!"
        open_idx = i + 2 if inner else i + 1
        if tokens[open_idx].text != "[":
            i += 1
            continue
        close_idx = matching(tokens, open_idx)
        name = tokens[open_idx + 1]
        if name.text not in ("cfg", "cfg_attr") or tokens[open_idx + 2].text != "(":
            i = close_idx + 1
            continue
        pred, j = parse_predicate(tokens, open_idx + 3)
        if not uses_capability(pred):
            i = close_idx + 1
            continue
        value = evaluate(pred, caps)
        attr_start, attr_end = tok.start, tokens[close_idx].end

        if name.text == "cfg_attr":
            if tokens[j].text != ",":
                raise TemplateError(f"invalid cfg_attr at {tok.start}")
            attrs = source[tokens[j + 1].start : tokens[close_idx - 1].end]
            bang = "!" if inner else ""
            if value is True:
                add(attr_start, attr_end, f"#{bang}[{attrs}]")
            elif value is False:
                add(*expand_lines(source, attr_start, attr_end), "")
            else:
                add(attr_start, attr_end, f"#{bang}[cfg_attr({render(value)}, {attrs})]")
            i = close_idx + 1
            continue

        if value is True:
            add(*expand_lines(source, attr_start, attr_end), "")
            # a block statement is unwrapped
            block = next_code(tokens, close_idx + 1)
            if not inner and tokens[block].text == "{" and tokens[block].kind == "punct":
                block_end = matching(tokens, block)
                if node_end(tokens, block) == tokens[block_end].end:
                    add(*expand_lines(source, tokens[block].start, tokens[block].end), "")
                    add(*expand_lines(source, tokens[block_end].start, tokens[block_end].end), "")
            i = close_idx + 1
        elif value is False:
            if inner:
                skip_file = True
                break
            start = node_start(source, tokens, i)
            end = node_end(tokens, close_idx + 1)
            add(*expand_lines(source, start, end), "")
            # continue after the removed node
            while i < len(tokens) and tokens[i].start < end:
                i += 1
        else:
            bang = "!" if inner else ""
            add(attr_start, attr_end, f"#{bang}[cfg({render(value)})]")
            i = close_idx + 1

    if skip_file:
        return None
    edits.sort()
    out = []
    pos = 0
    for start, end, text in edits:
        if start < pos:
            raise TemplateError(f"overlapping edits at {start}")
        out.append(source[pos:start])
        out.append(text)
        pos = end
    out.append(source[pos:])

    def capability_comment(m):
        if m.group(2) not in CAPABILITIES:
            raise TemplateError(f"unknown capability in comment {m.group(0)!r}")
        return f"{m.group(1)}// {m.group(3)}" if m.group(2) in caps else ""

    out = CAPABILITY_COMMENT_RE.sub(capability_comment, "".join(out))
    out = TEMPLATE_COMMENT_RE.sub("", out)
    return out.replace(TEMPLATE_CRATE, crate)


def rustfmt(source):
    rv = subprocess.run(
        ["rustfmt", "--edition", "2024", "--emit", "stdout"],
        input=source,
        capture_output=True,
        text=True,
    )
    if rv.returncode != 0:
        raise TemplateError(f"rustfmt failed:\n{rv.stderr}")
    return rv.stdout


def header(name):
    return (
        f"// @generated from deser-template-json/src/{name} by\n"
        f"// deser-template-json/generate.py.  Do not edit.\n"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true", help="check that the files are up to date")
    args = parser.parse_args()

    # the tests of the dialects know their capabilities
    for dialect, caps in sorted(DIALECTS.items()):
        tests = TESTS / (dialect.removeprefix("deser-") + ".rs")
        text = tests.read_text()
        for cap in sorted(CAPABILITIES):
            if f"{cap}: {str(cap in caps).lower()}," not in text:
                sys.exit(f"{tests}: the capability {cap} does not match {dialect}")

    outdated = []
    for dialect, caps in sorted(DIALECTS.items()):
        crate = dialect.replace("-", "_")
        for name in FILES:
            template_path = TEMPLATE / name
            target = ROOT / dialect / "src" / name
            try:
                text = generate(template_path.read_text(), caps, crate)
            except TemplateError as err:
                sys.exit(f"{template_path}: {err}")
            expected = None if text is None else header(name) + rustfmt(text)
            current = target.read_text() if target.exists() else None
            if expected == current:
                continue
            outdated.append(target)
            if args.check:
                diff = difflib.unified_diff(
                    (current or "").splitlines(True),
                    (expected or "").splitlines(True),
                    str(target),
                    "generated",
                )
                sys.stdout.writelines(diff)
            elif expected is None:
                target.unlink()
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(expected)

    if args.check and outdated:
        sys.exit(
            "generated files are out of date, run "
            "`python3 deser-template-json/generate.py`"
        )
    for target in outdated:
        print(f"updated {target.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
