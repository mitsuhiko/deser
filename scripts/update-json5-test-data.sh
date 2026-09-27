#!/usr/bin/env bash
# Vendors the JSON5 test suite into deser-json5/tests/data.
#
# The suite (https://github.com/json5/json5-tests) only tells which files
# are valid: `.json` and `.json5` files are, `.js` and `.txt` files are not.
# The expected values of the valid files are computed with node, the same
# way the suite defines them (with `eval` as JSON5 is ECMAScript 5), and
# written to json5-tests.expected.json.
#
# To update, change the pinned commit below and re-run the script (this
# needs curl and node).
set -euo pipefail

JSON5_TESTS_REPO=json5/json5-tests
JSON5_TESTS_COMMIT=ceb24d4080137d70833f86c25659c1331b80a387

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="$HERE/../deser-json5/tests/data"
OUT="$DATA/json5-tests"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL "https://github.com/$JSON5_TESTS_REPO/archive/$JSON5_TESTS_COMMIT.tar.gz" \
  | tar -xz -C "$TMP" --strip-components=1

rm -rf "$OUT"
mkdir -p "$OUT"
for dir in "$TMP"/*/; do
  cp -R "$dir" "$OUT/$(basename "$dir")"
done
cp "$TMP/LICENSE.md" "$OUT/LICENSE.md"
cat > "$OUT/SOURCE" <<EOF
https://github.com/$JSON5_TESTS_REPO
commit $JSON5_TESTS_COMMIT
EOF

# The values are encoded so that nothing is lost in JSON: every value is a
# sequence of its kind and its data.  Numbers are strings (for Infinity,
# NaN and -0) and maps are sequences of the keys and values.
node - "$OUT" > "$DATA/json5-tests.expected.json" <<'EOF'
const fs = require("fs");
const path = require("path");
const root = process.argv[2];

function encode(value) {
  if (value === null) return ["null"];
  switch (typeof value) {
    case "boolean": return ["bool", value];
    case "number": return ["number", Object.is(value, -0) ? "-0" : String(value)];
    case "string": return ["str", value];
  }
  if (Array.isArray(value)) return ["seq", value.map(encode)];
  return ["map", Object.keys(value).map((key) => [key, encode(value[key])])];
}

function walk(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const file = path.join(dir, entry.name);
    return entry.isDirectory() ? walk(file) : [file];
  });
}

const expected = {};
for (const file of walk(root).sort()) {
  const name = path.relative(root, file).split(path.sep).join("/");
  const text = fs.readFileSync(file, "utf8");
  // some `.json` files have comments, they are evaluated like JSON5
  if (name.endsWith(".json") || name.endsWith(".json5")) {
    expected[name] = encode(eval("(" + text + "\n)"));
  }
}
// a line per file
const lines = Object.entries(expected).map(
  ([name, value]) => "  " + JSON.stringify(name) + ": " + JSON.stringify(value)
);
process.stdout.write("{\n" + lines.join(",\n") + "\n}\n");
EOF

echo "Updated $DATA"
