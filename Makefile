qa: analyze test

analyze: format-check
	@cargo clippy

test:
	@cargo test

format:
	@cargo fmt

format-check:
	@cargo fmt --check

.PHONY: analyze qa test
