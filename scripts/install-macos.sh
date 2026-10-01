#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(CDPATH= cd "$SCRIPT_DIR/.." && pwd)"
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
