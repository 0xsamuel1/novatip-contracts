#!/usr/bin/env bash
set -euo pipefail

# Replace an existing jar's splits, with the same local validation
# create-jar.sh applies and an old/new preview before anything is sent.
#
# .env (repository root) is loaded automatically; exported variables win over
# it. The script works when started from any directory.
#
# Required env (see .env.example):
#   NETWORK, SOURCE, SLUG
#   CONTRACT_ID  (optional; defaults to .contract-id.<NETWORK>)
#   SPLITS or RECIPIENT  the new splits, same format as create-jar.sh
#
# SOURCE signs the transaction and must be the jar's owner: update_splits
# requires the owner's authorization, and this script refuses to send when
# get_jar reports a different owner.
#
# Usage:  ./scripts/update-splits.sh
#         SPLITS='[{"to":"G...A","bps":7000},{"to":"G...B","bps":3000}]' \
#           ./scripts/update-splits.sh

. "$(dirname "${BASH_SOURCE[0]}")/lib/env.sh"
# env.sh may have cd'ed to the repository root; resolve libs from there.
. "${NOVATIP_ROOT}/scripts/lib/splits.sh"

require_env NETWORK SOURCE SLUG
require_contract_id

resolve_splits || exit 1

invoke() {
  stellar contract invoke \
    --id "${CONTRACT_ID}" \
    --source "${SOURCE}" \
    --network "${NETWORK}" \
    -- "$@"
}

if ! CURRENT=$(invoke get_jar --jar_id "${SLUG}" 2>&1); then
  if grep -q 'Error(Contract, #3)' <<<"${CURRENT}"; then
    echo "error: no jar named ${SLUG} on ${CONTRACT_ID} (${NETWORK}) — nothing to update" >&2
    exit 1
  fi
  echo "error: reading the current jar failed:" >&2
  echo "${CURRENT}" >&2
  exit 1
fi

CURRENT_OWNER=$(jq -r '.owner' <<<"${CURRENT}")
if ! SIGNER_ADDR=$(stellar keys address "${SOURCE}" 2>/dev/null); then
  echo "error: could not resolve the address of identity '${SOURCE}' (via 'stellar keys address ${SOURCE}')." >&2
  echo "       update_splits must be signed by the jar's owner (${CURRENT_OWNER}); refusing to send." >&2
  exit 1
fi

if [ "${SIGNER_ADDR}" != "${CURRENT_OWNER}" ]; then
  echo "error: ${SLUG} is owned by ${CURRENT_OWNER}, but SOURCE '${SOURCE}' is ${SIGNER_ADDR}." >&2
  echo "       update_splits requires the jar owner's signature — set SOURCE to the owner's identity." >&2
  exit 1
fi

echo "==> Jar ${SLUG} (owner ${CURRENT_OWNER})"
echo "    current splits:"
jq -r '.splits[] | "      \(.to)  \(.bps) bps"' <<<"${CURRENT}"
echo "    new splits:"
jq -r '.[] | "      \(.to)  \(.bps) bps"' <<<"${SPLITS_JSON}"

# -S normalises key order within each entry so an equivalent JSON spelling is
# recognised as unchanged. Order between entries is meaningful (the last
# recipient absorbs the rounding dust), so a reorder is a real change.
if [ "$(jq -Sc '.splits' <<<"${CURRENT}")" = "$(jq -Sc '.' <<<"${SPLITS_JSON}")" ]; then
  echo "==> The new splits are identical to the current ones — nothing sent."
  exit 0
fi

echo "==> Updating splits of jar ${SLUG} on ${CONTRACT_ID}"
invoke update_splits --jar_id "${SLUG}" --splits "${SPLITS_JSON}"

echo "==> Done."
