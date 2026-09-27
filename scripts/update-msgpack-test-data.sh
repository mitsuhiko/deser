#!/usr/bin/env bash
# Vendors the MessagePack test data into deser-msgpack/tests/data.
#
# * msgpack-test-suite: values with all of their valid encodings.  Only the
#   prebuilt JSON file is copied.
#
# To update, change the pinned commit below and re-run the script.
set -euo pipefail

SUITE_REPO=kawanet/msgpack-test-suite
SUITE_COMMIT=e04f6edeaae589c768d6b70fcce80aa786b7800e

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="$HERE/../deser-msgpack/tests/data"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/suite"
curl -fsSL "https://github.com/$SUITE_REPO/archive/$SUITE_COMMIT.tar.gz" \
  | tar -xz -C "$TMP/suite" --strip-components=1

OUT="$DATA/msgpack-test-suite"
rm -rf "$OUT"
mkdir -p "$OUT"
cp "$TMP/suite/dist/msgpack-test-suite.json" "$OUT/msgpack-test-suite.json"
cp "$TMP/suite/LICENSE" "$OUT/LICENSE"
cat > "$OUT/SOURCE" <<EOS
https://github.com/$SUITE_REPO
commit $SUITE_COMMIT (dist/msgpack-test-suite.json)
EOS

echo "Updated $DATA"
