#!/bin/bash
set -euo pipefail

# Setup image mirror matrix
# This script creates a matrix JSON by merging default.image_mirror.json with IMAGE_MIRROR env variable
# IMAGE_MIRROR entries overwrite default entries based on matching "from" field

# Always read the default file
default=$(cat .github/vars/default.image_mirror.json)

# If IMAGE_MIRROR is set, merge it with default (IMAGE_MIRROR overwrites matching entries)
if [ -z "${IMAGE_MIRROR:-}" ] || [ "$IMAGE_MIRROR" = "" ]; then
  # No IMAGE_MIRROR provided, use default only
  matrix=$(echo "$default" | jq -c '.')
else
  # Merge IMAGE_MIRROR into default, with IMAGE_MIRROR entries overwriting default entries
  # Entries are matched by the "from" field
  matrix=$(echo "$default" | jq -c --argjson override "$IMAGE_MIRROR" '
    . as $default |
    $override as $override |
    ($default | map(.from)) as $default_froms |
    ($override | map(.from)) as $override_froms |
    ($default | map(select(.from as $f | ($override_froms | index($f)) == null))) + $override
  ')
fi

echo "matrix=$matrix" >> $GITHUB_OUTPUT

echo "Image mirror matrix:"
echo "$matrix" | jq '.'

