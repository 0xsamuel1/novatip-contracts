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
# Safety overrides (see .env.example):
#   ALLOW_CUSTOM_TOKEN=1  skip the USDC SAC check when deploying with a
#                         non-canonical token
#   CONFIRM_MAINNET=1     skip the typed confirmation on mainnet deploys
#
# Usage:  ./scripts/deploy.sh

. "$(dirname "${BASH_SOURCE[0]}")/lib/env.sh"

require_env NETWORK SOURCE ADMIN USDC_TOKEN

# Canonical USDC Stellar Asset Contract ids per network. Must match the
# table in README.md (lines 53-58). (#123)
declare -A KNOWN_USDC_SAC=(
  [testnet]="CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75"
  [mainnet]="CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75"
  [local]=""
)

# Reject a USDC_TOKEN that doesn't match the known SAC for the chosen
# network. `local` has no canonical token so it always passes. (#123)
if [ "${NETWORK}" != "local" ] && [ -z "${ALLOW_CUSTOM_TOKEN:-}" ]; then
  known="${KNOWN_USDC_SAC[${NETWORK}]:-}"
  if [ -n "${known}" ] && [ "${USDC_TOKEN}" != "${known}" ]; then
    echo "error: USDC_TOKEN does not match the known USDC SAC for ${NETWORK}." >&2
    echo "  known: ${known}" >&2
    echo "  got:   ${USDC_TOKEN}" >&2
    echo "Set ALLOW_CUSTOM_TOKEN=1 to deploy anyway." >&2
    exit 1
  fi
fi

# Mainnet deploys require an explicit confirmation showing what will be
# deployed, since the token is fixed at construction. (#123)
if [ "${NETWORK}" == "mainnet" ] && [ -z "${CONFIRM_MAINNET:-}" ]; then
  echo ""
  echo "=== MAINNET DEPLOY CONFIRMATION ==="
  echo "  Network: ${NETWORK}"
  echo "  Source:  ${SOURCE}"
  echo "  Admin:   ${ADMIN}"
  echo "  Token:   ${USDC_TOKEN}"
  echo ""
  printf "Type 'yes' to confirm mainnet deploy: "
  read -r confirmation
  if [ "${confirmation}" != "yes" ]; then
    echo "Deploy cancelled." >&2
    exit 1
  fi
fi

echo "==> Building contracts"
stellar contract build

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
