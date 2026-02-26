#!/bin/bash
set -euo pipefail

# Setup build matrix
# This script creates a build matrix JSON from TARGETS and BINARIES variables

# If TARGETS is empty or unset, use default.targets.json
if [ -z "${TARGETS:-}" ]; then
  TARGETS=$(cat .github/vars/default.targets.json)
fi

# If BINARIES is empty or unset, use default.binaries.json
if [ -z "${BINARIES:-}" ]; then
  BINARIES=$(cat .github/vars/default.binaries.json)
fi

matrix=$(echo 'null' | jq -c '
  {
    include: ($targets | to_entries | map(.key as $target_name | {
      name: .key,
      rust_target: .value.rust_target,
      runner: .value.runner,
      bins: [$binaries[] | select(.targets | index($target_name)) | .bin],
      cross: (if (.value | has("cross")) then .value.cross else true end)
    }) | map(select((.bins | length) > 0)))
  }
' --argjson targets "$TARGETS" --argjson binaries "$BINARIES")

echo "matrix=$matrix" >> $GITHUB_OUTPUT

echo "Build matrix:"
echo "$matrix" | jq '.'