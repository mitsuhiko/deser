# All targets run through the utility runner.  It shows a status line with a
# spinner per task and only prints the output of tasks that fail.
#
# What can run in parallel:
#
# * Tasks in different cargo workspaces (the main one, `benchmark` and the
#   ones in `compile-times`) have their own target directories and run fully
#   in parallel.
# * Tasks in the same workspace share the target directory.  Cargo only
#   locks it while building, so their builds are serialized but running tests
#   (for instance unit tests next to doctests, or the miri tests) overlaps.
# * Benchmarks never run in parallel with anything as that would skew the
#   numbers.
RUN := ./scripts/utility-runner
comma := ,
empty :=
space := $(empty) $(empty)

# In CI the runner streams the output where cargo's progress bar is noise.
ifeq ($(CI),true)
export CARGO_TERM_PROGRESS_WHEN := never
endif

# Crates tested with miri (with stacked borrows), ordered by how long they
# take, the slowest start first.  CI splits them across jobs.  The parsers
# of deser-json, deser-jsonc, deser-json5 and deser-hjson are tested by
# deser-template-json.
MIRI_CRATES ?= deser-core deser deser-template-json deser-json deser-cbor deser-msgpack deser-csv deser-xml deser-path deser-location deser-debug
# Crates also tested with tree borrows.  Almost all unsafe code is in the
# core crate (tested by its own tests and the integration tests of deser),
# the formats only have simple byte copies.
MIRI_TREE_BORROWS_CRATES ?= deser-core deser
# every miri run is single threaded
MIRI_JOBS ?= 6
# Tests that are slow in miri and do not test unsafe code opt out with
# `#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]`.  Tests
# with unsafe code under test should rather do less work in miri (see the
# uses of `cfg!(miri)`).  `make miri-test-full` also runs the ignored tests.
MIRI_TEST_ARGS ?=

# keep in sync with `rust-version` in Cargo.toml
MSRV := 1.88

# the targets the crates that support `no_std` are built for (targets
# without the standard library).  The 32 bit target checks the sizes of
# the types, the 64 bit one the SIMD code of the speedups.
NO_STD_TARGET := thumbv7em-none-eabihf
NO_STD_TARGET_64 := aarch64-unknown-none
# crates that support `no_std` (their `std` feature is off), the no-std
# example uses the derive
NO_STD_CRATES := deser deser-core deser-cbor deser-csv deser-json deser-jsonc deser-json5 deser-hjson deser-msgpack deser-plist deser-path deser-debug deser-transcode no-std
# the features of deser-core that work without `std`
NO_STD_FEATURES := derive,arrayvec,bigdecimal,bstr,bytes,chrono,hashbrown,indexmap,jiff,num-bigint,rust_decimal,smallvec,time,uuid
# the crates with speedups that work without `std`
NO_STD_SPEEDUPS := deser-cbor deser-json deser-jsonc deser-json5 deser-hjson deser-msgpack
NO_STD_SPEEDUPS_FEATURES := $(subst $(space),$(comma),$(foreach crate,$(NO_STD_SPEEDUPS),$(crate)/speedups))

# standalone workspaces that are not part of the main workspace
EXTRA_WORKSPACES := compile-times/deser-version compile-times/serde-version compile-times/miniserde-version

all: test

test:
	@$(RUN) -j 2 \
		"test" "cargo test --workspace --all-features --tests" \
		"doctest" "cargo test --workspace --all-features --doc"

miri-test:
	@$(RUN) "miri:setup" "cargo +nightly miri setup"
	@$(RUN) -j $(MIRI_JOBS) \
		$(foreach crate,$(MIRI_TREE_BORROWS_CRATES), \
			"miri:$(crate):tree-borrows" "cd $(crate) && MIRIFLAGS='-Zmiri-strict-provenance -Zmiri-tree-borrows' cargo +nightly miri test --all-features -- $(MIRI_TEST_ARGS)") \
		$(foreach crate,$(MIRI_CRATES), \
			"miri:$(crate)" "cd $(crate) && MIRIFLAGS='-Zmiri-strict-provenance' cargo +nightly miri test --all-features -- $(MIRI_TEST_ARGS)")

