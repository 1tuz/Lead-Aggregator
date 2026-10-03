#!/usr/bin/env bash
# Shared helpers for macOS install/uninstall path selection.
# Sourced by install.sh / uninstall.sh and scripts/tests.

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

macos_app_bundle_name() {
  printf 'Lead Aggregator.app\n'
}

macos_app_dest_path() {
  local install_dir
  install_dir="$(macos_default_install_dir)"
  printf '%s/%s\n' "$install_dir" "$(macos_app_bundle_name)"
}

macos_legacy_app_paths() {
  printf '%s/%s\n' "${HOME}/Applications" "$(macos_app_bundle_name)"
  printf '/Applications/%s\n' "$(macos_app_bundle_name)"
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

macos_uninstall_paths() {
  macos_legacy_app_paths
  macos_data_paths "dev.local.lead-aggregator"
  macos_legacy_data_paths
}
