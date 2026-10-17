.PHONY: build optimize test fmt lint clean deploy check-docs
.PHONY: build optimize test fmt lint clean deploy ci

# Build all contracts to wasm (requires the Stellar CLI).
# Embeds source repo and commit in the wasm metadata (#127).
build:
	@REPO="github:Novatip/novatip-contracts"; \
	COMMIT=$$(git rev-parse --short HEAD 2>/dev/null || echo "unknown"); \
	DIRTY=""; \
	if [ -n "$$(git status --porcelain 2>/dev/null)" ]; then DIRTY="-dirty"; fi; \
	stellar contract build \
		--meta source_repo=$$REPO \
		--meta source_commit=$$COMMIT$$DIRTY

# Optimize the built tip_splitter wasm and print the before/after size.
# Run `make build` first.
optimize:
	@WASM=$$(find target -name 'tip_splitter.wasm' -path '*release*' -not -path '*/deps/*' \
		-printf '%T@ %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-); \
	if [ -z "$$WASM" ] || [ ! -f "$$WASM" ]; then \
		echo "error: no built tip_splitter.wasm found under target/. Run 'make build' first." >&2; \
		exit 1; \
	fi; \
	BEFORE=$$(wc -c < "$$WASM"); \
	stellar contract optimize --wasm "$$WASM"; \
	OPTIMIZED="$${WASM%.wasm}.optimized.wasm"; \
	AFTER=$$(wc -c < "$$OPTIMIZED"); \
	echo "==> $$OPTIMIZED: $$BEFORE -> $$AFTER bytes ($$((BEFORE - AFTER)) saved)"

# Run the test suite.
test:
	cargo test --locked

# Auto-format all code.
fmt:
	cargo fmt --all

# Lint and fail on any warning (matches CI).
lint:
	cargo clippy --locked --all-targets -- -D warnings

# Remove build artifacts.
clean:
	cargo clean

# Deploy to the configured network (see scripts/deploy.sh).
deploy:
	./scripts/deploy.sh

# Build the contract and fail if docs/CONTRACT.md's function or error tables
# no longer match the built interface (requires the Stellar CLI and jq).
check-docs:
	./scripts/check-docs.sh
# Run everything CI checks, in the same order with the same flags,
# including the wasm build and size check.
ci:
	cargo fmt --all -- --check
	cargo clippy --locked --all-targets -- -D warnings
	cargo test --locked
	cargo build --locked --target wasm32-unknown-unknown --release
	./scripts/check-wasm-size.sh
