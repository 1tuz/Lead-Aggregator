#!/usr/bin/env bash
# Self-contained: safe for `curl … | bash` (no sourced helpers).
set -euo pipefail

APP_NAME="Lead Aggregator"
BUNDLE_ID="dev.local.lead-aggregator"
PROCESS_PATTERN="twogis-extractor-desktop"
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

macos_data_paths() {
  local bundle_id="${1:-dev.local.lead-aggregator}"
  printf '%s\n' \
    "${HOME}/Library/Application Support/${bundle_id}" \
    "${HOME}/Library/Caches/${bundle_id}" \
    "${HOME}/Library/WebKit/${bundle_id}" \
    "${HOME}/Library/Preferences/${bundle_id}.plist" \
    "${HOME}/Library/Saved Application State/${bundle_id}.savedState" \
    "${HOME}/Library/Logs/${bundle_id}"
}

macos_legacy_data_paths() {
  printf '%s\n' \
    "${HOME}/Library/Application Support/com.twogis.extractor" \
    "${HOME}/Library/Caches/com.twogis.extractor"
}

removed_any=0

remove_path() {
  local path="$1"
  local use_sudo="${2:-0}"
  [[ -e "$path" || -L "$path" ]] || return 0
  if [[ "$use_sudo" == 1 ]]; then
    sudo rm -rf "$path"
  else
    rm -rf "$path"
  fi
  printf 'Removed: %s\n' "$path"
  removed_any=1
}

stop_app() {
  if pgrep -f "$PROCESS_PATTERN" >/dev/null 2>&1; then
    pkill -f "$PROCESS_PATTERN" >/dev/null 2>&1 || true
    sleep 0.5
    pkill -9 -f "$PROCESS_PATTERN" >/dev/null 2>&1 || true
    printf 'Stopped running %s process\n' "$APP_NAME"
  fi
}

uninstall_macos() {
  stop_app

  macos_remove_duplicates
  macos_refresh_launch_services

  while IFS= read -r path; do
    remove_path "$path" 0
  done < <(macos_data_paths "$BUNDLE_ID"; macos_data_paths dev.local.twogis-extractor; macos_legacy_data_paths)

  if [[ "$removed_any" -eq 0 ]]; then
    printf 'App copies and Launch Services entries cleaned; no saved data found.\n'
  else
    printf 'Uninstall complete.\n'
  fi
}

uninstall_linux() {
  stop_app
  if dpkg -s twogis-extractor-desktop >/dev/null 2>&1 || dpkg -s lead-aggregator >/dev/null 2>&1; then
    local pkg=""
    if dpkg -s twogis-extractor-desktop >/dev/null 2>&1; then
      pkg=twogis-extractor-desktop
    else
      pkg=lead-aggregator
    fi
    if [[ "$EUID" -eq 0 ]]; then
      apt remove --yes "$pkg"
    else
      command -v sudo >/dev/null 2>&1 || { echo "sudo is required to remove the Debian package." >&2; exit 1; }
      sudo apt remove --yes "$pkg"
    fi
    removed_any=1
  fi

  remove_path "${HOME}/.local/share/${BUNDLE_ID}" 0
  remove_path "${HOME}/.cache/${BUNDLE_ID}" 0
  remove_path "${HOME}/.config/${BUNDLE_ID}" 0

  if [[ "$removed_any" -eq 0 ]]; then
    printf 'Nothing to remove for %s on this system.\n' "$APP_NAME"
  else
    printf 'Uninstall complete.\n'
  fi
}

case "$(uname -s)" in
  Darwin) uninstall_macos ;;
  Linux) uninstall_linux ;;
  *)
    echo "This uninstaller supports macOS and Debian/Ubuntu. On Windows remove the app via Settings → Apps." >&2
    exit 1
    ;;
esac
