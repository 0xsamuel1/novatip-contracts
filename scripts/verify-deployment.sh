#!/usr/bin/env bash
set -euo pipefail

# Verify that a deployed contract matches a local build.
#
# Fetches the deployed wasm via `stellar contract fetch`, builds the current
# checkout the same way `deploy.sh` does, and compares SHA-256 hashes.
#
# Usage:  ./scripts/verify-deployment.sh [CONTRACT_ID]
#
# If CONTRACT_ID is omitted, reads `.contract-id.<NETWORK>` (written by deploy.sh).
# Requires: NETWORK env var (testnet | mainnet | local), stellar CLI, sha256sum.

. "$(dirname "${BASH_SOURCE[0]}")/lib/env.sh"
. "${NOVATIP_ROOT}/scripts/lib/cli.sh"

require_env NETWORK
require_stellar_cli

CONTRACT_ID="${1:-}"
if [ -z "${CONTRACT_ID}" ]; then
  require_contract_id
fi

echo "==> Verifying deployment ${CONTRACT_ID} on ${NETWORK}"

# ── Fetch the deployed wasm ─────────────────────────────────────────────────
DEPLOYED_WASM=$(mktemp /tmp/deployed-XXXXXX.wasm)
trap 'rm -f "${DEPLOYED_WASM}"' EXIT

echo "==> Fetching deployed wasm"
stellar contract fetch \
  --id "${CONTRACT_ID}" \
  --network "${NETWORK}" \
  --out-file "${DEPLOYED_WASM}"

DEPLOYED_HASH=$(sha256sum "${DEPLOYED_WASM}" | cut -d' ' -f1)
echo "    Deployed SHA-256: ${DEPLOYED_HASH}"

# ── Build locally (same as deploy.sh) ───────────────────────────────────────
echo "==> Building local wasm"
stellar contract build --optimize

LOCAL_WASM="$(./scripts/find-wasm.sh || true)"

if [ -z "${LOCAL_WASM}" ] || [ ! -f "${LOCAL_WASM}" ]; then
  echo "error: no built tip_splitter.wasm found under target/." >&2
  exit 1
fi

LOCAL_HASH=$(sha256sum "${LOCAL_WASM}" | cut -d' ' -f1)
echo "    Local SHA-256:    ${LOCAL_HASH}"

# ── Compare ─────────────────────────────────────────────────────────────────
if [ "${DEPLOYED_HASH}" = "${LOCAL_HASH}" ]; then
  echo "==> MATCH: deployed wasm matches local build"
  exit 0
fi

echo ""
echo "==> MISMATCH: deployed wasm differs from local build"
echo ""
echo "    Deployed SHA-256: ${DEPLOYED_HASH}"
echo "    Local SHA-256:    ${LOCAL_HASH}"
echo ""

# ── Print metadata for both ─────────────────────────────────────────────────
echo "--- Deployed contract metadata ---"
stellar contract info meta --wasm "${DEPLOYED_WASM}" 2>/dev/null || echo "(unable to read metadata)"
echo ""
echo "--- Local build metadata ---"
stellar contract info meta --wasm "${LOCAL_WASM}" 2>/dev/null || echo "(unable to read metadata)"
echo ""

# ── Compare interfaces ──────────────────────────────────────────────────────
echo "--- Interface comparison ---"
DEPLOYED_IFACE=$(stellar contract info interface --wasm "${DEPLOYED_WASM}" 2>/dev/null || echo "")
LOCAL_IFACE=$(stellar contract info interface --wasm "${LOCAL_WASM}" 2>/dev/null || echo "")

if [ -n "${DEPLOYED_IFACE}" ] && [ -n "${LOCAL_IFACE}" ]; then
  DEPLOYED_FUNCS=$(echo "${DEPLOYED_IFACE}" | grep -oP 'fn \K\w+' | sort)
  LOCAL_FUNCS=$(echo "${LOCAL_IFACE}" | grep -oP 'fn \K\w+' | sort)

  ONLY_LOCAL=$(comm -13 <(echo "${DEPLOYED_FUNCS}") <(echo "${LOCAL_FUNCS}"))
  ONLY_DEPLOYED=$(comm -23 <(echo "${DEPLOYED_FUNCS}") <(echo "${LOCAL_FUNCS}"))

  if [ -n "${ONLY_LOCAL}" ]; then
    echo "  Functions in local but NOT deployed:"
    echo "${ONLY_LOCAL}" | awk '{ print "    + " $0 }'
  fi
  if [ -n "${ONLY_DEPLOYED}" ]; then
    echo "  Functions in deployed but NOT local:"
    echo "${ONLY_DEPLOYED}" | awk '{ print "    - " $0 }'
  fi
  if [ -z "${ONLY_LOCAL}" ] && [ -z "${ONLY_DEPLOYED}" ]; then
    echo "  Interfaces match (same function set)."
  fi
else
  echo "  (unable to compare interfaces)"
fi

echo ""
echo "==> The deployed contract does NOT match the current checkout."
echo "    A toolchain or CLI mismatch is the usual reason two builds of the"
echo "    same source differ. Check the metadata above."
exit 1
