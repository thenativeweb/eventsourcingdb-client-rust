qa: analyze test

analyze:
	@cargo clippy
	@cargo fmt --check
	@cargo doc --all-features --no-deps --document-private-items

test:
	@cargo test

format:
	@cargo fmt

.PHONY: analyze format qa test
