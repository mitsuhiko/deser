#!/usr/bin/env python3
"""Creates the seed corpora of the fuzz targets from the test data.

The inputs are written to `corpus/<target>/` with a header of zeroes (the
default configuration, see `Input` in `src/lib.rs`).  Inputs that are
already there are kept, so this can be run on an existing corpus.
"""

import hashlib
import json
import struct
from pathlib import Path

FUZZ = Path(__file__).resolve().parent
ROOT = FUZZ.parent
HEADER = b"\0" * 9
# longer inputs are not useful as seeds (and slow)
MAX_LEN = 16 * 1024


def files(*patterns):
    for pattern in patterns:
        for path in sorted(ROOT.glob(pattern)):
            if path.is_file() and path.name not in ("LICENSE", "SOURCE"):
                yield path.read_bytes()


def cases(path, key="input"):
    """The inputs of the `cases.json` files of the test data."""
    for case in json.loads((ROOT / path).read_text()):
        if key in case:
            yield case[key]


def json_values():
    for data in files(
        "deser-yaml/tests/data/**/*.json",
        "deser-toml/tests/data/**/*.json",
        "deser-json5/tests/data/**/*.json",
    ):
        try:
            yield json.loads(data)
        except ValueError:
            pass


def cbor(value):
    """A minimal CBOR encoder for JSON values."""

    def head(major, n):
        if n < 24:
            return bytes([major << 5 | n])
        for info, fmt in ((24, ">B"), (25, ">H"), (26, ">I"), (27, ">Q")):
            try:
                return bytes([major << 5 | info]) + struct.pack(fmt, n)
            except struct.error:
                pass
        raise ValueError("integer too large")

    if value is None:
        return b"\xf6"
    if value is True:
        return b"\xf5"
    if value is False:
        return b"\xf4"
    if isinstance(value, int):
        return head(0, value) if value >= 0 else head(1, -1 - value)
    if isinstance(value, float):
        return b"\xfb" + struct.pack(">d", value)
    if isinstance(value, str):
        data = value.encode("utf-8", "surrogatepass")
        return head(3, len(data)) + data
    if isinstance(value, list):
        return head(4, len(value)) + b"".join(cbor(v) for v in value)
    if isinstance(value, dict):
        return head(5, len(value)) + b"".join(cbor(k) + cbor(v) for k, v in value.items())
    raise TypeError(type(value))


def msgpack():
    suite = ROOT / "deser-msgpack/tests/data/msgpack-test-suite/msgpack-test-suite.json"
    for group in json.loads(suite.read_text()).values():
        for case in group:
            for encoded in case["msgpack"]:
                yield bytes.fromhex(encoded.replace("-", ""))


def text(inputs):
    for value in inputs:
        if isinstance(value, str):
            yield value.encode("utf-8", "surrogatepass")


def seeds():
    json_files = list(
        files(
            "deser-yaml/tests/data/**/*.json",
            "deser-json5/tests/data/**/*.json",
            "deser-toml/tests/data/**/*.json",
        )
    )
    return {
        "json": json_files,
        "jsonc": json_files + list(files("deser-json5/tests/data/**/*.json5")),
        "json5": list(files("deser-json5/tests/data/**/*.json5", "deser-json5/tests/data/**/*.js"))
        + json_files,
        "hj": list(files("deser-hj/tests/data/**/*.hjson")),
        "yaml": list(files("deser-yaml/tests/data/**/*.yaml")),
        "toml": list(files("deser-toml/tests/data/**/*.toml")),
        "ini": list(
            files(
                "deser-ini/tests/data/**/*.ini",
                "deser-ini/tests/data/**/*.config",
            )
        ),
        "csv": list(text(cases("deser-csv/tests/data/papaparse/test-cases.json"))),
        "xml": list(files("deser-plist/tests/data/**/*.xml")),
        "plist": list(files("deser-plist/tests/data/**/*.plist", "deser-plist/tests/data/**/*.pbxproj")),
        "php": list(text(cases("deser-php/tests/data/php-serialize/cases.json"))),
        "pickle": [
            bytes.fromhex(value)
            for value in cases("deser-pickle/tests/data/pickle/cases.json")
        ],
        "msgpack": list(msgpack()),
        "cbor": [cbor(value) for value in json_values()],
        "urlencoded": [
            b"a=1&b=2",
            b"name=Jane+Doe&tags[]=a&tags[]=b",
            b"user[name]=x&user[address][city]=y&ids[0]=1&ids[1]=2",
            b"a.b.c=1&a.b.d=%20x%2B",
            b"flag&empty=&=value&x=%E2%9C%93",
        ],
        "env": [
            b"APP_NAME=shop\nAPP_DEBUG=yes\nAPP_SERVER__PORT=8080\nPATH=/usr/bin",
            b"APP_HOSTS__0=a\nAPP_HOSTS__1=b\nAPP_FEATURES__NEW_UI=on",
            b"APP_ID=1\nAPP_LIST__0__KEY=x\nAPP_LIST__0__VALUE=1.5\nAPP_TYPE=circle",
        ],
    }


def main():
    for target, inputs in seeds().items():
        corpus = FUZZ / "corpus" / target
        corpus.mkdir(parents=True, exist_ok=True)
        count = 0
        for data in inputs:
            if len(data) > MAX_LEN:
                continue
            data = HEADER + data
            (corpus / hashlib.sha1(data).hexdigest()).write_bytes(data)
            count += 1
        print(f"{target}: {count} inputs")


if __name__ == "__main__":
    main()
