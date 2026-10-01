#!/bin/sh
set -eu

INSTALL_DIR="${INSTALL_DIR:-$HOME/Applications}"
APP_NAME="Lead Aggregator"
APP_DEST="$INSTALL_DIR/$APP_NAME.app"

SCRIPT_DIR=""
if [ -f "$0" ]; then
  candidate="$(CDPATH= cd "$(dirname "$0")" && pwd)"
  if [ -f "$candidate/../Cargo.toml" ]; then
    SCRIPT_DIR="$candidate"
  fi
fi

if [ -n "$SCRIPT_DIR" ]; then
  ROOT_DIR="$(CDPATH= cd "$SCRIPT_DIR/.." && pwd)"
  "$SCRIPT_DIR/bootstrap-macos.sh"
  cd "$ROOT_DIR"
  pnpm --dir apps/desktop tauri build --bundles app
  APP_SOURCE="$ROOT_DIR/target/release/bundle/macos/$APP_NAME.app"
  if [ ! -d "$APP_SOURCE" ]; then
    echo "Build completed, but app bundle was not found: $APP_SOURCE" >&2
    exit 1
  fi
  mkdir -p "$INSTALL_DIR"
  ditto "$APP_SOURCE" "$APP_DEST"
  printf 'Installed: %s\n' "$APP_DEST"
  exit 0
fi

command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }
command -v hdiutil >/dev/null 2>&1 || { echo "hdiutil is required" >&2; exit 1; }
command -v ditto >/dev/null 2>&1 || { echo "ditto is required" >&2; exit 1; }
TEMP_ROOT="$(mktemp -d)"
MOUNT_POINT="$TEMP_ROOT/mount"
mkdir -p "$MOUNT_POINT"
cleanup() {
  hdiutil detach "$MOUNT_POINT" -quiet >/dev/null 2>&1 || true
  rm -rf "$TEMP_ROOT"
}
trap cleanup EXIT HUP INT TERM

case "$(uname -m)" in
  arm64|aarch64) ARCH_TAG=aarch64 ;;
  x86_64) ARCH_TAG=x86_64 ;;
  *) echo "Unsupported Mac architecture: $(uname -m)" >&2; exit 1 ;;
esac

RELEASE_JSON="$(curl --fail --location --silent --show-error \
  -H 'Accept: application/vnd.github+json' \
  https://api.github.com/repos/1tuz/Lead-Aggregator/releases/latest)"
DMG_URL="$(printf '%s\n' "$RELEASE_JSON" \
  | grep -Eo '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+\.dmg"' \
  | grep "_${ARCH_TAG}\\.dmg\"" \
  | sed -E 's/^[^:]+:[[:space:]]*"([^"]+)"$/\1/' \
  | head -n 1)"
if [ -z "$DMG_URL" ]; then
  echo "No macOS $ARCH_TAG installer found in the latest GitHub release." >&2
  echo "See https://github.com/1tuz/Lead-Aggregator/releases" >&2
  exit 1
fi

curl --fail --location --silent --show-error "$DMG_URL" -o "$TEMP_ROOT/installer.dmg"
hdiutil attach "$TEMP_ROOT/installer.dmg" -nobrowse -readonly -mountpoint "$MOUNT_POINT" >/dev/null
APP_SOURCE="$MOUNT_POINT/$APP_NAME.app"
if [ ! -d "$APP_SOURCE" ]; then
  echo "The downloaded disk image does not contain $APP_NAME.app" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
ditto "$APP_SOURCE" "$APP_DEST"
printf 'Installed: %s\n' "$APP_DEST"
