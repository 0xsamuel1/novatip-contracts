#!/usr/bin/env bash
# Shared bootstrap for the scripts/*.sh entry points.
#
# Every script sources this file first. It:
#   - loads the repository-root .env (if present) so scripts run without a
#     manual `set -a; source .env; set +a`, and
#   - cds to the repository root so relative paths (.contract-id.<network>,
#     target/) resolve no matter where the script is started from.
#
# Precedence: a variable already set in the environment always wins over the
# same variable in .env, so `SLUG=@bob ./scripts/create-jar.sh` keeps working.
#
# Usage from a script in scripts/:
#   . "$(dirname "${BASH_SOURCE[0]}")/lib/env.sh"

# shellcheck disable=SC2317 # sourced callers return from this guard
if [ -n "${_NOVATIP_ENV_SH:-}" ]; then
  return 0 2>/dev/null || true
fi
_NOVATIP_ENV_SH=1

_novatip_env_root() {
  local src dir
  src="${BASH_SOURCE[0]}"
  while [ -L "$src" ]; do
    dir="$(cd -P "$(dirname "$src")" && pwd)"
    src="$(readlink "$src")"
    [[ "$src" == /* ]] || src="$dir/$src"
  done
  cd -P "$(dirname "$src")/../.." && pwd
}

NOVATIP_ROOT="$(_novatip_env_root)"

if [ -f "$NOVATIP_ROOT/.env" ]; then
  while IFS= read -r _line || [ -n "$_line" ]; do
    case "$_line" in
      '' | '#'*) continue ;;
    esac
    _key="${_line%%=*}"
    _val="${_line#*=}"
    [[ "$_key" =~ ^[[:space:]]*[A-Za-z_][A-Za-z0-9_]*[[:space:]]*$ ]] || continue
    _key="${_key//[[:space:]]/}"
    # Strip one layer of surrounding quotes, as `source` would.
    case "$_val" in
      \"*\") _val="${_val#\"}"; _val="${_val%\"}" ;;
      \'*\') _val="${_val#\'}"; _val="${_val%\'}" ;;
    esac
    [ -z "${!_key+set}" ] && export "$_key=$_val"
  done < "$NOVATIP_ROOT/.env"
fi

cd "$NOVATIP_ROOT" || exit 1

# Fail with a readable message instead of a bare parameter-expansion error
# when a required setting is missing from both the environment and .env.
require_env() {
  local v
  for v in "$@"; do
    if [ -z "${!v:-}" ]; then
      echo "error: $v is required but not set — export it or fill it in .env (see .env.example)" >&2
      exit 1
    fi
  done
}


# Path of the deployed contract id for the current NETWORK (#122).
contract_id_file() {
  printf '.contract-id.%s' "${NETWORK:?NETWORK must be set}"
}

# Resolve CONTRACT_ID for the current NETWORK when the caller did not set it.
# Fails with a message naming the network and the expected file, instead of a
# raw `cat: .contract-id: No such file or directory` from `set -e`.
require_contract_id() {
  if [ -n "${CONTRACT_ID:-}" ]; then
    export CONTRACT_ID
    return 0
  fi

  local file id
  file="$(contract_id_file)"
  if [ ! -f "${file}" ]; then
    echo "error: no contract id for NETWORK=${NETWORK} (expected ${file})." >&2
    echo "       Run ./scripts/deploy.sh with NETWORK=${NETWORK} first, or set CONTRACT_ID." >&2
    exit 1
  fi

  id="$(tr -d '[:space:]' < "${file}")"
  if [ -z "${id}" ]; then
    echo "error: ${file} is empty; redeploy with NETWORK=${NETWORK} or set CONTRACT_ID." >&2
    exit 1
  fi
  CONTRACT_ID="${id}"
  export CONTRACT_ID
}
