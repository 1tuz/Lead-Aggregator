#!/usr/bin/env bash
# Self-contained: safe for `curl … | bash` (no sourced helpers).
set -euo pipefail

REPOSITORY="1tuz/Lead-Aggregator"
APP_NAME="Lead Aggregator"
TEMP_DIR="$(mktemp -d)"
MOUNT_POINT="${TEMP_DIR}/mount"
MOUNTED=0
LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"

# Kept inline in both scripts: each script also runs via curl | bash.
macos_registered_apps() {
  [[ -x "$LSREGISTER" ]] || return 0
  "$LSREGISTER" -dump 2>/dev/null | awk '
    /^path:/ { path=$0; sub(/^path:[[:space:]]*/, "", path); sub(/ \(0x[[:xdigit:]]+\)$/, "", path) }
    /^identifier:[[:space:]]+(dev\.local\.lead-aggregator|dev\.local\.twogis-extractor|com\.twogis\.extractor)[[:space:]]*$/ { print path }
  '
}

macos_app_candidates() {
  macos_registered_apps
  if command -v mdfind >/dev/null 2>&1; then
    mdfind "kMDItemCFBundleIdentifier == 'dev.local.lead-aggregator' || kMDItemCFBundleIdentifier == 'dev.local.twogis-extractor' || kMDItemCFBundleIdentifier == 'com.twogis.extractor'" 2>/dev/null || true
  fi
  local root
  for root in /Applications "${HOME}/Applications" "${PWD}/target" "${LEAD_AGGREGATOR_INSTALL_DIR:-${HOME}/Applications}" /Volumes; do
    [[ -d "$root" ]] || continue
    find "$root" -name '*.app' -prune -print 2>/dev/null || true
  done
}

macos_is_our_app() {
  local identifier
  [[ -d "$1" && ! -L "$1" ]] || return 1
  identifier="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$1/Contents/Info.plist" 2>/dev/null)" || return 1
  case "$identifier" in
    dev.local.lead-aggregator|dev.local.twogis-extractor|com.twogis.extractor) return 0 ;;
    *) return 1 ;;
  esac
}

