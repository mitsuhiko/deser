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

# In CI the runner streams the output where cargo's progress bar is noise.
ifeq ($(CI),true)
export CARGO_TERM_PROGRESS_WHEN := never
endif

# Crates tested with miri (with stacked borrows), ordered by how long they
# take, the slowest start first.  CI splits them across jobs.
MIRI_CRATES ?= deser deser-json deser-cbor deser-path deser-location deser-debug
# Crates also tested with tree borrows.  Almost all unsafe code is in the
# core crate, the formats only have simple byte copies.
MIRI_TREE_BORROWS_CRATES ?= deser
# every miri run is single threaded
MIRI_JOBS ?= 6
# Tests that are slow in miri and do not test unsafe code opt out with
# `#[cfg_attr(miri, ignore = "slow, no unsafe code under test")]`.  Tests
# with unsafe code under test should rather do less work in miri (see the
# uses of `cfg!(miri)`).  `make miri-test-full` also runs the ignored tests.
MIRI_TEST_ARGS ?=

# keep in sync with `rust-version` in Cargo.toml
MSRV := 1.88

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
	@$(RUN) "check:no-default-features" "cargo check -p deser -p deser-json -p deser-cbor -p deser-yaml -p deser-toml --all-targets --no-default-features"

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
		"fmt:benchmark" "cd benchmark && cargo fmt --all -- --check" \
		$(foreach ws,$(EXTRA_WORKSPACES),"fmt:$(notdir $(ws))" "cd $(ws) && cargo fmt --all -- --check")

lint:
	@rustup component add clippy > /dev/null 2>&1
	@$(RUN) -j 5 \
		"clippy" "cargo clippy --workspace --all-targets --all-features -- -D warnings" \
		"clippy:benchmark" "cd benchmark && RUSTC_BOOTSTRAP=1 cargo clippy --all-targets --all-features -- -D warnings" \
		$(foreach ws,$(EXTRA_WORKSPACES),"clippy:$(notdir $(ws))" "cd $(ws) && cargo clippy --all-targets -- -D warnings")

bench:
	@$(RUN) "bench" --show-on-output "cd benchmark && RUSTC_BOOTSTRAP=1 cargo bench"

bench-compile-times:
	@$(RUN) "bench-compile-times" --show-on-output "cd compile-times && ./bench.sh"

.PHONY: all test miri-test miri-test-full check msrv doc format format-check lint bench bench-compile-times
