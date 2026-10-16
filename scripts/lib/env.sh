#!/usr/bin/env bash
# Shared bootstrap for the scripts/*.sh entry points.
#
# Every script sources this file first. It:
#   - loads the repository-root .env (if present) so scripts run without a
#     manual `set -a; source .env; set +a`, and
#   - cds to the repository root so relative paths (.contract-id, target/)
#     resolve no matter where the script is started from.
#
# Precedence: a variable already set in the environment always wins over the
# same variable in .env, so `SLUG=@bob ./scripts/create-jar.sh` keeps working.
#
# Usage from a script in scripts/:
#   . "$(dirname "${BASH_SOURCE[0]}")/lib/env.sh"

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

cd "$NOVATIP_ROOT"

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
