"""Writes captured git config inputs and records how git reads them.

Takes the JSON lines files written by the capture hooks ({"test": ...,
"input": <base64>} or {"test": ..., "input_hex": ...}), and writes the
distinct inputs of every source as <dir>/NNNN.config.  Inputs larger than
64 KiB (generated stress tests) are left out, the paths of temporary
directories in inputs are replaced with /tmp/.tmpXXXXXX (or tmpXXXXXXXX).  Then every input and every
extra file is read with `git config --file <file> --list --null` of the
given docker image, and the result is stored next to it (NNNN.json for
captured inputs, <file>.git.json for extra files):

    {"tests": [...],               (captured inputs only)
     "git": {"entries": [[section, subsection, name, value], ...]}}
    or {"git": {"error": {"line": 3, "message": "bad config line 3 in file <file>"}}}

git lowercases section and variable names but not subsections.  Strings
that are not valid UTF-8 are stored as {"hex": ...}.  The
subsection is null for [section] and "" for [section ""].  The value is
null for a variable without "=" (an implicit true).  The section (and
subsection) is null for variables before the first section header.

Usage: python baseline.py <docker-image> <out-root>
           [<dir>=<capture.jsonl>...] [--extra <file>...]
"""

import base64
import json
import os
import re
import subprocess
import sys
import tempfile

MAX_SIZE = 64 * 1024


# Tests that write include files put the paths of their temporary
# directories into the inputs, those are replaced to keep the inputs stable.
TEMP_ROOTS = sorted(
    {os.fsencode(d.rstrip("/")) for d in (tempfile.gettempdir(), os.path.realpath(tempfile.gettempdir()))},
    key=len,
    reverse=True,
)
TEMP_NAMES = [
    (re.compile(rb"\.tmp[A-Za-z0-9]{6}"), b".tmpXXXXXX"),  # Rust's tempfile
    (re.compile(rb"\btmp[a-z0-9_]{8}\b"), b"tmpXXXXXXXX"),  # Python's tempfile
]


def normalize(data):
    for root in TEMP_ROOTS:
        data = data.replace(root + b"/", b"/tmp/")
    for pattern, replacement in TEMP_NAMES:
        data = pattern.sub(replacement, data)
    return data


def load(path):
    inputs = {}
    with open(path) as f:
        for line in f:
            record = json.loads(line)
            if "input_hex" in record:
                data = bytes.fromhex(record["input_hex"])
            else:
                data = base64.b64decode(record["input"])
            inputs.setdefault(normalize(data), set()).add(record["test"])
    return inputs


def write_inputs(directory, inputs):
    os.makedirs(directory, exist_ok=True)
    skipped = sum(len(data) > MAX_SIZE for data in inputs)
    items = sorted(
        ((sorted(tests), data) for data, tests in inputs.items() if len(data) <= MAX_SIZE),
        key=lambda item: (item[0][0], item[1]),
    )
    written = {}
    for index, (tests, data) in enumerate(items, 1):
        path = os.path.join(directory, f"{index:04}.config")
        with open(path, "wb") as f:
            f.write(data)
        written[path] = tests
    print(f"{directory}: {len(items)} inputs ({skipped} larger than {MAX_SIZE} left out)")
    return written


def run_git(image, root, files):
    """Runs git over all files in one container, returns {file: (code, out, err)}."""
    rel = [os.path.relpath(path, root) for path in files]
    script = r"""
        set -u
        while IFS= read -r f; do
            git config --file "$f" --list --null > "$f.out" 2> "$f.err"
            echo $? > "$f.code"
        done
    """
    subprocess.run(
        ["docker", "run", "--rm", "-i", "-v", f"{os.path.abspath(root)}:/data",
         "-w", "/data", "--entrypoint", "sh", image, "-c", script],
        input="\n".join(rel) + "\n", text=True, check=True,
    )
    results = {}
    for path in files:
        parts = []
        for ext in (".code", ".out", ".err"):
            with open(path + ext, "rb") as f:
                parts.append(f.read())
            os.remove(path + ext)
        results[path] = (int(parts[0]), parts[1], parts[2].decode("utf-8", "replace"))
    return results


def parse_result(name, code, out, err):
    if code != 0:
        message = err.strip().removeprefix("fatal: ").replace(name, "<file>")
        error = {"message": message}
        m = re.search(r"\bline (\d+)\b", message)
        if m:
            error["line"] = int(m.group(1))
        return {"error": error}
    entries = []
    for record in out.split(b"\0")[:-1]:
        key, sep, value = record.partition(b"\n")
        value = text(value) if sep else None
        if b"." not in key:
            # A variable before the first section header.
            entries.append([None, None, text(key), value])
            continue
        first, last = key.index(b"."), key.rindex(b".")
        subsection = text(key[first + 1 : last]) if first != last else None
        entries.append([text(key[:first]), subsection, text(key[last + 1 :]), value])
    return {"entries": entries}


def text(data):
    """Strings that are not UTF-8 are stored as {"hex": ...}."""
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return {"hex": data.hex()}


def dump(value):
    return json.dumps(value, ensure_ascii=False)


def to_json(tests, git):
    parts = []
    if tests is not None:
        parts.append('  "tests": [\n%s\n  ]' % ",\n".join("    " + dump(t) for t in tests))
    if "entries" in git:
        entries = ",\n".join("      " + dump(e) for e in git["entries"])
        entries = "[\n%s\n    ]" % entries if entries else "[]"
        parts.append('  "git": {\n    "entries": %s\n  }' % entries)
    else:
        parts.append('  "git": {"error": %s}' % dump(git["error"]))
    return "{\n%s\n}\n" % ",\n".join(parts)


def main():
    image, root = sys.argv[1], sys.argv[2]
    args = sys.argv[3:]
    extra = []
    if "--extra" in args:
        extra = args[args.index("--extra") + 1 :]
        args = args[: args.index("--extra")]
    captured = {}
    for arg in args:
        directory, path = arg.split("=", 1)
        captured.update(write_inputs(os.path.join(root, directory), load(path)))
    files = list(captured) + extra
    for path, (code, out, err) in run_git(image, root, files).items():
        git = parse_result(os.path.relpath(path, root), code, out, err)
        if path in captured:
            target, tests = path[: -len(".config")] + ".json", captured[path]
        else:
            target, tests = path + ".git.json", None
        with open(target, "w", encoding="utf-8") as f:
            f.write(to_json(tests, git))


main()
