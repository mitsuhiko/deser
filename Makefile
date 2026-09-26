all: test

test:
	@cargo test

MIRI_CRATES := deser deser-cbor deser-json deser-location deser-path deser-debug

# keep in sync with `rust-version` in Cargo.toml
MSRV := 1.88

# standalone workspaces that are not part of the main workspace
EXTRA_WORKSPACES := compile-times/deser-version compile-times/serde-version compile-times/miniserde-version

miri-test:
	@for crate in $(MIRI_CRATES); do \
		(cd $$crate && MIRIFLAGS="-Zmiri-strict-provenance" cargo +nightly miri test --all-features) || exit 1; \
		(cd $$crate && MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo +nightly miri test --all-features) || exit 1; \
	done

check:
	@cargo check --all-features

msrv:
	@rustup toolchain install $(MSRV) --profile minimal 2> /dev/null
	@cargo +$(MSRV) test --workspace --all-features

doc:
	@cargo doc --all-features

format:
	@rustup component add rustfmt 2> /dev/null
	@cargo fmt --all

format-check:
	@rustup component add rustfmt 2> /dev/null
	@cargo fmt --all -- --check

lint:
	@rustup component add clippy 2> /dev/null
	@cargo clippy --workspace --all-targets --all-features -- -D warnings
	@cd benchmark && RUSTC_BOOTSTRAP=1 cargo clippy --all-targets --all-features -- -D warnings
	@for ws in $(EXTRA_WORKSPACES); do \
		(cd $$ws && cargo clippy --all-targets -- -D warnings) || exit 1; \
	done

bench:
	@cd benchmark; RUSTC_BOOTSTRAP=1 cargo bench

bench-compile-times:
	@cd compile-times/; ./bench.sh

.PHONY: all test miri-test check msrv doc format format-check lint bench bench-compile-times
