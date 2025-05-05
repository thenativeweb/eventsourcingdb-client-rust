qa: analyze test

analyze:
	@cargo clippy

test:
	@cargo test

.PHONY: analyze qa test
