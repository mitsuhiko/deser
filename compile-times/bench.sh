#!/bin/sh
echo serde
echo "  check"
rm -rf serde-version/target
(cd serde-version; cargo check 2>&1 | grep Finished)
echo "  build"
rm -rf serde-version/target
(cd serde-version; cargo build 2>&1 | grep Finished)
echo "  build --release"
rm -rf serde-version/target
(cd serde-version; cargo build --release 2>&1 | grep Finished)

echo

echo miniserde
echo "  check"
rm -rf miniserde-version/target
(cd miniserde-version; cargo check 2>&1 | grep Finished)
echo "  build"
rm -rf miniserde-version/target
(cd miniserde-version; cargo build 2>&1 | grep Finished)
echo "  build --release"
rm -rf miniserde-version/target
(cd miniserde-version; cargo build --release 2>&1 | grep Finished)

echo

echo deser
echo "  check"
rm -rf deser-version/target
(cd deser-version; cargo check 2>&1 | grep Finished)
echo "  build"
rm -rf deser-version/target
(cd deser-version; cargo build 2>&1 | grep Finished)
echo "  build --release"
rm -rf deser-version/target
(cd deser-version; cargo build --release 2>&1 | grep Finished)
