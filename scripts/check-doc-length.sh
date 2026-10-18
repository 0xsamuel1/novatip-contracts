#!/usr/bin/env bash
set -euo pipefail

# Soroban embeds each Rust doc comment in contractspecv0 and truncates
# individual docs at 1024 bytes.
LC_ALL=C awk '
  function check() {
    if (length(block) > 1024) {
      printf "error: documentation block exceeds 1024 bytes: %d\n", length(block) > "/dev/stderr"
      failed = 1
    }
    block = ""
  }
  /^\/\/\// { block = block $0 "\n"; next }
  { if (block != "") check() }
  END { if (block != "") check(); exit failed }
' contracts/tip-splitter/src/lib.rs
