.PHONY: setup dev fmt check test build

setup:
	./scripts/bootstrap-macos.sh


dev:
	pnpm dev

fmt:
	cargo fmt --all
	pnpm format:ui

check:
	cargo fmt --all -- --check
	cargo check --workspace
	pnpm check:ui

test:
	cargo test --workspace

build:
	pnpm build
