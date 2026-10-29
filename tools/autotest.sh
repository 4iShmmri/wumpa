#!/bin/sh
# Runs the real app once in a debug build: opens a PDF, reads its text,
# exports it to Word and Excel, and quits. Exercises the viewer and the
# IPC inside the system WebView, which unit tests cannot reach.
# Usage: tools/autotest.sh [file.pdf]   (defaults to a reference document)
set -e
cd "$(dirname "$0")/.."
src="${1:-$PWD/src-tauri/testdata/export/ar-report.pdf}"
out="$(mktemp -d)/autotest"
log="$HOME/Library/Logs/com.wumpa.app/wumpa.log"
bin="src-tauri/target/debug/wumpa"

[ -x src-tauri/binaries/wumpa-ocr-universal-apple-darwin ] || ./tools/ocr/build.sh
npx tauri build --debug --no-bundle >/dev/null

mkdir -p "$(dirname "$log")"
: > "$log"
WUMPA_AUTOTEST_SRC="$src" WUMPA_AUTOTEST_OUT="$out" "$bin" &
pid=$!
for _ in $(seq 1 180); do
  kill -0 "$pid" 2>/dev/null || break
  sleep 1
done
if kill -0 "$pid" 2>/dev/null; then
  kill "$pid"
  echo "autotest: the app did not finish within 3 minutes"
  cat "$log"
  exit 1
fi
if grep -q "^[0-9]* autotest ok" "$log" && [ -s "$out.docx" ] && [ -s "$out.xlsx" ]; then
  echo "autotest: $(grep "autotest ok" "$log" | tail -1 | cut -d' ' -f2-)"
  echo "outputs: $out.docx $out.xlsx"
else
  echo "autotest: failed"
  cat "$log"
  exit 1
fi
