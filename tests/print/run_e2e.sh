#!/usr/bin/env bash
# End-to-end print test. Drives the real Monocle.app via JXA, opens the file,
# fires ⌘P, reads the "All N Pages" label from the print dialog, and checks
# it matches the expected page count.
#
# Catches the stale-pagination bug in Tauri's `w.print()` path that the
# headless harness (run.sh) cannot reproduce.
#
# Requirements:
#   - /Applications/Monocle.app present (use the symlink to the build output)
#   - Accessibility permission for Terminal.app (or whatever launches this):
#     System Settings → Privacy & Security → Accessibility → enable Terminal
#
# usage: ./run_e2e.sh [fixture.md] [expected-pages]

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
FIXTURE="${1:-$SCRIPT_DIR/fixtures/knob_analysis.md}"
EXPECTED="${2:-3}"

echo "[step] driving Monocle.app, fixture=$(basename "$FIXTURE"), expected=$EXPECTED" >&2
if osascript -l JavaScript "$SCRIPT_DIR/check_pages.js" "$FIXTURE" "$EXPECTED"; then
    echo "[PASS]" >&2
    exit 0
else
    code=$?
    echo "[FAIL] check_pages.js exited $code — first ⌘P produced wrong page count" >&2
    exit "$code"
fi
