"""Writes the results of the reference parsers, one file per input.

Reads the JSON lines of the reference runners and writes
<data>/references/<input>.json with the results of every reference and
variant (in the order of the arguments):

    {"inih": {"default": {...}, ...}, "configparser": {...}, "php": {...}}

A variant with the same result as the first variant of its reference is
written as {"same_as": <first variant>}.

Records name their reference and variant unless the argument does
(reference:variant=records.jsonl, for inih).  Strings given as {"hex": ...}
are turned into text if they are UTF-8.

Usage: python write.py <data-dir> [<reference>:<variant>=]<records.jsonl>...
"""

import json
import os
import shutil
import sys


def unhex(value):
    if isinstance(value, dict) and set(value) == {"hex"}:
        data = bytes.fromhex(value["hex"])
        try:
            return data.decode("utf-8")
        except UnicodeDecodeError:
            return value
    if isinstance(value, list):
        return [unhex(item) for item in value]
    if isinstance(value, dict):
        return {key: unhex(item) for key, item in value.items()}
    return value


def dump(value, indent=0):
    """JSON with objects and lists expanded, list items on one line each."""
    pad = "  " * (indent + 1)
    if isinstance(value, dict) and value:
        items = [f"{pad}{json.dumps(k)}: {dump(v, indent + 1)}" for k, v in value.items()]
        return "{\n%s\n%s}" % (",\n".join(items), "  " * indent)
    if isinstance(value, list) and value:
        items = [pad + json.dumps(item, ensure_ascii=False) for item in value]
        return "[\n%s\n%s]" % (",\n".join(items), "  " * indent)
    return json.dumps(value, ensure_ascii=False)


def main():
    root = sys.argv[1]
    results = {}
    for arg in sys.argv[2:]:
        label, _, path = arg.rpartition("=")
        with open(path, encoding="utf-8") as f:
            for line in f:
                record = unhex(json.loads(line))
                if label:
                    reference, variant = label.split(":")
                else:
                    reference, variant = record["reference"], record["variant"]
                result = record.get("result") or {
                    k: v for k, v in record.items() if k not in ("file", "reference", "variant")
                }
                file_results = results.setdefault(record["file"], {})
                file_results.setdefault(reference, {})[variant] = result

    # A variant that reads an input like the first variant of its reference
    # refers to it, which also shows which options matter for an input.
    for file_results in results.values():
        for variants in file_results.values():
            first, *rest = variants
            for variant in rest:
                if variants[variant] == variants[first]:
                    variants[variant] = {"same_as": first}

    out = os.path.join(root, "references")
    shutil.rmtree(out, ignore_errors=True)
    for path, file_results in sorted(results.items()):
        target = os.path.join(out, path + ".json")
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with open(target, "w", encoding="utf-8") as f:
            f.write(dump(file_results) + "\n")
    print(f"wrote reference results for {len(results)} inputs")


main()
