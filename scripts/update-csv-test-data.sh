#!/usr/bin/env bash
# Vendors the CSV test data into deser-csv/tests/data.
#
# * papaparse: the test cases of PapaParse (a CSV parser for JavaScript)
#   for parsing and writing.  They are extracted from `tests/test-cases.js`
#   into JSON with node (see `csv/extract-papaparse.js`), cases that need
#   functions (like callbacks) are left out.
#
# To update, change the pinned commit below and re-run the script.
set -euo pipefail

PAPAPARSE_REPO=mholt/PapaParse
PAPAPARSE_COMMIT=4843fc2021bbe04bdc176e71f4c9f8f8388af7bc

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="$HERE/../deser-csv/tests/data"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/papaparse"
curl -fsSL "https://github.com/$PAPAPARSE_REPO/archive/$PAPAPARSE_COMMIT.tar.gz" \
  | tar -xz -C "$TMP/papaparse" --strip-components=1

OUT="$DATA/papaparse"
rm -rf "$OUT"
mkdir -p "$OUT"
node "$HERE/csv/extract-papaparse.js" "$TMP/papaparse/tests/test-cases.js" > "$OUT/test-cases.json"
cp "$TMP/papaparse/LICENSE" "$OUT/LICENSE"
cat > "$OUT/SOURCE" <<EOS
https://github.com/$PAPAPARSE_REPO
commit $PAPAPARSE_COMMIT (extracted from tests/test-cases.js)
EOS

echo "Updated $DATA"