miri-test-full:
	@$(MAKE) --no-print-directory miri-test MIRI_TEST_ARGS=--include-ignored

check:
	@$(RUN) "check" "cargo check --workspace --all-targets --all-features"
	@$(RUN) "check:no-default-features" "cargo check -p deser -p deser-core -p deser-json -p deser-jsonc -p deser-json5 -p deser-hjson -p deser-cbor -p deser-msgpack -p deser-yaml -p deser-toml -p deser-urlencoded -p deser-csv -p deser-plist -p deser-xml --all-targets --no-default-features"

# builds without the standard library for a target that does not have one
check-no-std:
	@$(RUN) "check-no-std:setup" "rustup target add $(NO_STD_TARGET) $(NO_STD_TARGET_64)"
	@$(RUN) \
		"check-no-std" "cargo build --target $(NO_STD_TARGET) --no-default-features --lib $(foreach crate,$(NO_STD_CRATES),-p $(crate))" \
		"check-no-std:features" "cargo build --target $(NO_STD_TARGET) --no-default-features --lib -p deser-core $(foreach crate,$(NO_STD_SPEEDUPS),-p $(crate)) --features deser-core/$(subst $(comma),$(comma)deser-core/,$(NO_STD_FEATURES)),$(NO_STD_SPEEDUPS_FEATURES)" \
		"check-no-std:64" "cargo build --target $(NO_STD_TARGET_64) --no-default-features --lib -p deser-core $(foreach crate,$(NO_STD_SPEEDUPS),-p $(crate)) --features deser-core/derive,$(NO_STD_SPEEDUPS_FEATURES)"

# uses its own target directory so it does not invalidate the regular builds
msrv:
	@$(RUN) "msrv" "rustup toolchain install $(MSRV) --profile minimal && CARGO_TARGET_DIR=target/msrv cargo +$(MSRV) test --workspace --all-features"

doc:
	@$(RUN) "doc" "cargo doc --all-features"

format:
	@rustup component add rustfmt > /dev/null 2>&1
	@$(RUN) -j 5 \
		"fmt" "cargo fmt --all" \
		"fmt:benchmark" "cd benchmark && cargo fmt --all" \
		$(foreach ws,$(EXTRA_WORKSPACES),"fmt:$(notdir $(ws))" "cd $(ws) && cargo fmt --all")

format-check:
	@rustup component add rustfmt > /dev/null 2>&1
	@$(RUN) -j 5 \
		"fmt" "cargo fmt --all -- --check" \
		"codegen" "python3 deser-template-json/generate.py --check" \
		"fmt:benchmark" "cd benchmark && cargo fmt --all -- --check" \
		$(foreach ws,$(EXTRA_WORKSPACES),"fmt:$(notdir $(ws))" "cd $(ws) && cargo fmt --all -- --check")

lint:
	@rustup component add clippy > /dev/null 2>&1
	@$(RUN) -j 5 \
		"clippy" "cargo clippy --workspace --all-targets --all-features -- -D warnings" \
		"clippy:benchmark" "cd benchmark && RUSTC_BOOTSTRAP=1 cargo clippy --all-targets --all-features -- -D warnings" \
		$(foreach ws,$(EXTRA_WORKSPACES),"clippy:$(notdir $(ws))" "cd $(ws) && cargo clippy --all-targets -- -D warnings")

# regenerates the parsers of deser-json, deser-jsonc, deser-json5 and
# deser-hjson from
# deser-template-json
codegen:
	@$(RUN) "codegen" --show-on-output "python3 deser-template-json/generate.py"

bench:
	@$(RUN) "bench" --show-on-output "cd benchmark && RUSTC_BOOTSTRAP=1 cargo bench"

bench-versus:
	@$(RUN) "bench-versus" --show-on-output "cd benchmark && cargo run --release -- versus"

bench-compile-times:
	@$(RUN) "bench-compile-times" --show-on-output "cd compile-times && ./bench.sh"

.PHONY: all test miri-test miri-test-full check check-no-std msrv doc format format-check lint codegen bench bench-versus bench-compile-times
