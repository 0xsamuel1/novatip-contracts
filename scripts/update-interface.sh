#!/usr/bin/env bash
# Regenerate contracts/tip-splitter/interface.json from the built wasm (#134).
# Doc strings are stripped so comment edits do not trip the CI check.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

WASM=""
for candidate in \
  target/wasm32v1-none/release/tip_splitter.wasm \
  target/wasm32-unknown-unknown/release/tip_splitter.wasm
do
  if [ -f "$candidate" ]; then WASM="$candidate"; break; fi
done
if [ -z "$WASM" ]; then
  echo "building contract for interface snapshot..."
  stellar contract build 2>/dev/null || cargo build --release --target wasm32v1-none
  WASM=$(find target -name 'tip_splitter.wasm' -path '*release*' -not -path '*/deps/*' | head -1)
fi
if [ -z "$WASM" ] || [ ! -f "$WASM" ]; then
  echo "error: no tip_splitter.wasm found" >&2
  exit 1
fi

OUT="contracts/tip-splitter/interface.json"
mkdir -p "$(dirname "$OUT")"
stellar contract info interface --wasm "$WASM" --output json-formatted \
  | jq 'walk(if type == "object" then del(.doc, .docstring) else . end)' \
  > "$OUT"
echo "wrote $OUT from $WASM"
