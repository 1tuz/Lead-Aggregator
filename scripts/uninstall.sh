#!/usr/bin/env bash
set -euo pipefail

APP_NAME="Lead Aggregator"
BUNDLE_ID="dev.local.lead-aggregator"
PROCESS_PATTERN="twogis-extractor-desktop"

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

  local system_app="/Applications/${APP_NAME}.app"
  local user_app="${HOME}/Applications/${APP_NAME}.app"

  if [[ -d "$system_app" ]]; then
    if [[ -w "/Applications" && -w "$system_app" ]]; then
      remove_path "$system_app" 0
    else
      command -v sudo >/dev/null 2>&1 || {
        echo "Need sudo to remove ${system_app}" >&2
        exit 1
      }
      remove_path "$system_app" 1
    fi
  fi

  remove_path "$user_app" 0

  # App data / caches / UI state for this bundle id.
  remove_path "${HOME}/Library/Application Support/${BUNDLE_ID}" 0
  remove_path "${HOME}/Library/Caches/${BUNDLE_ID}" 0
  remove_path "${HOME}/Library/WebKit/${BUNDLE_ID}" 0
  remove_path "${HOME}/Library/Preferences/${BUNDLE_ID}.plist" 0
  remove_path "${HOME}/Library/Saved Application State/${BUNDLE_ID}.savedState" 0
  remove_path "${HOME}/Library/Logs/${BUNDLE_ID}" 0

  # Legacy id from earlier package names, if present.
  remove_path "${HOME}/Library/Application Support/com.twogis.extractor" 0
  remove_path "${HOME}/Library/Caches/com.twogis.extractor" 0

  if [[ "$removed_any" -eq 0 ]]; then
    printf 'Nothing to remove. %s was not found in /Applications or ~/Applications.\n' "$APP_NAME"
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
