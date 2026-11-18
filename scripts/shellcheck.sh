#!/usr/bin/env bash
# Lint every shell script in the repository.
#
# The official CI runs shellcheck on its scripts; the mobile repository had no
# such gate. This wrapper is that gate. It uses only the shellcheck binary the
# runner already ships — no downloads, no other tools.
#
# Notes are not failures: `--severity=warning` keeps the gate on warnings and
# errors, which is where real breakage lives. The four files below carry
# pre-existing warnings in code this work order does not own; each gets a
# per-file exclusion of exactly those codes, the same way upstream's CI
# excludes SC2037/SC2016 for `board-test.sh`. Every other script, including
# this one, must be clean under the warning threshold.
#
# Usage: scripts/shellcheck.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SHELLCHECK="${SHELLCHECK:-shellcheck}"
if ! command -v "$SHELLCHECK" >/dev/null 2>&1; then
  echo "shellcheck.sh: $SHELLCHECK not found" >&2
  exit 2
fi

# Per-file exclusions for warnings that predate this gate.
exclusions() {
  case "$1" in
    # ls | grep and an unquoted substitution in the rust target probe.
    android/scripts/build-rust-android.sh) echo "SC2010,SC2046" ;;
    # Unused shell variables read by the emulator/test harness.
    android/scripts/smoke-test.sh) echo "SC2034" ;;
    # `cd` without a failure guard; the script continues on its own.
    ios/scripts/control-app.sh) echo "SC2164" ;;
    # Unused variables and `$?` read after a condition, across the harness.
    ios/scripts/sim-drive.sh) echo "SC2034,SC2319" ;;
    *) echo "" ;;
  esac
}

for script in scripts/*.sh ios/scripts/*.sh android/scripts/*.sh; do
  excludes="$(exclusions "$script")"
  if [ -n "$excludes" ]; then
    "$SHELLCHECK" --severity=warning --exclude="$excludes" "$script"
  else
    "$SHELLCHECK" --severity=warning "$script"
  fi
done

echo "shellcheck: all scripts clean at warning severity"
