#!/usr/bin/env bash
# Shared Stellar CLI version gate (#121).
#
# Sourced by scripts/deploy.sh, scripts/check-stellar-cli.sh and
# scripts/verify-deployment.sh. The minimum is the version whose
# `stellar contract build` targets wasm32v1-none and optimizes the artifact by
# default; older CLIs produce a different path and/or an unoptimized wasm.

MIN_STELLAR_CLI_VERSION="27.1.0"

# version_ge A B -> true when A >= B (dotted numeric versions).
version_ge() {
  local -a a b
  local i x y
  IFS=. read -r -a a <<< "$1"
  IFS=. read -r -a b <<< "$2"
  for i in 0 1 2; do
    x="${a[$i]:-0}"
    y="${b[$i]:-0}"
    if (( 10#$x > 10#$y )); then return 0; fi
    if (( 10#$x < 10#$y )); then return 1; fi
  done
  return 0
}

require_stellar_cli() {
  if ! command -v stellar >/dev/null 2>&1; then
    echo "error: the Stellar CLI is required (minimum ${MIN_STELLAR_CLI_VERSION})." >&2
    echo "       Install it from https://developers.stellar.org/docs/tools/cli" >&2
    exit 1
  fi

  local version
  version="$(stellar version 2>/dev/null | awk 'NR==1 { print $2 }')"
  if [ -z "${version}" ]; then
    echo "error: could not determine the Stellar CLI version ('stellar version' failed)." >&2
    exit 1
  fi
  if ! [[ "${version}" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; then
    echo "error: unrecognised Stellar CLI version '${version}'." >&2
    exit 1
  fi
  if ! version_ge "${version}" "${MIN_STELLAR_CLI_VERSION}"; then
    echo "error: Stellar CLI ${version} is too old; ${MIN_STELLAR_CLI_VERSION} or newer is required." >&2
    echo "       Upgrade with: cargo install stellar-cli --version ${MIN_STELLAR_CLI_VERSION} --locked --force" >&2
    exit 1
  fi
}
