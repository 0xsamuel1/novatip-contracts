#!/usr/bin/env bash
# Shared SPLITS/RECIPIENT handling for create-jar.sh and update-splits.sh.
#
# One copy of the validation so improving it updates both scripts. The rules,
# messages and the RECIPIENT shorthand behave exactly as create-jar.sh always
# has.
#
# resolve_splits reads SPLITS (full JSON) or RECIPIENT (single-recipient
# shorthand) and sets:
#   SPLITS_JSON   the validated splits array to send to the contract
#   SPLIT_COUNT   number of recipients
# It returns non-zero (with a message on stderr) when the splits are invalid.

resolve_splits() {
  if [ -n "${SPLITS:-}" ]; then
    command -v jq >/dev/null 2>&1 || {
      echo "error: SPLITS requires jq to be installed" >&2
      return 1
    }
    echo "${SPLITS}" | jq -e 'type == "array"' >/dev/null 2>&1 || {
      echo "error: SPLITS must be a JSON array" >&2
      return 1
    }

    SPLIT_COUNT=$(echo "${SPLITS}" | jq 'length')
    if [ "${SPLIT_COUNT}" -eq 0 ] || [ "${SPLIT_COUNT}" -gt 20 ]; then
      echo "error: splits must have between 1 and 20 recipients (got ${SPLIT_COUNT})" >&2
      return 1
    fi

    TOTAL_BPS=$(echo "${SPLITS}" | jq '[.[].bps] | add')
    if [ "${TOTAL_BPS}" != "10000" ]; then
      echo "error: bps values must sum to 10000 (got ${TOTAL_BPS})" >&2
      return 1
    fi

    SPLITS_JSON="${SPLITS}"
  else
    : "${RECIPIENT:?set RECIPIENT or SPLITS}"
    SPLIT_COUNT=1
    SPLITS_JSON="[{\"to\":\"${RECIPIENT}\",\"bps\":10000}]"
  fi
}
