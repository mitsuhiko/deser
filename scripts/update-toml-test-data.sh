#!/usr/bin/env bash
# Vendors the TOML conformance test data into deser-toml/tests/data.
#
# * toml-test: the language agnostic TOML test suite.  Only the files that
#   apply to TOML 1.1.0 (as listed in `tests/files-toml-1.1.0`) are copied.
#
# To update, change the pinned commit below and re-run the script.
set -euo pipefail

TOML_TEST_REPO=toml-lang/toml-test
# master after v2.2.0 (includes additional invalid cases)
TOML_TEST_COMMIT=ff49d109861c1ad25af53f687f2aef19ab650600

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="$HERE/../deser-toml/tests/data"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/toml-test"
curl -fsSL "https://github.com/$TOML_TEST_REPO/archive/$TOML_TEST_COMMIT.tar.gz" \
  | tar -xz -C "$TMP/toml-test" --strip-components=1

OUT="$DATA/toml-test"
rm -rf "$OUT"
mkdir -p "$OUT"

LIST="$TMP/toml-test/tests/files-toml-1.1.0"
while IFS= read -r file; do
  [ -z "$file" ] && continue
  mkdir -p "$OUT/$(dirname "$file")"
  cp "$TMP/toml-test/tests/$file" "$OUT/$file"
done < "$LIST"
cp "$LIST" "$OUT/files-toml-1.1.0"
cp "$TMP/toml-test/LICENSE" "$OUT/LICENSE"
cat > "$OUT/SOURCE" <<EOF
https://github.com/$TOML_TEST_REPO
commit $TOML_TEST_COMMIT (files listed in tests/files-toml-1.1.0)
EOF

echo "Updated $DATA"
