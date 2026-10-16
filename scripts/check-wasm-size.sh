#!/usr/bin/env bash
set -euo pipefail

# Check the built tip_splitter wasm against the size ceiling.
#
# Single source of truth for the limit: both CI (.github/workflows/ci.yml)
# and `make ci` call this script instead of carrying their own copy.
# Override with MAX_WASM_BYTES=... in the environment if needed.
#
# Usage: ./scripts/check-wasm-size.sh [path/to/tip_splitter.wasm]

# Hard ceiling for the unoptimized release wasm, in bytes.
#
# The contract builds to roughly 18 KiB today, so 64 KiB leaves well
# over 3x headroom: ordinary feature work and a new dependency or two
# will not trip it, and only a genuine regression — a heavyweight dep
# pulled in by accident, or `opt-level`/`lto` silently dropping out of
# the release profile — pushes the artifact past it.
#
# This measures the raw `cargo build` output, not the `stellar
# contract optimize` artifact that actually gets uploaded, so it is a
# deliberately loose upper bound rather than a cost estimate. If
# optimization moves into CI, point the check at the optimized file
# and lower this number to match.
#
# Raising it is fine when the growth is understood and intentional —
# the point is that the bump shows up in a diff and gets a reason.
MAX_WASM_BYTES="${MAX_WASM_BYTES:-65536}"

WASM="${1:-target/wasm32-unknown-unknown/release/tip_splitter.wasm}"
if [ ! -f "$WASM" ]; then
  echo "error: expected wasm at $WASM but it was not built" >&2
  exit 1
fi
SIZE=$(wc -c < "$WASM")
echo "tip_splitter.wasm: $SIZE bytes ($((SIZE / 1024)) KiB)"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  {
    echo "### Contract size"
    echo
    echo "| Artifact | Bytes | KiB | Limit (bytes) |"
    echo "|----------|------:|----:|--------------:|"
    echo "| \`tip_splitter.wasm\` | $SIZE | $((SIZE / 1024)) | $MAX_WASM_BYTES |"
  } >> "$GITHUB_STEP_SUMMARY"
fi
if [ "$SIZE" -gt "$MAX_WASM_BYTES" ]; then
  echo "::error::tip_splitter.wasm is $SIZE bytes, over the $MAX_WASM_BYTES byte limit." >&2
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    echo "Shrink the contract, or raise MAX_WASM_BYTES in scripts/check-wasm-size.sh with a reason." >> "$GITHUB_STEP_SUMMARY"
  fi
  exit 1
fi
