all: test

test:
	@cargo test

miri-test:
	cd deser; MIRIFLAGS=-Zmiri-tag-raw-pointers cargo +nightly miri test

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

.PHONY: all test miri-test check doc format format-check lint
