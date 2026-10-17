#!/usr/bin/env bash
# Fail when rust-toolchain.toml channel and workspace rust-version disagree (#133).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLCHAIN_FILE="$ROOT/rust-toolchain.toml"
CARGO_TOML="$ROOT/Cargo.toml"

CHANNEL=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$TOOLCHAIN_FILE" | head -1)
MSRV=$(sed -n 's/^rust-version = "\(.*\)"/\1/p' "$CARGO_TOML" | head -1)

if [ -z "$CHANNEL" ] || [ -z "$MSRV" ]; then
  echo "error: could not read channel ($CHANNEL) or rust-version ($MSRV)" >&2
  exit 1
fi

# Compare major.minor only (channel may be 1.84.0 vs rust-version 1.84).
CH_MM=$(echo "$CHANNEL" | cut -d. -f1-2)
MSRV_MM=$(echo "$MSRV" | cut -d. -f1-2)
if [ "$CH_MM" != "$MSRV_MM" ]; then
  echo "error: rust-toolchain.toml channel ($CHANNEL) and workspace rust-version ($MSRV) disagree" >&2
  echo "Update both to the same major.minor — crate Cargo.toml inherits rust-version from the workspace." >&2
  exit 1
fi
echo "check-msrv: OK (channel $CHANNEL / rust-version $MSRV)"
