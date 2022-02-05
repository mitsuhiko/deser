#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd $SCRIPT_DIR/..

NEW_VERSION="${1}"

echo "Bumping version: ${NEW_VERSION}"

for path in */Cargo.toml; do
  perl -pi -e "s/^(deser.*)?version = \".*?\"/\$1version = \"$NEW_VERSION\"/" $path
done
