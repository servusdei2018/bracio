.PHONY: check
check:
	cargo check
	uv run ruff check python/bracio python/tests

.PHONY: dev
dev:
	uvx maturin dev

.PHONY: format
format:
	cargo fmt
	uv run ruff format

.PHONY: test
test:
	uv run pytest