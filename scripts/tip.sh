#!/usr/bin/env bash
set -euo pipefail

# Send a tip to a jar on a deployed tip_splitter contract.
#
# .env (repository root) is loaded automatically; exported variables win over
# it. The script works when started from any directory.
#
# Required env (see .env.example):
#   NETWORK, SOURCE, JAR_ID, AMOUNT
#   CONTRACT_ID  (optional; defaults to the contents of .contract-id)
#   RECIPIENT    (optional; defaults to jar owner for single-recipient jars)
#
# Usage:  ./scripts/tip.sh
#         JAR_ID=@bob AMOUNT=100 ./scripts/tip.sh

. "$(dirname "${BASH_SOURCE[0]}")/lib/env.sh"

require_env NETWORK SOURCE JAR_ID AMOUNT
CONTRACT_ID="${CONTRACT_ID:-$(cat .contract-id)}"

echo "==> Sending tip of ${AMOUNT} to jar ${JAR_ID} on ${CONTRACT_ID}"
stellar contract invoke \
  --id "${CONTRACT_ID}" \
  --source "${SOURCE}" \
  --network "${NETWORK}" \
  -- \
  tip \
  --jar_id "${JAR_ID}" \
  --amount "${AMOUNT}"

echo "==> Tip sent."
