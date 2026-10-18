#!/usr/bin/env bash
set -euo pipefail

# Set up the USDC trustline every classic G... account needs before it can
# send or receive a tip.
#
# The contract settles in the USDC Stellar Asset Contract. A tipper with no
# trustline fails `tip` with the token's Error(Contract, #13), and so does a
# jar where any single recipient has none — tips are all-or-nothing. This
# script adds the missing trustlines, or just reports who is missing them.
#
# The classic asset is derived from USDC_TOKEN itself: the SAC's `name`
# function returns "USDC:<issuer>", so the change-trust line never has to be
# pasted by hand and cannot disagree with the deployed contract.
#
# Required env:
#   NETWORK      testnet | mainnet | local
#   USDC_TOKEN   USDC Stellar Asset Contract id (C...) — see .env.example
#   IDENTITIES   space-separated CLI identity names or G... addresses, e.g.
#                "tipper creator1 creator2" (identity names are resolved with
#                `stellar keys address`).
#
# Optional env:
#   SOURCE       identity used for the read-only calls; default: first entry
#                of IDENTITIES.
#   CONTRACT_ID  optional; defaults to .contract-id.<NETWORK> (--check only)
#   SLUG         jar id, e.g. "@demo" — required with --check
#   HORIZON_URL  override the Horizon endpoint used to read account balances
#                (default per NETWORK: horizon-testnet / horizon / localhost:8000)
#
# Usage:
#   set -a; source .env; set +a
#   ./scripts/trustline.sh                     # add the line for IDENTITIES
#   ./scripts/trustline.sh --check             # same list, report only
#   SLUG=@demo ./scripts/trustline.sh --check  # every recipient of the jar,
#                                              # exit non-zero if any is missing
#
# Where testnet USDC comes from, once the trustline exists:
#   Testnet USDC is issued by the account encoded in the asset name above —
#   USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5. There is
#   no on-chain claim: fund an account by receiving a payment from a holder.
#   `stellar fund --source <identity> --network testnet` covers the XLM side
#   (base reserve and minimum trust-line reserve), then either use Circle's
#   testnet faucet (https://faucet.circle.com) if it currently covers Stellar,
#   or send from any account that already holds testnet USDC:
#
#     stellar tx new payment --source funded-identity --destination G... \
#       --asset native --network testnet --amount 1            # XLM via friendbot first
#     stellar tx new payment --source holder --destination G... \
#       --asset "USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5" \
#       --network testnet --amount 100                          # USDC from a holder

. "$(dirname "${BASH_SOURCE[0]}")/lib/env.sh"

: "${NETWORK:?set NETWORK (testnet | mainnet | local)}"
: "${USDC_TOKEN:?set USDC_TOKEN (the USDC Stellar Asset Contract id, C...)}"
: "${IDENTITIES:?set IDENTITIES to one or more identity names or G... addresses}"

MODE="apply"
if [ "${1:-}" = "--check" ]; then
  MODE="check"
  shift
fi
if [ "$#" -gt 0 ]; then
  echo "error: unknown argument '$1' (only --check is accepted)" >&2
  exit 1
fi
if [ "${MODE}" = "check" ] && [ -n "${SLUG:-}" ]; then
  require_contract_id
fi

command -v stellar >/dev/null 2>&1 || { echo "error: the stellar CLI is required" >&2; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "error: jq is required" >&2; exit 1; }
command -v curl >/dev/null 2>&1 || { echo "error: curl is required to read account trustlines" >&2; exit 1; }

case "${NETWORK}" in
  testnet)  HORIZON_URL="${HORIZON_URL:-https://horizon-testnet.stellar.org}" ;;
  mainnet)  HORIZON_URL="${HORIZON_URL:-https://horizon.stellar.org}" ;;
  local)    HORIZON_URL="${HORIZON_URL:-http://localhost:8000}" ;;
  *)        HORIZON_URL="${HORIZON_URL:?set HORIZON_URL for unknown NETWORK ${NETWORK}}" ;;
