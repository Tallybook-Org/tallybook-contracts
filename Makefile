.PHONY: build build-price-book build-statement-registry test fmt fmt-check clippy clean

# price-book has no dependencies. statement-registry contractimports its wasm
# (see contracts/statement-registry/src/price_book.rs), so it must be built
# after price-book or the import fails to find
# target/wasm32v1-none/release/price_book.wasm.
build: build-price-book build-statement-registry

build-price-book:
	stellar contract build --package price-book

build-statement-registry: build-price-book
	stellar contract build --package statement-registry

test:
	cargo test --locked --workspace

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

clippy:
	cargo clippy --locked --workspace --all-targets -- -D warnings

clean:
	cargo clean
