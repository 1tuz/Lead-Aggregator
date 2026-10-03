#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/macos-paths.sh
source "${SCRIPT_DIR}/../lib/macos-paths.sh"

fail=0
assert_eq() {
  local got="$1"
  local want="$2"
  local label="$3"
  if [[ "$got" != "$want" ]]; then
    printf 'FAIL %s\n  got:  %s\n  want: %s\n' "$label" "$got" "$want" >&2
    fail=1
  else
    printf 'ok %s\n' "$label"
  fi
}

assert_contains() {
  local haystack="$1"
  local needle="$2"
  local label="$3"
  if [[ "$haystack" != *"$needle"* ]]; then
    printf 'FAIL %s\n  missing: %s\n  in: %s\n' "$label" "$needle" "$haystack" >&2
    fail=1
  else
    printf 'ok %s\n' "$label"
  fi
}

HOME_SAVE="${HOME}"
HOME="/tmp/lead-aggregator-test-home"
export HOME

unset LEAD_AGGREGATOR_INSTALL_DIR
unset LEAD_AGGREGATOR_USER_INSTALL
assert_eq "$(macos_default_install_dir)" "/Applications" "default install dir is /Applications"

LEAD_AGGREGATOR_USER_INSTALL=1
assert_eq "$(macos_default_install_dir)" "${HOME}/Applications" "user install override"
unset LEAD_AGGREGATOR_USER_INSTALL

LEAD_AGGREGATOR_INSTALL_DIR="/opt/apps"
assert_eq "$(macos_default_install_dir)" "/opt/apps" "explicit install dir"
unset LEAD_AGGREGATOR_INSTALL_DIR

paths="$(macos_uninstall_paths)"
assert_contains "$paths" "/Applications/Lead Aggregator.app" "uninstall lists /Applications"
assert_contains "$paths" "${HOME}/Applications/Lead Aggregator.app" "uninstall lists ~/Applications"
assert_contains "$paths" "${HOME}/Library/Application Support/dev.local.lead-aggregator" "uninstall lists app support"
assert_contains "$paths" "${HOME}/Library/Application Support/com.twogis.extractor" "uninstall lists legacy data"

HOME="${HOME_SAVE}"
export HOME

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi
printf 'All macos path tests passed\n'
