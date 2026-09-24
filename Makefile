all: test

test:
	@cargo test

MIRI_CRATES := deser deser-json deser-path deser-debug

miri-test:
	@for crate in $(MIRI_CRATES); do \
		(cd $$crate && MIRIFLAGS="-Zmiri-strict-provenance" cargo +nightly miri test --all-features) || exit 1; \
		(cd $$crate && MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo +nightly miri test --all-features) || exit 1; \
	done

check:
	@cargo check --all-features

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
	@cargo clippy

bench:
	@cd benchmark; RUSTC_BOOTSTRAP=1 cargo bench

bench-compile-times:
	@cd compile-times/; ./bench.sh

.PHONY: all test miri-test check doc format format-check lint bench bench-compile-times