esac

# Resolve an identity name to a G... address; pass through anything that
# already looks like a classic address.
resolve_address() {
  local id="$1"
  case "${id}" in
    G*) printf '%s' "${id}" ;;
    *) stellar keys address "${id}" ;;
  esac
}

SRC="${SOURCE:-${IDENTITIES%% *}}"

echo "==> Deriving the classic asset from ${USDC_TOKEN} on ${NETWORK}"
# The SAC's name function returns "USDC:<issuer>" — the exact --line string
# change-trust expects, so nothing here hardcodes the issuer.
ASSET_LINE=$(stellar contract invoke \
  --id "${USDC_TOKEN}" \
  --source "${SRC}" \
  --network "${NETWORK}" \
  --output json \
  -- name | jq -r . | tr -d '"')
ASSET_CODE="${ASSET_LINE%%:*}"
ASSET_ISSUER="${ASSET_LINE#*:}"
case "${ASSET_ISSUER}" in
  G*) ;;
  *) echo "error: unexpected SAC name '${ASSET_LINE}' (expected CODE:G...)" >&2; exit 1 ;;
esac
echo "    asset line: ${ASSET_LINE}"

# True when the classic account already has a trustline to ASSET_LINE. A
# missing account (404) has none by definition; it still needs funding with
# XLM before change-trust will go through.
has_trustline() {
  local addr="$1" body
  body=$(curl -fsS "${HORIZON_URL}/accounts/${addr}" 2>/dev/null) || return 1
  printf '%s' "${body}" | jq -e --arg c "${ASSET_CODE}" --arg i "${ASSET_ISSUER}" \
    '[.balances[] | select((.asset.code // .asset_code) == $c and (.asset.issuer // .asset_issuer) == $i)] | length > 0' \
    >/dev/null 2>&1
}

# Recipient addresses of the jar named by SLUG, one per line.
jar_recipients() {
  stellar contract invoke \
    --id "${CONTRACT_ID}" \
    --source "${SRC}" \
    --network "${NETWORK}" \
    --output json \
    -- get_jar --jar_id "${SLUG}" | jq -r '.splits[].to'
}

MISSING=()
add_or_check() {
  local id="$1" addr
  addr=$(resolve_address "${id}")
  if has_trustline "${addr}"; then
    echo "    ${addr}  already has ${ASSET_LINE} — skipped"
    return 0
  fi
  MISSING+=("${addr}")
  [ "${MODE}" = "check" ] && { echo "    ${addr}  MISSING ${ASSET_LINE}"; return 0; }
  echo "    ${addr}  adding trustline (source: ${id})"
  # change-trust must be signed by the account itself. When a jar recipient is
  # listed here but its key is not in this CLI, the tx fails — that is not a
  # reason to abandon the other accounts, so report who has to run the script
  # themselves instead of exiting.
  if ! stellar tx new change-trust \
    --source "${id}" \
    --line "${ASSET_LINE}" \
    --network "${NETWORK}"; then
    echo "    ${addr}  could not be signed from '${id}'; its owner must run:" >&2
    echo "      IDENTITIES=${addr} ./scripts/trustline.sh" >&2
  fi
}

echo "==> ${MODE}: ${ASSET_CODE} trustline for ${IDENTITIES}"
for id in ${IDENTITIES}; do
  add_or_check "${id}"
done

if [ -n "${SLUG:-}" ]; then
  echo "==> Checking recipients of ${SLUG} on ${CONTRACT_ID}"
  while IFS= read -r addr; do
    add_or_check "${addr}"
  done < <(jar_recipients)
fi

if [ "${#MISSING[@]}" -gt 0 ]; then
  if [ "${MODE}" = "check" ]; then
    echo "error: ${#MISSING[@]} account(s) cannot receive a tip until a USDC trustline exists." >&2
    exit 1
  fi
  echo "==> Done. ${#MISSING[@]} trustline(s) added."
else
  echo "==> Done. Nothing to do — every account already had the trustline."
fi
