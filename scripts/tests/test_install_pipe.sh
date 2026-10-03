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

# Piped install must not reference BASH_SOURCE or external lib/.
if grep -nE 'BASH_SOURCE|macos-paths\.sh' "${ROOT}/scripts/install.sh" "${ROOT}/scripts/uninstall.sh" >/tmp/lead-agg-pipe-scan.err; then
  printf 'FAIL install/uninstall still depend on BASH_SOURCE/lib source\n' >&2
  cat /tmp/lead-agg-pipe-scan.err >&2
  FAIL=1
else
  printf 'ok install/uninstall are self-contained\n'
fi

# Simulate curl|bash: pipe script, exit before network download.
STUB="$(mktemp)"
trap 'rm -f "$STUB"' EXIT
awk '
  /^ASSET_URL=""/ {
    print "echo PIPE_OK"
    print "exit 0"
  }
  { print }
' "${ROOT}/scripts/install.sh" >"$STUB"

if out="$(bash <"$STUB")"; then
  if [[ "$out" == *PIPE_OK* ]]; then
    printf 'ok piped install.sh reaches post-bootstrap\n'
  else
    printf 'FAIL piped install output: %s\n' "$out" >&2
    FAIL=1
  fi
else
  printf 'FAIL piped install.sh\n' >&2
  FAIL=1
fi

if [[ "$FAIL" -ne 0 ]]; then
  exit 1
fi
printf 'All install pipe tests passed\n'
