#!/usr/bin/env bash
# Compare the built wasm's public interface against the committed snapshot (#134).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

COMMITTED="contracts/tip-splitter/interface.json"
if [ ! -f "$COMMITTED" ]; then
  echo "error: $COMMITTED is missing — run 'make interface' and commit it" >&2
  exit 1
fi

WASM=""
for candidate in \
  target/wasm32v1-none/release/tip_splitter.wasm \
  target/wasm32-unknown-unknown/release/tip_splitter.wasm
do
  if [ -f "$candidate" ]; then WASM="$candidate"; break; fi
done
if [ -z "$WASM" ]; then
  stellar contract build 2>/dev/null || cargo build --release --target wasm32v1-none
  WASM=$(find target -name 'tip_splitter.wasm' -path '*release*' -not -path '*/deps/*' | head -1)
fi

TMP=$(mktemp)
trap 'rm -f "$TMP"' EXIT
stellar contract info interface --wasm "$WASM" --output json-formatted \
  | jq 'walk(if type == "object" then del(.doc, .docstring) else . end)' \
  > "$TMP"

if ! diff -u "$COMMITTED" "$TMP"; then
  echo "" >&2
  echo "error: contract public interface changed without updating $COMMITTED" >&2
  echo "Run 'make interface', review the diff, and add a CHANGELOG.md entry." >&2
  echo "Do not renumber existing Error variants — append new ones only." >&2
  exit 1
fi
echo "check-interface: OK — public interface matches $COMMITTED"
