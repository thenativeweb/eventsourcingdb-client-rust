qa: analyze test

analyze:
	@cargo clippy --all-features
	@cargo fmt --check
	@cargo doc --all-features --no-deps --document-private-items

test:
	@cargo test --all-features

format:
	@cargo fmt

.PHONY: analyze format qa test
