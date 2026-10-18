#!/usr/bin/env bash
set -euo pipefail

# Check that docs/CONTRACT.md's function and error tables match the built
# contract.
#
# Builds the contract, extracts every public function and every error variant
# (with its number) from the wasm via `stellar contract info interface`, and
# fails with a named item when:
#   - a public function is missing from the Functions table, or the table
#     lists a function the contract does not expose;
#   - an error variant is missing from the Errors table, listed with a
#     different number, or listed although the contract does not define it.
#
# Requires the Stellar CLI (the version pinned in .github/workflows/ci.yml,
# also used by `make build`) and jq. Set STELLAR_BIN to override the binary.
#
# Usage:  ./scripts/check-docs.sh   (or: make check-docs)

. "$(dirname "${BASH_SOURCE[0]}")/lib/env.sh"

: "${STELLAR_BIN:=stellar}"
command -v "${STELLAR_BIN}" >/dev/null 2>&1 || {
  echo "error: '${STELLAR_BIN}' not found — install the pinned Stellar CLI (see .github/workflows/ci.yml) or set STELLAR_BIN." >&2
  exit 1
}
command -v jq >/dev/null 2>&1 || {
  echo "error: jq is required by check-docs.sh" >&2
  exit 1
}

DOC="docs/CONTRACT.md"
[ -f "${DOC}" ] || { echo "error: ${DOC} not found" >&2; exit 1; }

echo "==> Building the contract"
if ! "${STELLAR_BIN}" contract build; then
  echo "error: the contract build failed. If it is a missing target, run:" >&2
  echo "       rustup target add wasm32-unknown-unknown" >&2
  exit 1
fi

# Same artifact lookup as deploy.sh/Makefile (#120).
WASM="$(./scripts/find-wasm.sh || true)"
if [ -z "${WASM}" ] || [ ! -f "${WASM}" ]; then
  echo "error: no built tip_splitter.wasm found under target/." >&2
  exit 1
fi

echo "==> Reading the public interface of ${WASM}"
SPEC="$("${STELLAR_BIN}" contract info interface --wasm "${WASM}" --output json-formatted)"
echo "${SPEC}" | jq -e . >/dev/null 2>&1 || {
  echo "error: 'stellar contract info interface --output json-formatted' did not return JSON" >&2
  exit 1
}

# --- everything the built contract exposes -------------------------------

# v22 prints a top-level array of spec entries (function_v0, udt_*); newer
# CLIs nest them under .exports/.function. Handle both shapes.
SPEC_FUNCS="$(jq -r '
  (if type == "object" and has("exports") then .exports else . end)[]?
    | ((.function_v0.name? // .function.name? // .constructor.name?) // empty)
    | select(type == "string")' <<<"${SPEC}" | sort -u)"

# Error variants with their numbers, from the Error enum in the type table.
SPEC_ERRORS="$(jq -r '
  (if type == "object" and has("exports") then .exports else . end)[]?
    | ((.udt_error_enum_v0 // .udt.enum? // empty)
       | select((.name? // "") == "Error")
       | (.cases // .discriminants // [])[]?
       | "\(.value) \(.name)")' <<<"${SPEC}" | sort -u -n)"

if [ -z "${SPEC_FUNCS}" ]; then
  echo "error: could not extract any functions from the built interface; is the CLI version pinned in ci.yml installed?" >&2
  exit 1
fi
if [ -z "${SPEC_ERRORS}" ]; then
  echo "error: could not extract the Error enum from the built interface; is the CLI version pinned in ci.yml installed?" >&2
  exit 1
fi

# --- everything docs/CONTRACT.md claims ----------------------------------

DOC_FUNCS="$(awk '
  /^## Functions$/ { f = 1; next }
  f && /^## /      { f = 0 }
  f && /^\| `/ {
    name = $0
    sub(/^\| `/, "", name)
    sub(/[ (].*$/, "", name)
    print name
  }' "${DOC}" | sort -u)"

DOC_ERRORS="$(awk '
  /^## Errors$/ { f = 1; next }
  f && /^## /   { f = 0 }
  f && /^\| [0-9]+ \| `/ {
    row = $0
    code = row; sub(/^\| /, "", code); sub(/ .*/, "", code)
    name = row; sub(/^\| [0-9]+ \| `/, "", name); sub(/[` ].*/, "", name)
    print code " " name
  }' "${DOC}" | sort -n)"

# --- compare ---------------------------------------------------------------

FAIL=0

while read -r fn; do
  [ -n "${fn}" ] || continue
  grep -qxF "${fn}" <<<"${DOC_FUNCS}" || {
    echo "FAIL: function '${fn}' is exposed by the contract but missing from the Functions table in ${DOC}"
    FAIL=1
  }
done <<<"${SPEC_FUNCS}"

while read -r fn; do
  [ -n "${fn}" ] || continue
  grep -qxF "${fn}" <<<"${SPEC_FUNCS}" || {
    echo "FAIL: the Functions table in ${DOC} lists '${fn}', which the built contract does not expose"
    FAIL=1
  }
done <<<"${DOC_FUNCS}"

while read -r code name; do
  [ -n "${code}" ] || continue
  if grep -qxF "${code} ${name}" <<<"${DOC_ERRORS}"; then
    continue
  fi
  documented=$(awk -v n="${name}" '$2 == n { print $1 }' <<<"${DOC_ERRORS}")
  if [ -n "${documented}" ]; then
    echo "FAIL: error '${name}' is code ${code} in the built contract but documented as ${documented} in ${DOC}"
  else
    echo "FAIL: error variant '${name}' (code ${code}) is missing from the Errors table in ${DOC}"
  fi
  FAIL=1
done <<<"${SPEC_ERRORS}"

while read -r code name; do
  [ -n "${code}" ] || continue
  grep -qxF "${code} ${name}" <<<"${SPEC_ERRORS}" || {
    echo "FAIL: the Errors table in ${DOC} lists '${name}' (code ${code}), which the built contract does not define"
    FAIL=1
  }
done <<<"${DOC_ERRORS}"

if [ "${FAIL}" -ne 0 ]; then
  echo "check-docs: ${DOC} disagrees with the built contract — fix the tables above." >&2
  exit 1
fi

echo "check-docs: OK — $(echo "${SPEC_FUNCS}" | wc -l) functions and $(echo "${SPEC_ERRORS}" | wc -l | tr -d ' ') error variants match ${DOC}."
