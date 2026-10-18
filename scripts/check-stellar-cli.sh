#!/usr/bin/env bash
set -euo pipefail

# Fail early, with a readable message, when the installed Stellar CLI is older
# than the version the build/deploy scripts require (#121).
#
# Usage: ./scripts/check-stellar-cli.sh

. "$(dirname "${BASH_SOURCE[0]}")/lib/env.sh"
. "${NOVATIP_ROOT}/scripts/lib/cli.sh"

require_stellar_cli
