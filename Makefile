all: test

test:
	@cargo test

miri-test:
	cd deser; MIRIFLAGS=-Zmiri-tag-raw-pointers cargo +nightly miri test --all-features

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
