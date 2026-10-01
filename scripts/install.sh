#!/usr/bin/env bash
set -euo pipefail

REPOSITORY="1tuz/Lead-Aggregator"
INSTALL_DIR="${HOME}/Applications"
APP_NAME="Lead Aggregator"
TEMP_DIR="$(mktemp -d)"
MOUNT_POINT="${TEMP_DIR}/mount"
MOUNTED=0

cleanup() {
  if [[ "$MOUNTED" == 1 ]]; then
    hdiutil detach "$MOUNT_POINT" -quiet >/dev/null 2>&1 || true
  fi
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }

case "$(uname -s)" in
  Darwin)
    case "$(uname -m)" in
      arm64|aarch64) ASSET_SUFFIX="_aarch64.dmg" ;;
      *) echo "This release supports Apple Silicon Macs only." >&2; exit 1 ;;
    esac
    INSTALL_KIND=macos
    ;;
  Linux)
    [[ -f /etc/debian_version ]] || { echo "Linux installer supports Debian and Ubuntu." >&2; exit 1; }
    case "$(uname -m)" in
      x86_64|amd64) ASSET_SUFFIX="_amd64.deb" ;;
      *) echo "This release supports x86_64 Debian/Ubuntu only." >&2; exit 1 ;;
    esac
    command -v apt >/dev/null 2>&1 || { echo "apt is required" >&2; exit 1; }
    INSTALL_KIND=linux
    ;;
  *) echo "This installer supports Apple Silicon macOS and x86_64 Debian/Ubuntu. Download the Windows .msi from https://github.com/${REPOSITORY}/releases." >&2; exit 1 ;;
esac

RELEASE_JSON="$(curl --fail --location --silent --show-error \
  -H 'Accept: application/vnd.github+json' \
  "https://api.github.com/repos/${REPOSITORY}/releases/latest")"
ASSET_URL=""
while IFS= read -r url; do
  case "$url" in
    *"$ASSET_SUFFIX") ASSET_URL="$url"; break ;;
  esac
done < <(printf '%s\n' "$RELEASE_JSON" \
  | grep -Eo '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+"' \
  | sed -E 's/^[^:]+:[[:space:]]*"([^"]+)"$/\1/')

if [[ -z "$ASSET_URL" ]]; then
  echo "No ${ASSET_SUFFIX} installer found in the latest release: https://github.com/${REPOSITORY}/releases" >&2
  exit 1
fi

PACKAGE_PATH="${TEMP_DIR}/installer${ASSET_SUFFIX##*_}"
curl --fail --location --silent --show-error "$ASSET_URL" -o "$PACKAGE_PATH"

if [[ "$INSTALL_KIND" == macos ]]; then
  command -v hdiutil >/dev/null 2>&1 || { echo "hdiutil is required" >&2; exit 1; }
  command -v ditto >/dev/null 2>&1 || { echo "ditto is required" >&2; exit 1; }
  mkdir -p "$MOUNT_POINT" "$INSTALL_DIR"
  hdiutil attach "$PACKAGE_PATH" -nobrowse -readonly -mountpoint "$MOUNT_POINT" >/dev/null
  MOUNTED=1
  APP_SOURCE="$MOUNT_POINT/${APP_NAME}.app"
  [[ -d "$APP_SOURCE" ]] || { echo "The disk image does not contain ${APP_NAME}.app" >&2; exit 1; }
  ditto "$APP_SOURCE" "$INSTALL_DIR/${APP_NAME}.app"
  printf 'Installed: %s/%s.app\n' "$INSTALL_DIR" "$APP_NAME"
else
  if [[ "$EUID" -eq 0 ]]; then
    apt install --yes "$PACKAGE_PATH"
  else
    command -v sudo >/dev/null 2>&1 || { echo "Run as root or install sudo to install the Debian package." >&2; exit 1; }
    sudo apt install --yes "$PACKAGE_PATH"
  fi
fi
