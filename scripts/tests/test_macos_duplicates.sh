#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$TEST_DIR"' EXIT
# Test the actual inline helpers, without executing installation/uninstallation.
awk '/^# Kept inline/{copy=1} /^macos_default_install_dir\(\)/{copy=0} copy' "$ROOT/scripts/install.sh" > "$TEST_DIR/helpers"
awk '/^# Kept inline/{copy=1} /^macos_data_paths\(\)/{copy=0} copy' "$ROOT/scripts/uninstall.sh" > "$TEST_DIR/uninstall-helpers"
cmp "$TEST_DIR/helpers" "$TEST_DIR/uninstall-helpers"
source "$TEST_DIR/helpers"
LSREGISTER="$TEST_DIR/lsregister"
LOG="$TEST_DIR/log"
export LOG
cat > "$LSREGISTER" <<'STUB'
#!/usr/bin/env bash
if [[ "$1" == -h ]]; then
  printf "%s\n" "${LS_HELP:--kill}"
elif [[ "$1" == -dump ]]; then
  cat "$REGISTRY"
else
  printf '%s\n' "$*" >> "$LOG"
fi
STUB
chmod +x "$LSREGISTER"
REGISTRY="$TEST_DIR/registry"
export REGISTRY
cat > "$REGISTRY" <<'DUMP'
path:                       /Volumes/old image/Lead Aggregator.app (0x123)
identifier:                 dev.local.lead-aggregator
path:                       /Volumes/old legacy/2GIS Extractor.app (0x124)
identifier:                 dev.local.twogis-extractor
path:                       /Applications/Unrelated.app (0x125)
identifier:                 some.other.vendor
DUMP
expected=$'/Volumes/old image/Lead Aggregator.app\n/Volumes/old legacy/2GIS Extractor.app'
[[ "$(macos_registered_apps)" == "$expected" ]]
printf 'ok registry parsing includes missing current and legacy apps, excludes unrelated IDs\n'

TEST_DIR="$(cd "$TEST_DIR" && pwd -P)"
test_keep="$TEST_DIR/installed/Lead Aggregator.app"
duplicate="$TEST_DIR/target/release/bundle/macos/Lead Aggregator.app"
unrelated="$TEST_DIR/other/Lead Aggregator.app"
mkdir -p "$test_keep" "$duplicate" "$unrelated"
# Identity mock: test filesystem mutations without requiring macOS on CI.
macos_is_our_app() { [[ -d "$1" && ( "$1" == "$test_keep" || "$1" == "$duplicate" ) ]]; }
macos_app_candidates() { printf '%s\n' "$test_keep" "$duplicate" "$unrelated" '/Volumes/missing/Lead Aggregator.app'; }
(cd "$TEST_DIR" && macos_remove_duplicates "./installed/Lead Aggregator.app")
[[ -d "$test_keep" && ! -e "$duplicate" && -d "$unrelated" ]]
grep -Fx -- '-u /Volumes/missing/Lead Aggregator.app' "$LOG" >/dev/null
! grep -F -- "$unrelated" "$LOG" >/dev/null
macos_refresh_launch_services "$test_keep"
grep -Fx -- '-kill -r -domain local -domain system -domain user' "$LOG" >/dev/null
grep -Fx -- "-f $test_keep" "$LOG" >/dev/null
printf 'ok relative install path keeps one bundle, removes build duplicate, preserves unrelated app, unregisters stale DMG, rebuilds registry\n'

macos_remove_duplicates
[[ ! -e "$test_keep" && -d "$unrelated" ]]
printf 'ok uninstall removes remaining app\n'
LS_HELP='-gc -r -apps'
export LS_HELP
macos_refresh_launch_services "$test_keep"
grep -Fx -- '-gc' "$LOG" >/dev/null
grep -Fx -- '-r -apps local,system,user' "$LOG" >/dev/null
printf 'ok modern macOS rebuild uses garbage collection and rescan\n'

# Mounted images are detached, never passed to rm. A busy image is a failure.
macos_app_candidates() { printf '/Volumes/Lead Aggregator/Lead Aggregator.app\n'; }
macos_is_our_app() { return 0; }
diskutil() { printf '%s\n' "$*" >> "$LOG"; return 0; }
macos_remove_duplicates
grep -Fx -- 'unmount /Volumes/Lead Aggregator' "$LOG" >/dev/null
diskutil() { return 1; }
hdiutil() { return 1; }
if macos_remove_duplicates; then
  echo 'FAIL busy disk image must fail' >&2
  exit 1
fi
printf 'ok mounted image detached; busy image reports failure\n'
