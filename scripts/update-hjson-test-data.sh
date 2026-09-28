#!/usr/bin/env bash
# Vendors the Hjson test suite into deser-hjson/tests/data.
#
# The suite (the `testCases` of https://github.com/hjson/hjson) has a
# `NAME_test.hjson` (or `.json`) file per case.  Cases whose name starts
# with `fail` are invalid, for the others `NAME_result.json` holds the
# expected value as JSON.  The expected output of serializing as Hjson
# (`NAME_result.hjson`, `sorted/` and `stringify/`) is not used as values
# are serialized as JSON.
#
# To update, change the pinned commit below and re-run the script (this
# needs curl).
set -euo pipefail

HJSON_REPO=hjson/hjson
HJSON_COMMIT=414a9871b82ce80d8b140e30ff3458706904c160

HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="$HERE/../deser-hjson/tests/data/hjson-tests"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL "https://github.com/$HJSON_REPO/archive/$HJSON_COMMIT.tar.gz" \
  | tar -xz -C "$TMP" --strip-components=1

rm -rf "$OUT"
mkdir -p "$OUT/extra"
for file in "$TMP"/testCases/*_test.* "$TMP"/testCases/*_result.json; do
  cp "$file" "$OUT/"
done
for file in "$TMP"/testCases/extra/*_test.* "$TMP"/testCases/extra/*_result.json; do
  cp "$file" "$OUT/extra/"
done
cp "$TMP/LICENSE" "$OUT/LICENSE"
cat > "$OUT/SOURCE" <<SOURCE
https://github.com/$HJSON_REPO (testCases)
commit $HJSON_COMMIT
SOURCE

echo "Updated $OUT"
