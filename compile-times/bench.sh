#!/bin/sh
echo Serde
echo "  check"
rm -rf serde-version/target
(cd serde-version; cargo check 2>&1 | grep Finished)
echo "  build"
rm -rf serde-version/target
(cd serde-version; cargo build 2>&1 | grep Finished)
echo "  build --release"
rm -rf serde-version/target
(cd serde-version; cargo build --release 2>&1 | grep Finished)

echo Deser
echo "  check"
rm -rf deser-version/target
(cd deser-version; cargo check 2>&1 | grep Finished)
echo "  build"
rm -rf deser-version/target
(cd deser-version; cargo build 2>&1 | grep Finished)
echo "  build --release"
rm -rf deser-version/target
(cd deser-version; cargo build --release 2>&1 | grep Finished)
