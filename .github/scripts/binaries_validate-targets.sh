#!/bin/bash
set -euo pipefail

# Validate targets
# This script validates that all targets referenced in BINARIES exist in TARGETS

# If TARGETS is empty or unset, use default.targets.json
if [ -z "${TARGETS:-}" ]; then
  TARGETS=$(cat .github/vars/default.targets.json)
fi

# If BINARIES is empty or unset, use default.binaries.json
if [ -z "${BINARIES:-}" ]; then
  BINARIES=$(cat .github/vars/default.binaries.json)
fi

missing=$(jq -r --argjson targets "$TARGETS" '
  .[] | .targets[] | select(. as $t | $targets | has($t) | not)
' <<< "$BINARIES" | sort -u)

if [ -n "$missing" ]; then
  missing_list=$(echo "$missing" | tr '\n' ',' | sed 's/,$//')
  echo "::error::Missing BINARY_TARGETS: $missing_list"
  exit 1
fi