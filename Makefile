qa: analyze test

analyze:
	@cargo clippy
	@cargo fmt --check

test:
	@cargo test

format:
	@cargo fmt

.PHONY: analyze format qa test
