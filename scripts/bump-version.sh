#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd $SCRIPT_DIR/..

NEW_VERSION="${1}"

echo "Bumping version: ${NEW_VERSION}"

# only the published crates, the benchmark, the examples and the template
# of the JSON parsers keep their versions
for path in deser*/Cargo.toml; do
  [ "$path" = deser-template-json/Cargo.toml ] && continue
  perl -pi -e "s/^(deser.*)?version = \".*?\"/\$1version = \"$NEW_VERSION\"/" $path
done
