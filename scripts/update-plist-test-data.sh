#!/usr/bin/env bash
# Vendors the property list test data into deser-plist/tests/data.
#
# * rust-plist: the test files of the `plist` crate.  For every file that
#   Core Foundation can read, its conversion to XML by `plutil` is stored in
#   `expected/` so the tests can compare against Apple's implementation.
#   Files that Core Foundation rejects are listed in `expected/errors.txt`.
#
# `plutil` is only available on macOS.  To update, change the pinned commit
# below and re-run the script.
set -euo pipefail

PLIST_REPO=ebarnard/rust-plist
PLIST_COMMIT=10ba3e3b44adb6246f3f5cc8e66134e03498f634

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="$HERE/../deser-plist/tests/data"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if ! command -v plutil > /dev/null; then
  echo "plutil is required (macOS only)" >&2
  exit 1
fi

mkdir -p "$TMP/plist"
curl -fsSL "https://github.com/$PLIST_REPO/archive/$PLIST_COMMIT.tar.gz" \
  | tar -xz -C "$TMP/plist" --strip-components=1

OUT="$DATA/rust-plist"
rm -rf "$OUT"
mkdir -p "$OUT/expected"
cp "$TMP/plist/tests/data/"* "$OUT/"
cp "$TMP/plist/LICENCE" "$OUT/LICENSE"
cat > "$OUT/SOURCE" <<EOS
https://github.com/$PLIST_REPO
commit $PLIST_COMMIT (tests/data)

netnewswire.pbxproj originates from https://github.com/Ranchero-Software/NetNewsWire
(MIT licensed), ascii-sample.plist from the GNUstep wiki.  The files in
expected/ were generated with plutil (see scripts/update-plist-test-data.sh).
EOS

: > "$OUT/expected/errors.txt"
for file in "$OUT"/*.plist "$OUT"/*.pbxproj; do
  name="$(basename "$file")"
  if ! plutil -convert xml1 -o "$OUT/expected/$name.xml" "$file" 2> /dev/null; then
    rm -f "$OUT/expected/$name.xml"
    echo "$name" >> "$OUT/expected/errors.txt"
  fi
done

echo "Updated $DATA"
