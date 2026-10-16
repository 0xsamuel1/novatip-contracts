#!/usr/bin/env bash
set -euo pipefail

# Transfer a tip jar to a new owner.
#
# Required env (see .env.example):
#   NETWORK      testnet | mainnet | local
#   SOURCE       Stellar CLI identity used to sign (must be the current owner)
#   SLUG         the jar's public id, e.g. @alice
#   NEW_OWNER    G... address taking control of the jar
#   CONTRACT_ID  optional; defaults to the contents of .contract-id
#
# Usage:  set -a; source .env; set +a; ./scripts/transfer-jar.sh
#         NEW_OWNER=GB7T... ./scripts/transfer-jar.sh
#
# `transfer_jar_ownership` requires the *current* owner to authorize, so SOURCE
# must control the jar. The new owner does not authorize today — the call is a
# unilateral hand-over — but issue #42 proposes requiring their signature too.
# The signing account is therefore passed as a single `--source` flag rather
# than being assumed anywhere else: adding a second signer later means adding
# one more `--source`, not reworking this script.

: "${NETWORK:?set NETWORK}"
: "${SOURCE:?set SOURCE}"
: "${SLUG:?set SLUG}"
: "${NEW_OWNER:?set NEW_OWNER}"
CONTRACT_ID="${CONTRACT_ID:-$(cat .contract-id)}"

# Show who controls the jar before the call, so an operator driving the wrong
# SOURCE, or pointing at the wrong deployed contract, sees it here rather than
# discovering it from an unexpected `jar_xfer` event afterwards. This is a view
# call and signs nothing.
echo "==> Reading current owner of ${SLUG} on ${CONTRACT_ID}"
CURRENT_OWNER=$(stellar contract invoke \
  --id "${CONTRACT_ID}" \
  --source "${SOURCE}" \
  --network "${NETWORK}" \
  -- \
  get_jar_owner \
  --jar_id "${SLUG}")
echo "    current owner: ${CURRENT_OWNER}"
if [ "${CURRENT_OWNER}" != "${SOURCE}" ] && [ "${CURRENT_OWNER}" != "${NEW_OWNER}" ]; then
  echo "    note: SOURCE is not the current owner, so the call will fail authorization" >&2
fi
if [ "${CURRENT_OWNER}" = "${NEW_OWNER}" ]; then
  echo "    note: ${NEW_OWNER} already owns this jar; the call is a no-op reassignment" >&2
fi

echo "==> Transferring ${SLUG} to ${NEW_OWNER}"
stellar contract invoke \
  --id "${CONTRACT_ID}" \
  --source "${SOURCE}" \
  --network "${NETWORK}" \
  -- \
  transfer_jar_ownership \
  --jar_id "${SLUG}" \
  --new_owner "${NEW_OWNER}"

echo "==> Done. ${SLUG} is now owned by ${NEW_OWNER} (splits unchanged)."
