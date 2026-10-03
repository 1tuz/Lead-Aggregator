#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FAIL=0

assert_ok() {
  local label="$1"
  shift
  if "$@" >/dev/null 2>/tmp/lead-agg-pipe-test.err; then
    printf 'ok %s\n' "$label"
  else
    printf 'FAIL %s\n' "$label" >&2
    cat /tmp/lead-agg-pipe-test.err >&2 || true
    FAIL=1
  fi
}

assert_ok "install.sh syntax" bash -n "${ROOT}/scripts/install.sh"
assert_ok "uninstall.sh syntax" bash -n "${ROOT}/scripts/uninstall.sh"
assert_ok "local install sources lib" bash -c "
  set -euo pipefail
  # shellcheck disable=SC1091
  source '${ROOT}/scripts/lib/macos-paths.sh'
  test \"\$(macos_default_install_dir)\" = '/Applications'
"

# Reproduce curl|bash: no BASH_SOURCE file, must fetch helper.
PIPE_SCRIPT="$(mktemp)"
trap 'rm -f "$PIPE_SCRIPT"' EXIT
cat >"$PIPE_SCRIPT" <<EOF
set -euo pipefail
REPOSITORY="1tuz/Lead-Aggregator"
RAW_SCRIPTS_BASE="file://${ROOT}/scripts"
TEMP_DIR="\$(mktemp -d)"
trap 'rm -rf "\$TEMP_DIR"' EXIT
load_macos_paths() {
  local script_path="\${BASH_SOURCE[0]:-}"
  if [[ -n "\$script_path" && -f "\$script_path" ]]; then
    local dir helper
    dir="\$(cd "\$(dirname "\$script_path")" && pwd)"
    helper="\${dir}/lib/macos-paths.sh"
    if [[ -f "\$helper" ]]; then
      source "\$helper"
      return 0
    fi
  fi
  local remote_helper="\${TEMP_DIR}/macos-paths.sh"
  curl --fail --location --silent --show-error \\
    "\${RAW_SCRIPTS_BASE}/lib/macos-paths.sh" -o "\$remote_helper"
  source "\$remote_helper"
}
load_macos_paths
test "\$(macos_default_install_dir)" = "/Applications"
echo LOADED
EOF

# Pipe through bash so BASH_SOURCE[0] is not a real script file path.
if out="$(bash <"$PIPE_SCRIPT")"; then
  if [[ "$out" == *LOADED* ]]; then
    printf 'ok piped helper load without BASH_SOURCE file\n'
  else
    printf 'FAIL piped helper load: unexpected output: %s\n' "$out" >&2
    FAIL=1
  fi
else
  printf 'FAIL piped helper load\n' >&2
  FAIL=1
fi

# Ensure the real install.sh no longer trips unbound BASH_SOURCE when piped.
# Stub network-heavy part by exiting immediately after load_macos_paths.
STUB="$(mktemp)"
trap 'rm -f "$PIPE_SCRIPT" "$STUB"' EXIT
awk '
  BEGIN { print "set -euo pipefail" }
  /^REPOSITORY=/ { print; next }
  /^RAW_SCRIPTS_BASE=/ {
    print "RAW_SCRIPTS_BASE=\"file://'"${ROOT}"'/scripts\""
    next
  }
  { print }
  /^load_macos_paths$/ {
    print "macos_default_install_dir >/dev/null"
    print "echo LOADED"
    print "exit 0"
  }
' "${ROOT}/scripts/install.sh" >"$STUB"

if out="$(bash <"$STUB")"; then
  if [[ "$out" == *LOADED* ]]; then
    printf 'ok piped install.sh survives helper bootstrap\n'
  else
    printf 'FAIL piped install.sh bootstrap output: %s\n' "$out" >&2
    FAIL=1
  fi
else
  printf 'FAIL piped install.sh bootstrap\n' >&2
  FAIL=1
fi

if [[ "$FAIL" -ne 0 ]]; then
  exit 1
fi
printf 'All install pipe tests passed\n'
