#!/bin/bash
# Publishes the crates of the workspace to crates.io.
#
# Crates whose current version is already on crates.io are skipped so that
# a release that failed half way (for instance because of the rate limit of
# crates.io for new crates) can be continued by running this again.  Extra
# arguments are passed to `cargo publish` (for instance `--dry-run`).
set -euo pipefail

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR/.."

# path of a crate in the sparse index of crates.io
index_path() {
  local name
  name=$(echo "$1" | tr '[:upper:]' '[:lower:]')
  case ${#name} in
    1) echo "1/$name" ;;
    2) echo "2/$name" ;;
    3) echo "3/${name:0:1}/$name" ;;
    *) echo "${name:0:2}/${name:2:2}/$name" ;;
  esac
}

is_published() {
  local index
  index=$(curl -sSf "https://index.crates.io/$(index_path "$1")" 2>/dev/null) || return 1
  echo "$index" | jq -e --arg vers "$2" 'select(.vers == $vers)' > /dev/null
}

excludes=()
pending=0
while read -r name version; do
  if is_published "$name" "$version"; then
    echo "Skipping $name $version (already published)"
    excludes+=(--exclude "$name")
  else
    echo "Publishing $name $version"
    pending=$((pending + 1))
  fi
done < <(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.publish != []) | "\(.name) \(.version)"')

if [ "$pending" -eq 0 ]; then
  echo "Nothing to publish"
  exit 0
fi

cargo publish --workspace ${excludes[@]+"${excludes[@]}"} "$@"
