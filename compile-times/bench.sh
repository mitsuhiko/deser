#!/bin/sh
# Compares the compile times of serde, miniserde and deser.
#
# 1. Clean builds of a small program (`LIB-version`), including all
#    dependencies.  The best of three runs is reported.
# 2. Builds of a program with 100 structs and 100 enums (generated into
#    `target/many`), without the dependencies.  This is the cost of the
#    derived code.  The best of three runs is reported.
set -e
cd "$(dirname "$0")"

LIBS="serde miniserde deser"

# Prints the best of three runs of a command in a directory.  `prepare`
# runs before every run.
best_of_three() {
  dir=$1; cmd=$2; prepare=$3
  best=
  for _ in 1 2 3; do
    (cd $dir; eval "$prepare")
    t=$( { /usr/bin/time -p sh -c "cd $dir && cargo $cmd -q"; } 2>&1 | awk '/^real/ { print $2 }')
    if [ -z "$best" ] || [ "$(echo "$t < $best" | bc)" = 1 ]; then
      best=$t
    fi
  done
  printf "  %-16s %6.2fs\n" "$cmd" "$best"
}

# Prints the best of three clean builds (with all dependencies).
clean_builds() {
  lib=$1
  echo "$lib"
  for cmd in "check" "build" "build --release"; do
    best_of_three $lib-version "$cmd" "rm -rf target"
  done
}

# Generates a program with 100 structs and enums for a library.
generate_many() {
  lib=$1
  dir=target/many/$lib
  mkdir -p $dir/src
  sed -e "s/^name = .*/name = \"many-$lib\"/" \
    -e 's|path = "\.\./\.\./|path = "../../../../|' \
    $lib-version/Cargo.toml > $dir/Cargo.toml
  cp $lib-version/Cargo.lock $dir/Cargo.lock
  case $lib in
    serde)
      attr='#[serde(rename_all = "camelCase")]'
      enum_attr='#[serde(rename_all = "snake_case")]'
      de=serde_json::from_str; ser=serde_json::to_string; unwrap=.unwrap\(\) ;;
    deser)
      attr='#[deser(rename_all = "camelCase")]'
      enum_attr='#[deser(rename_all = "snake_case")]'
      de=deser_json::from_str; ser=deser_json::to_string; unwrap=.unwrap\(\) ;;
    miniserde)
      attr=''; enum_attr=''
      de=miniserde::json::from_str; ser=miniserde::json::to_string; unwrap='' ;;
  esac
  {
    echo "use $lib::{Deserialize, Serialize};"
    i=0
    while [ $i -lt 100 ]; do
      echo "#[derive(Serialize, Deserialize)]"
      echo "$enum_attr"
      echo "pub enum Kind$i { First, SecondKind, Third }"
      echo "#[derive(Serialize, Deserialize)]"
      echo "$attr"
      echo "pub struct Struct$i {"
      echo "    id: u64, user_name: String, is_enabled: bool, score: f64,"
      echo "    kind: Kind$i, tags: Vec<String>, maybe_count: Option<u32>,"
      # nested structs, at most ten levels deep
      [ $i -gt 0 ] && echo "    nested_value: Option<Box<Struct$(( (i - 1) % 9 ))>>,"
      echo "}"
      i=$((i + 1))
    done
    echo "fn main() {"
    echo "    let input = std::env::args().nth(1).unwrap_or_default();"
    i=0
    while [ $i -lt 100 ]; do
      echo "    if let Ok(value) = $de::<Struct$i>(&input) { println!(\"{}\", $ser(&value)$unwrap); }"
      i=$((i + 1))
    done
    echo "}"
  } > $dir/src/main.rs
}

# Prints the best of three builds of the generated program (without the
# dependencies).
many_builds() {
  lib=$1
  dir=target/many/$lib
  echo "$lib"
  for cmd in "check" "build" "build --release"; do
    (cd $dir; cargo $cmd -q)
    best_of_three $dir "$cmd" "touch src/main.rs"
  done
}

echo "clean builds (best of three)"
for lib in $LIBS; do
  clean_builds $lib
done

echo
echo "100 structs and enums, without dependencies (best of three)"
for lib in $LIBS; do
  generate_many $lib
  many_builds $lib
done
