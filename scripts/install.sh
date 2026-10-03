#!/usr/bin/env bash
set -euo pipefail

REPOSITORY="1tuz/Lead-Aggregator"
APP_NAME="Lead Aggregator"
TEMP_DIR="$(mktemp -d)"
MOUNT_POINT="${TEMP_DIR}/mount"
MOUNTED=0
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/macos-paths.sh
source "${SCRIPT_DIR}/lib/macos-paths.sh"

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

ASSET_URL=""
for release_path in releases/latest 'releases?per_page=20'; do
  RELEASE_JSON="$(curl --fail --location --silent --show-error \
    -H 'Accept: application/vnd.github+json' \
    "https://api.github.com/repos/${REPOSITORY}/${release_path}")"
  while IFS= read -r url; do
    case "$url" in
      *"$ASSET_SUFFIX") ASSET_URL="$url"; break ;;
    esac
  done < <(printf '%s\n' "$RELEASE_JSON" \
    | grep -Eo '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]+"' \
    | sed -E 's/^[^:]+:[[:space:]]*"([^"]+)"$/\1/')
  if [[ -n "$ASSET_URL" ]]; then
    break
  fi
done

if [[ -z "$ASSET_URL" ]]; then
  echo "No ${ASSET_SUFFIX} installer found in recent releases: https://github.com/${REPOSITORY}/releases" >&2
  exit 1
fi

PACKAGE_PATH="${TEMP_DIR}/installer${ASSET_SUFFIX##*_}"
printf 'Downloading: %s\n' "$ASSET_URL"
curl --fail --location --silent --show-error "$ASSET_URL" -o "$PACKAGE_PATH"

print_gatekeeper_help() {
  local app_path="$1"
  cat >&2 <<EOF
macOS blocked the unsigned release build (no Apple Developer ID / notarization yet).

Open once via Finder:
  1. open "$(dirname "$app_path")"
  2. Right-click "${APP_NAME}.app" → Open → Open
Or clear quarantine and retry:
  xattr -dr com.apple.quarantine "${app_path}"
  open "${app_path}"
If macOS still says the app is damaged, System Settings → Privacy & Security → Open Anyway.
EOF
}

install_macos_app() {
  local app_source="$1"
  local app_dest="$2"
  local needs_sudo=0
  local dest_dir
  dest_dir="$(dirname "$app_dest")"

  if [[ "$dest_dir" == "/Applications" ]] && [[ ! -w /Applications ]]; then
    needs_sudo=1
  fi

  if [[ "$needs_sudo" == 1 ]]; then
    command -v sudo >/dev/null 2>&1 || {
      echo "Administrator rights are required to install into /Applications." >&2
      exit 1
    }
    sudo mkdir -p /Applications
    sudo rm -rf "$app_dest"
    sudo ditto "$app_source" "$app_dest"
    sudo xattr -dr com.apple.quarantine "$app_dest" 2>/dev/null || true
    if command -v codesign >/dev/null 2>&1; then
      sudo codesign --force --deep --sign - "$app_dest" >/dev/null 2>&1 || true
    fi
    if [[ -x "$LSREGISTER" ]]; then
      sudo "$LSREGISTER" -f "$app_dest" >/dev/null 2>&1 || true
    fi
  else
    mkdir -p "$dest_dir"
    rm -rf "$app_dest"
    ditto "$app_source" "$app_dest"
    xattr -dr com.apple.quarantine "$app_dest" 2>/dev/null || true
    if command -v codesign >/dev/null 2>&1; then
      codesign --force --deep --sign - "$app_dest" >/dev/null 2>&1 || true
    fi
    if [[ -x "$LSREGISTER" ]]; then
      "$LSREGISTER" -f "$app_dest" >/dev/null 2>&1 || true
    fi
  fi

  [[ -d "$app_dest" ]] || {
    echo "Install failed: ${app_dest} was not created" >&2
    exit 1
  }
}

if [[ "$INSTALL_KIND" == macos ]]; then
  command -v hdiutil >/dev/null 2>&1 || { echo "hdiutil is required" >&2; exit 1; }
  command -v ditto >/dev/null 2>&1 || { echo "ditto is required" >&2; exit 1; }
  INSTALL_DIR="$(macos_default_install_dir)"
  APP_DEST="${INSTALL_DIR}/${APP_NAME}.app"
  mkdir -p "$MOUNT_POINT"
  hdiutil attach "$PACKAGE_PATH" -nobrowse -readonly -mountpoint "$MOUNT_POINT" >/dev/null
  MOUNTED=1
  APP_SOURCE="$MOUNT_POINT/${APP_NAME}.app"
  [[ -d "$APP_SOURCE" ]] || { echo "The disk image does not contain ${APP_NAME}.app" >&2; exit 1; }

  install_macos_app "$APP_SOURCE" "$APP_DEST"
  printf 'Installed: %s\n' "$APP_DEST"

  if [[ "$INSTALL_DIR" != "/Applications" ]]; then
    printf 'Note: installed under %s (override via LEAD_AGGREGATOR_INSTALL_DIR or LEAD_AGGREGATOR_USER_INSTALL=1).\n' "$INSTALL_DIR"
  fi

  if ! open "$APP_DEST" 2>/dev/null; then
    print_gatekeeper_help "$APP_DEST"
    exit 1
  fi
else
  if [[ "$EUID" -eq 0 ]]; then
    apt install --yes "$PACKAGE_PATH"
  else
    command -v sudo >/dev/null 2>&1 || { echo "Run as root or install sudo to install the Debian package." >&2; exit 1; }
    sudo apt install --yes "$PACKAGE_PATH"
  fi
  nohup /usr/bin/twogis-extractor-desktop </dev/null >/dev/null 2>&1 &
fi
