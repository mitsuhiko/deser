#!/usr/bin/env bash
# Vendors the YAML conformance test data into deser-yaml/tests/data.
#
# * yaml-test-suite: the official YAML test suite (syntax / event level).
#   We use the generated `data` branch which has one directory per test.
# * yaml-test-schema: expected scalar resolution for the failsafe, JSON,
#   core (YAML 1.2) and YAML 1.1 schemas.
#
# To update, change the pinned commits below and re-run the script.
set -euo pipefail

YAML_TEST_SUITE_REPO=yaml/yaml-test-suite
# `data` branch, generated from release v2022-01-17
YAML_TEST_SUITE_COMMIT=6ad3d2c62885d82fc349026c136ef560838fdf3d
# the `data` branch has no license file, it's taken from the release
YAML_TEST_SUITE_RELEASE=v2022-01-17

YAML_TEST_SCHEMA_REPO=perlpunk/yaml-test-schema
YAML_TEST_SCHEMA_COMMIT=0276b888c6cdaf8e634a5ee79ffd17737d41df2b

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="$HERE/../deser-yaml/tests/data"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fetch() {
  local repo=$1 commit=$2 dest=$3
  mkdir -p "$dest"
  curl -fsSL "https://github.com/$repo/archive/$commit.tar.gz" \
    | tar -xz -C "$dest" --strip-components=1
}

# -- yaml-test-suite --------------------------------------------------------

fetch "$YAML_TEST_SUITE_REPO" "$YAML_TEST_SUITE_COMMIT" "$TMP/suite"
OUT="$DATA/yaml-test-suite"
rm -rf "$OUT"
mkdir -p "$OUT/cases"

# only copy the test directories, `name/` and `tags/` are symlink indexes
for dir in "$TMP/suite"/*/; do
  id="$(basename "$dir")"
  case "$id" in
    name|tags) continue ;;
  esac
  cp -R "$dir" "$OUT/cases/$id"
done

# flatten the tag index into a text file: `<id> <tag> <tag> ...`
(
  cd "$TMP/suite/tags"
  for tag in *; do
    for id in "$tag"/*; do
      echo "$(basename "$id") $tag"
    done
  done
) | sort | awk '
  $1 != last { if (line) print line; line = $1; last = $1 }
  { line = line " " $2 }
  END { if (line) print line }
' > "$OUT/tags.txt"

curl -fsSL -o "$OUT/LICENSE" \
  "https://raw.githubusercontent.com/$YAML_TEST_SUITE_REPO/$YAML_TEST_SUITE_RELEASE/License"
cat > "$OUT/SOURCE" <<EOF
https://github.com/$YAML_TEST_SUITE_REPO
commit $YAML_TEST_SUITE_COMMIT (data branch of $YAML_TEST_SUITE_RELEASE)
EOF

# -- yaml-test-schema -------------------------------------------------------

fetch "$YAML_TEST_SCHEMA_REPO" "$YAML_TEST_SCHEMA_COMMIT" "$TMP/schema"
OUT="$DATA/yaml-test-schema"
rm -rf "$OUT"
mkdir -p "$OUT"
cp "$TMP/schema"/data/schema-*.json "$OUT/"
cp "$TMP/schema/LICENSE" "$OUT/LICENSE"
cat > "$OUT/SOURCE" <<EOF
https://github.com/$YAML_TEST_SCHEMA_REPO
commit $YAML_TEST_SCHEMA_COMMIT
EOF

echo "Updated $DATA"
