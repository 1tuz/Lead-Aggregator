#!/bin/sh
set -eu

SCRIPT_DIR=""
if [ -f "$0" ]; then
  candidate="$(CDPATH= cd "$(dirname "$0")" && pwd)"
  if [ -f "$candidate/../Cargo.toml" ]; then
    SCRIPT_DIR="$candidate"
  fi
fi

if [ -n "$SCRIPT_DIR" ]; then
  ROOT_DIR="$(CDPATH= cd "$SCRIPT_DIR/.." && pwd)"
else
  command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }
  command -v tar >/dev/null 2>&1 || { echo "tar is required" >&2; exit 1; }
  TEMP_ROOT="$(mktemp -d)"
  trap 'rm -rf "$TEMP_ROOT"' EXIT HUP INT TERM
  curl --fail --location --silent --show-error \
    https://github.com/1tuz/Lead-Aggregator/archive/refs/heads/main.tar.gz \
    -o "$TEMP_ROOT/source.tar.gz"
  tar -xzf "$TEMP_ROOT/source.tar.gz" --strip-components=1 -C "$TEMP_ROOT"
  SCRIPT_DIR="$TEMP_ROOT/scripts"
  ROOT_DIR="$TEMP_ROOT"
fi
INSTALL_DIR="${INSTALL_DIR:-$HOME/Applications}"
APP_NAME="Lead Aggregator"
APP_SOURCE="$ROOT_DIR/target/release/bundle/macos/$APP_NAME.app"
APP_DEST="$INSTALL_DIR/$APP_NAME.app"

"$SCRIPT_DIR/bootstrap-macos.sh"

cd "$ROOT_DIR"
pnpm --dir apps/desktop tauri build --bundles app

if [ ! -d "$APP_SOURCE" ]; then
  echo "Build completed, but app bundle was not found: $APP_SOURCE" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
ditto "$APP_SOURCE" "$APP_DEST"
printf 'Installed: %s\n' "$APP_DEST"
