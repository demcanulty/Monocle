#!/usr/bin/env bash
# Print-pagination test harness.
#
# Pipeline:
#   1. Render the markdown fixture to HTML using the same pulldown-cmark options Monocle uses.
#   2. Load that HTML into an offscreen WKWebView and save to PDF via NSPrintOperation.
#      (Same WebKit print pipeline that Monocle's `w.print()` hits.)
#   3. Extract text from the PDF with pdftotext.
#   4. Assert that every expected string is present.
#
# A failure here is a real user-visible bug.

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)
WORK_DIR=$(mktemp -d /tmp/monocle-print-test.XXXXXX)
trap 'rm -rf "$WORK_DIR"' EXIT

FIXTURE="${1:-$SCRIPT_DIR/fixtures/knob_analysis.md}"
STYLES="$REPO_ROOT/src/styles.css"
HTML="$WORK_DIR/page.html"
PDF="$WORK_DIR/page.pdf"
TXT="$WORK_DIR/page.txt"

# --- Build the Rust + Swift tools once, cache in WORK_DIR's sibling. ---
TOOL_DIR="$SCRIPT_DIR/.tools"
mkdir -p "$TOOL_DIR"
RENDER_BIN="$TOOL_DIR/render_html"
PDF_BIN="$TOOL_DIR/html_to_pdf"

if [[ ! -x "$RENDER_BIN" || "$SCRIPT_DIR/render_html.rs" -nt "$RENDER_BIN" ]]; then
    echo "[build] render_html.rs -> $RENDER_BIN" >&2
    (cd "$SCRIPT_DIR" && cargo build --release --quiet --bin render_html)
    cp "$SCRIPT_DIR/target/release/render_html" "$RENDER_BIN"
fi

if [[ ! -x "$PDF_BIN" || "$SCRIPT_DIR/html_to_pdf.swift" -nt "$PDF_BIN" ]]; then
    echo "[build] html_to_pdf.swift -> $PDF_BIN" >&2
    swiftc -O -o "$PDF_BIN" "$SCRIPT_DIR/html_to_pdf.swift"
fi

# --- Render markdown -> HTML ---
echo "[step] render markdown" >&2
"$RENDER_BIN" "$FIXTURE" "$STYLES" "$HTML"

# --- HTML -> PDF via WKWebView + NSPrintOperation ---
echo "[step] render PDF" >&2
"$PDF_BIN" "$HTML" "$PDF"

# --- PDF -> text ---
if ! command -v pdftotext >/dev/null; then
    echo "pdftotext not found — install via 'brew install poppler'" >&2
    exit 3
fi
echo "[step] extract text" >&2
pdftotext -layout "$PDF" "$TXT"

# --- Page count + assertions ---
PAGES=$(pdfinfo "$PDF" 2>/dev/null | awk '/^Pages:/ {print $2}')
echo "[info] pages: $PAGES" >&2

EXPECTED=(
    "Buchla USA"
    "Background"
    "Pink — Poor fit"
    "Blue — Good fit"
    "Measurements"
    "Cross-section to check internal taper"
    "Light squeeze with pliers"
    "Questions for the Knob Engineers"
    "Is this squeezing process safe as a production technique?"
    "Is a squeezed knob likely to retain"
    "bin for tight-fitting knobs"
    "Based on these tests"
    "What is your recommendation"
)

fail=0
for needle in "${EXPECTED[@]}"; do
    if grep -qF -- "$needle" "$TXT"; then
        echo "  ok: $needle" >&2
    else
        echo "  MISSING: $needle" >&2
        fail=1
    fi
done

if [[ "$fail" -ne 0 ]]; then
    echo "" >&2
    echo "[FAIL] missing strings — PDF kept at $PDF" >&2
    # Don't auto-delete the work dir so we can inspect.
    trap - EXIT
    echo "       text   at $TXT" >&2
    exit 1
fi

echo "[PASS] all expected strings found" >&2
