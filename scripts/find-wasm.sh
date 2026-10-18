#!/usr/bin/env bash
set -euo pipefail

# Print the newest built tip_splitter wasm, or exit 1 when none exists.
#
# Portable replacement for `find -printf` (GNU-only) so the Makefile, deploy
# and verify scripts behave the same on macOS/BSD and Linux (#120). The logic
# lives here so those callers cannot drift.

_root() {
  local src dir
  src="${BASH_SOURCE[0]}"
  while [ -L "$src" ]; do
    dir="$(cd -P "$(dirname "$src")" && pwd)"
    src="$(readlink "$src")"
    [[ "$src" == /* ]] || src="$dir/$src"
  done
  cd -P "$(dirname "$src")/.." && pwd
}

ROOT="$(_root)"
BEST=""

for candidate in \
  target/wasm32v1-none/release/tip_splitter.optimized.wasm \
  target/wasm32v1-none/release/tip_splitter.wasm \
  target/wasm32-unknown-unknown/release/tip_splitter.optimized.wasm \
  target/wasm32-unknown-unknown/release/tip_splitter.wasm
do
  path="${ROOT}/${candidate}"
  if [ -f "${path}" ]; then
    if [ -z "${BEST}" ] || [ "${path}" -nt "${BEST}" ]; then
      BEST="${path}"
    fi
  fi
done

if [ -z "${BEST}" ]; then
  exit 1
fi
printf '%s\n' "${BEST#"${ROOT}/"}"
