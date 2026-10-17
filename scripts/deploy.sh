#!/usr/bin/env bash
set -euo pipefail

# Deploy the tip_splitter contract to a Stellar network.
#
# .env (repository root) is loaded automatically; exported variables win over
# it. The script works when started from any directory.
#
# Required env (see .env.example):
#   NETWORK     testnet | mainnet | local
#   SOURCE      Stellar CLI identity used to sign
#   ADMIN       admin address stored in the contract
#   USDC_TOKEN  USDC Stellar Asset Contract id (C...)
#
# Usage:  ./scripts/deploy.sh

. "$(dirname "${BASH_SOURCE[0]}")/lib/env.sh"

require_env NETWORK SOURCE ADMIN USDC_TOKEN

echo "==> Building contracts"
REPO="github:Novatip/novatip-contracts"
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
DIRTY=""
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then DIRTY="-dirty"; fi
stellar contract build \
  --meta source_repo="${REPO}" \
  --meta source_commit="${COMMIT}${DIRTY}"

# The Stellar CLI moved its wasm target from wasm32-unknown-unknown to
# wasm32v1-none, so the output path differs by CLI version. Locate the artifact
# rather than hardcoding either path, newest first.
WASM="$(find target -name 'tip_splitter.wasm' -path '*release*' -not -path '*/deps/*' \
        -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-)"

if [ -z "${WASM}" ] || [ ! -f "${WASM}" ]; then
  echo "error: no built tip_splitter.wasm found under target/." >&2
  echo "       If the build printed a missing-target error, run:" >&2
  echo "       rustup target add wasm32v1-none" >&2
  exit 1
fi

BUILT_SIZE=$(wc -c < "${WASM}")
echo "==> Built ${WASM} (${BUILT_SIZE} bytes)"

echo "==> Optimizing wasm"
stellar contract optimize --wasm "${WASM}"
OPTIMIZED_WASM="${WASM%.wasm}.optimized.wasm"
if [ ! -f "${OPTIMIZED_WASM}" ]; then
  echo "error: expected optimized wasm at ${OPTIMIZED_WASM}" >&2
  exit 1
fi
OPTIMIZED_SIZE=$(wc -c < "${OPTIMIZED_WASM}")
echo "==> Optimized ${OPTIMIZED_WASM} (${BUILT_SIZE} -> ${OPTIMIZED_SIZE} bytes, $((BUILT_SIZE - OPTIMIZED_SIZE)) saved)"
WASM="${OPTIMIZED_WASM}"

echo "==> Deploying tip_splitter to ${NETWORK}"
CONTRACT_ID=$(stellar contract deploy \
  --wasm "${WASM}" \
  --source "${SOURCE}" \
  --network "${NETWORK}" \
  -- \
  --admin "${ADMIN}" \
  --token "${USDC_TOKEN}")

echo "==> Deployed contract id: ${CONTRACT_ID}"
echo "${CONTRACT_ID}" > .contract-id
echo "    (saved to .contract-id)"
