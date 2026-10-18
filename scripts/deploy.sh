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
. "${NOVATIP_ROOT}/scripts/lib/cli.sh"

require_env NETWORK SOURCE ADMIN USDC_TOKEN
require_stellar_cli

# Canonical USDC Stellar Asset Contract ids per network. Must match the
# table in README.md. A case statement (not an associative array) keeps this
# working under macOS's bash 3.2 (#120).
known=""
case "${NETWORK}" in
  testnet | mainnet) known="CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75" ;;
  local) known="" ;;
esac

# Reject a USDC_TOKEN that doesn't match the known SAC for the chosen
# network. `local` has no canonical token so it always passes. (#123)
if [ "${NETWORK}" != "local" ] && [ -z "${ALLOW_CUSTOM_TOKEN:-}" ]; then
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
REPO="github:Novatip/novatip-contracts"
COMMIT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
DIRTY=""
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then DIRTY="-dirty"; fi
# `--optimize` is the current CLI's supported way to produce the deploy
# artifact; the separate optimize subcommand is deprecated (#121).
stellar contract build \
  --optimize \
  --meta source_repo="${REPO}" \
  --meta source_commit="${COMMIT}${DIRTY}"

WASM="$(./scripts/find-wasm.sh || true)"
if [ -z "${WASM}" ] || [ ! -f "${WASM}" ]; then
  echo "error: no built tip_splitter.wasm found under target/." >&2
  echo "       If the build printed a missing-target error, run:" >&2
  echo "       rustup target add wasm32v1-none" >&2
  exit 1
fi

WASM_SIZE=$(wc -c < "${WASM}")
echo "==> Built optimized ${WASM} (${WASM_SIZE} bytes)"

echo "==> Deploying tip_splitter to ${NETWORK}"
CONTRACT_ID=$(stellar contract deploy \
  --wasm "${WASM}" \
  --source "${SOURCE}" \
  --network "${NETWORK}" \
  -- \
  --admin "${ADMIN}" \
  --token "${USDC_TOKEN}")

CONTRACT_ID_FILE=".contract-id.${NETWORK}"
echo "==> Deployed contract id: ${CONTRACT_ID}"
echo "${CONTRACT_ID}" > "${CONTRACT_ID_FILE}"
echo "    (saved to ${CONTRACT_ID_FILE})"
