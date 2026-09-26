#!/usr/bin/env bash
# Vendors the real world benchmark data into benchmark/data.
#
# The data comes from the benchmarks of the serde based libraries that the
# benchmark compares against:
#
# * json-benchmark (serde_json): twitter.json, canada.json and
#   citm_catalog.json (originally from nativejson-benchmark).
# * toml-rs (toml): the manifests of cargo and web-sys (from wasm-bindgen).
# * serde-saphyr: its Cargo.lock (a large real world TOML document, stored
#   as `Cargo.lock.toml` as lock files are ignored by git).  The
#   YAML benchmark of serde-saphyr is generated, see `src/saphyr.rs`.
#
# Every file is used for all formats: the benchmark converts it into the
# other formats.
#
# To update, change the pinned commits below and re-run the script.
set -euo pipefail

JSON_BENCHMARK_REPO=serde-rs/json-benchmark
JSON_BENCHMARK_COMMIT=17b13dd2d7a5e5fdd5594e847077932f955b5e2b

TOML_REPO=toml-rs/toml
TOML_COMMIT=e4b8bda51c458b3c9b5671b6ad50a0a4a279f583

SAPHYR_REPO=bourumir-wyngs/serde-saphyr
SAPHYR_COMMIT=1.3.0

HERE="$(cd "$(dirname "$0")" && pwd)"
DATA="$HERE/../benchmark/data"

fetch() {
  local repo=$1 commit=$2 path=$3 dest=$4
  curl -fsSL -o "$dest" "https://raw.githubusercontent.com/$repo/$commit/$path"
}

rm -rf "$DATA"
mkdir -p "$DATA/json-benchmark" "$DATA/toml" "$DATA/serde-saphyr"

for file in twitter.json canada.json citm_catalog.json; do
  fetch "$JSON_BENCHMARK_REPO" "$JSON_BENCHMARK_COMMIT" "data/$file" "$DATA/json-benchmark/$file"
done
fetch "$JSON_BENCHMARK_REPO" "$JSON_BENCHMARK_COMMIT" LICENSE-MIT "$DATA/json-benchmark/LICENSE"

for file in Cargo.cargo.toml Cargo.web-sys.toml; do
  fetch "$TOML_REPO" "$TOML_COMMIT" "crates/benchmarks/src/$file" "$DATA/toml/$file"
done
fetch "$TOML_REPO" "$TOML_COMMIT" LICENSE-MIT "$DATA/toml/LICENSE"

fetch "$SAPHYR_REPO" "$SAPHYR_COMMIT" Cargo.lock "$DATA/serde-saphyr/Cargo.lock.toml"
fetch "$SAPHYR_REPO" "$SAPHYR_COMMIT" LICENSE-MIT "$DATA/serde-saphyr/LICENSE"

cat > "$DATA/json-benchmark/SOURCE" <<EOF
https://github.com/$JSON_BENCHMARK_REPO
commit $JSON_BENCHMARK_COMMIT (data/)
EOF
cat > "$DATA/toml/SOURCE" <<EOF
https://github.com/$TOML_REPO
commit $TOML_COMMIT (crates/benchmarks/src/)
EOF
cat > "$DATA/serde-saphyr/SOURCE" <<EOF
https://github.com/$SAPHYR_REPO
tag $SAPHYR_COMMIT
EOF

echo "Updated $DATA"
