#!/bin/sh
# Compares the compile times and binary sizes of serde, miniserde and
# deser.
#
# 1. Clean builds of a small program (`LIB-version`), including all
#    dependencies.  The best of three runs is reported.  deser is used
#    through path dependencies which cargo compiles incrementally (unlike
#    crates from crates.io), so these builds are not incremental.
# 2. Builds of a library with 100 structs and 100 enums (generated into
#    `target/many`), without the dependencies.  This is the cost of the
#    derived code.  It's a library as in a binary only the code that is
#    used is compiled.  The best of three runs is reported.
# 3. The sizes of the stripped binaries of the small program and of a
#    program that reads and writes the 100 structs of the library, built
#    with the default release profile and one optimized for size (see
#    `SMALL`).  The binaries are generated into `target/size`.
#
# `./bench.sh compile` only measures the compile times, `./bench.sh sizes`
# only the binary sizes.
set -e
cd "$(dirname "$0")"

LIBS="serde miniserde deser"
WHAT=${1:-all}

# the profile optimized for size (on top of the release profile)
SMALL="CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_OPT_LEVEL=s CARGO_PROFILE_RELEASE_PANIC=abort"

# Prints the best of three runs of a command in a directory.  `prepare`
# runs before every run, `incremental` is the value of `CARGO_INCREMENTAL`
# (if given).
best_of_three() {
  dir=$1; cmd=$2; prepare=$3; incremental=$4
  best=
  for _ in 1 2 3; do
    (cd $dir; eval "$prepare")
    t=$( { /usr/bin/time -p sh -c "cd $dir && ${incremental:+CARGO_INCREMENTAL=$incremental} cargo $cmd -q"; } 2>&1 | awk '/^real/ { print $2 }')
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
    best_of_three $lib-version "$cmd" "rm -rf target" 0
  done
}

# Copies the manifest of `LIB-version` into a directory two levels below
# `target` with a new name.
copy_manifest() {
  lib=$1; dir=$2; name=$3
  mkdir -p $dir/src
  sed -e "s/^name = .*/name = \"$name\"/" \
    -e 's|path = "\.\./\.\./|path = "../../../../|' \
    $lib-version/Cargo.toml > $dir/Cargo.toml
  cp $lib-version/Cargo.lock $dir/Cargo.lock
}

# Generates a crate with 100 structs and enums for a library.  With `bin`
# as the second argument it's a program, otherwise a library in
# `target/many`.
generate_many() {
  lib=$1
  if [ "$2" = bin ]; then
    dir=target/size/many-$lib; file=main.rs
  else
    dir=target/many/$lib; file=lib.rs
  fi
  copy_manifest $lib $dir many-$lib
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
    echo "pub fn run() {"
    echo "    let input = std::env::args().nth(1).unwrap_or_default();"
    i=0
    while [ $i -lt 100 ]; do
      echo "    if let Ok(value) = $de::<Struct$i>(&input) { println!(\"{}\", $ser(&value)$unwrap); }"
      i=$((i + 1))
    done
    echo "}"
    if [ $file = main.rs ]; then
      echo "fn main() { run(); }"
    fi
  } > $dir/src/$file
}

# Prints the best of three builds of the generated program (without the
# dependencies).
many_builds() {
  lib=$1
  dir=target/many/$lib
  echo "$lib"
  for cmd in "check" "build" "build --release"; do
    (cd $dir; cargo $cmd -q)
    best_of_three $dir "$cmd" "touch src/lib.rs"
  done
}

# Prints the size of the stripped binary of a crate in bytes, built with
# a profile (`release` or `small`).
binary_size() {
  dir=$1; profile=$2
  name=$(awk -F '"' '/^name = / { print $2; exit }' $dir/Cargo.toml)
  env CARGO_PROFILE_RELEASE_STRIP=symbols $([ $profile = small ] && echo $SMALL) \
    cargo build -q --release --manifest-path $dir/Cargo.toml --target-dir $dir/target/$profile
  wc -c < $dir/target/$profile/release/$name | tr -d ' '
}

# Prints the sizes of a binary in KiB with both profiles, and how much
# larger they are than hello world.
size_row() {
  label=$1; dir=$2
  release=$(binary_size $dir release)
  small=$(binary_size $dir small)
  printf "  %-32s %6d KiB (+%4d)  %6d KiB (+%4d)\n" "$label" \
    $((release / 1024)) $(((release - hello_release) / 1024)) \
    $((small / 1024)) $(((small - hello_small) / 1024))
}

# Turns off the zmij feature of deser-json (the float formatting falls
# back to the standard library) in a copy of a deser crate.
without_zmij() {
  rm -rf $2
  mkdir -p $2
  cp -R $1/Cargo.toml $1/Cargo.lock $1/src $2/
  sed -i.bak -e "s/^name = \"\(.*\)\"/name = \"\1-no-zmij\"/" \
    -e 's|^\(deser-json = { path = "[^"]*"\) }|\1, default-features = false, features = ["std"] }|' $2/Cargo.toml
  rm $2/Cargo.toml.bak
}

binary_sizes() {
  mkdir -p target/size/hello/src
  printf '[package]\nname = "hello"\nversion = "0.1.0"\nedition = "2024"\n\n[workspace]\n' \
    > target/size/hello/Cargo.toml
  echo 'fn main() { println!("Hello, world!"); }' > target/size/hello/src/main.rs
  hello_release=$(binary_size target/size/hello release)
  hello_small=$(binary_size target/size/hello small)

  echo "binary sizes, stripped (in parentheses: KiB more than hello world)"
  printf "  %-32s %-19s %s\n" "" "   release" "   size optimized"
  printf "  %-32s %6d KiB          %6d KiB\n" "hello world" \
    $((hello_release / 1024)) $((hello_small / 1024))
  for lib in $LIBS; do
    copy_manifest $lib target/size/one-$lib one-$lib
    cp $lib-version/src/main.rs target/size/one-$lib/src/main.rs
    size_row "$lib" target/size/one-$lib
  done
  without_zmij target/size/one-deser target/size/one-deser-no-zmij
  size_row "deser (without zmij)" target/size/one-deser-no-zmij
  for lib in $LIBS; do
    generate_many $lib bin
    size_row "$lib, 100 types" target/size/many-$lib
  done
  without_zmij target/size/many-deser target/size/many-deser-no-zmij
  size_row "deser (without zmij), 100 types" target/size/many-deser-no-zmij
}

if [ "$WHAT" != sizes ]; then
  echo "clean builds (best of three)"
  for lib in $LIBS; do
    clean_builds $lib
  done

  echo
  echo "library with 100 structs and enums, without dependencies (best of three)"
  for lib in $LIBS; do
    generate_many $lib
    many_builds $lib
  done
fi

if [ "$WHAT" != compile ]; then
  [ "$WHAT" = sizes ] || echo
  binary_sizes
fi
