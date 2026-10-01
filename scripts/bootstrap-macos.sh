#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(CDPATH= cd "$SCRIPT_DIR/.." && pwd)"

command -v xcode-select >/dev/null 2>&1 || { echo "Xcode Command Line Tools are required" >&2; exit 1; }
xcode-select -p >/dev/null 2>&1 || { echo "Run: xcode-select --install" >&2; exit 1; }
command -v rustup >/dev/null 2>&1 || { echo "Install Rust from https://rustup.rs" >&2; exit 1; }
command -v node >/dev/null 2>&1 || { echo "Node.js 22+ is required" >&2; exit 1; }

node_major="$(node -p 'process.versions.node.split(".")[0]')"
[ "$node_major" -ge 22 ] || { echo "Node.js 22+ is required" >&2; exit 1; }

rustup toolchain install 1.96.0 --profile minimal --component rustfmt --component clippy
if ! command -v pnpm >/dev/null 2>&1; then
  command -v corepack >/dev/null 2>&1 || { echo "Install pnpm 11.10.0 or Corepack" >&2; exit 1; }
  corepack enable
fi
pnpm_major="$(pnpm --version | cut -d. -f1)"
[ "$pnpm_major" = 11 ] || { echo "pnpm 11 is required" >&2; exit 1; }

cd "$ROOT_DIR"
pnpm install

echo "Ready. Run: pnpm dev"