macos_remove_duplicates() {
  local keep="${1:-}" path
  if [[ -n "$keep" && -d "$keep" ]]; then
    keep="$(cd "$(dirname "$keep")" && pwd -P)/$(basename "$keep")"
  fi
  while IFS= read -r path; do
    if [[ -d "$path" ]]; then
      path="$(cd "$(dirname "$path")" && pwd -P)/$(basename "$path")"
    fi
    [[ "$path" == /*.app && "$path" != "$keep" ]] || continue
    # Missing entries still need unregistering, especially old DMG mount points.
    if [[ ! -e "$path" ]] || macos_is_our_app "$path"; then
      [[ ! -x "$LSREGISTER" ]] || "$LSREGISTER" -u "$path" >/dev/null 2>&1 || true
    fi
    macos_is_our_app "$path" || continue
    # Never delete from a mounted image. Unregister and eject only its volume.
    if [[ "$path" == /Volumes/* ]]; then
      local volume="/Volumes/$(printf '%s' "${path#/Volumes/}" | cut -d / -f 1)"
      diskutil unmount "$volume" >/dev/null 2>&1 || hdiutil detach "$volume" -quiet >/dev/null 2>&1 || {
        echo "Cannot unmount $volume; close the disk image and rerun." >&2
        return 1
      }
    elif [[ -w "$(dirname "$path")" && -w "$path" ]]; then
      rm -rf "$path"
      printf 'Removed duplicate: %s\n' "$path" >&2
    else
      sudo rm -rf "$path"
      printf 'Removed duplicate: %s\n' "$path" >&2
    fi
  done < <(macos_app_candidates | sort -u)
}

macos_refresh_launch_services() {
  [[ -x "$LSREGISTER" ]] || return 0
  # Run as the desktop user, not sudo (which would rebuild root's registry).
  local help
  help="$("$LSREGISTER" -h 2>&1 || true)"
  if printf '%s\n' "$help" | grep -q -- '-kill'; then
    "$LSREGISTER" -kill -r -domain local -domain system -domain user >/dev/null 2>&1
  else
    # Recent macOS removed -kill. -delete requires reboot; GC + rescan is live.
    "$LSREGISTER" -gc >/dev/null 2>&1
    "$LSREGISTER" -r -apps local,system,user >/dev/null 2>&1
  fi
  [[ -z "${1:-}" ]] || "$LSREGISTER" -f "$1" >/dev/null 2>&1
}

macos_default_install_dir() {
  if [[ -n "${LEAD_AGGREGATOR_INSTALL_DIR:-}" ]]; then
    printf '%s\n' "$LEAD_AGGREGATOR_INSTALL_DIR"
    return 0
  fi
  if [[ "${LEAD_AGGREGATOR_USER_INSTALL:-}" == "1" ]]; then
    printf '%s\n' "${HOME}/Applications"
    return 0
  fi
  printf '/Applications\n'
}

detach_image() {
  # Prefer modern diskutil; fall back to hdiutil.
  diskutil unmount force "$MOUNT_POINT" >/dev/null 2>&1 \
    || diskutil eject "$MOUNT_POINT" >/dev/null 2>&1 \
    || hdiutil detach "$MOUNT_POINT" -quiet >/dev/null 2>&1
}

attach_image() {
  local image="$1"
  local mount="$2"
  mkdir -p "$mount"
  if diskutil image attach --help >/dev/null 2>&1 \
    || diskutil image 2>&1 | grep -q 'attach'; then
    diskutil image attach --readOnly --nobrowse --mountPoint "$mount" "$image" >/dev/null
  else
    # Older macOS only.
    hdiutil attach "$image" -nobrowse -readonly -mountpoint "$mount" >/dev/null 2>&1
  fi
}

cleanup() {
  if [[ "$MOUNTED" == 1 ]] && ! detach_image; then
    echo "Installer image is still mounted at $MOUNT_POINT; temporary directory retained." >&2
    return 0
  fi
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }

print_banner() {
  # stderr so banner stays visible with `curl … | bash` and download progress.
  cat <<'EOF' >&2

██╗     ███████╗ █████╗ ██████╗      █████╗  ██████╗  ██████╗ ██████╗ ███████╗ ██████╗  █████╗ ████████╗ ██████╗ ██████╗
██║     ██╔════╝██╔══██╗██╔══██╗    ██╔══██╗██╔════╝ ██╔════╝ ██╔══██╗██╔════╝██╔════╝ ██╔══██╗╚══██╔══╝██╔═══██╗██╔══██╗
██║     █████╗  ███████║██║  ██║    ███████║██║  ███╗██║  ███╗██████╔╝█████╗  ██║  ███╗███████║   ██║   ██║   ██║██████╔╝
██║     ██╔══╝  ██╔══██║██║  ██║    ██╔══██║██║   ██║██║   ██║██╔══██╗██╔══╝  ██║   ██║██╔══██║   ██║   ██║   ██║██╔══██╗
███████╗███████╗██║  ██║██████╔╝    ██║  ██║╚██████╔╝╚██████╔╝██║  ██║███████╗╚██████╔╝██║  ██║   ██║   ╚██████╔╝██║  ██║
╚══════╝╚══════╝╚═╝  ╚═╝╚═════╝     ╚═╝  ╚═╝ ╚═════╝  ╚═════╝ ╚═╝  ╚═╝╚══════╝ ╚═════╝ ╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝  ╚═╝
  Lead Aggregator · desktop lead collector · catalogs → CSV / Excel / JSON

EOF
}
print_banner

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

# /releases is newest-first. Prefer it over /releases/latest (can lag).
ASSET_URL=""
for release_path in 'releases?per_page=30' releases/latest; do
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
printf 'Downloading: %s\n' "$ASSET_URL" >&2
# Progress bar on stderr (works with `curl … | bash`; keep API calls silent above).
curl --fail --location --progress-bar --show-error "$ASSET_URL" -o "$PACKAGE_PATH"
printf '\n' >&2

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
  else
    mkdir -p "$dest_dir"
    rm -rf "$app_dest"
    ditto "$app_source" "$app_dest"
    xattr -dr com.apple.quarantine "$app_dest" 2>/dev/null || true
    if command -v codesign >/dev/null 2>&1; then
      codesign --force --deep --sign - "$app_dest" >/dev/null 2>&1 || true
    fi
  fi

  [[ -d "$app_dest" ]] || {
    echo "Install failed: ${app_dest} was not created" >&2
    exit 1
  }
}

if [[ "$INSTALL_KIND" == macos ]]; then
  command -v ditto >/dev/null 2>&1 || { echo "ditto is required" >&2; exit 1; }
  if ! command -v diskutil >/dev/null 2>&1 && ! command -v hdiutil >/dev/null 2>&1; then
    echo "diskutil or hdiutil is required" >&2
    exit 1
  fi
  INSTALL_DIR="$(macos_default_install_dir)"
  # Canonicalize a custom relative directory before comparing duplicate paths.
  if [[ "$INSTALL_DIR" != /Applications ]]; then
    mkdir -p "$INSTALL_DIR"
    INSTALL_DIR="$(cd "$INSTALL_DIR" && pwd -P)"
  fi
  APP_DEST="${INSTALL_DIR}/${APP_NAME}.app"
  attach_image "$PACKAGE_PATH" "$MOUNT_POINT"
  MOUNTED=1
  APP_SOURCE="$MOUNT_POINT/${APP_NAME}.app"
  [[ -d "$APP_SOURCE" ]] || { echo "The disk image does not contain ${APP_NAME}.app" >&2; exit 1; }

  macos_is_our_app "$APP_SOURCE" || { echo "Unexpected bundle identifier in disk image" >&2; exit 1; }
  if [[ -e "$APP_DEST" ]] && ! macos_is_our_app "$APP_DEST"; then
    echo "Refusing to replace unrelated application: $APP_DEST" >&2
    exit 1
  fi
  install_macos_app "$APP_SOURCE" "$APP_DEST"
  # Detach before registry rebuild so the release image cannot appear in Spotlight.
  [[ ! -x "$LSREGISTER" ]] || "$LSREGISTER" -u "$APP_SOURCE" >/dev/null 2>&1 || true
  if ! detach_image; then
    echo "Cannot detach installer image: $MOUNT_POINT" >&2
    exit 1
  fi
  MOUNTED=0
  macos_remove_duplicates "$APP_DEST"
  macos_refresh_launch_services "$APP_DEST"
  printf 'Installed: %s\n' "$APP_DEST" >&2

  if [[ "$INSTALL_DIR" != "/Applications" ]]; then
    printf 'Note: installed under %s (override via LEAD_AGGREGATOR_INSTALL_DIR or LEAD_AGGREGATOR_USER_INSTALL=1).\n' "$INSTALL_DIR" >&2
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
