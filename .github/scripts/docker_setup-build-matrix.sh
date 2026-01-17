#!/bin/bash
set -euo pipefail

# Setup build matrix for Docker images
# This script creates a build matrix JSON from DOCKER_IMAGES variable

# If DOCKER_IMAGES is empty or unset, use default.docker.json
if [ -z "${DOCKER_IMAGES:-}" ]; then
  DOCKER_IMAGES=$(cat .github/vars/default.docker.json)
fi

# Validate it's valid JSON and output
echo "$DOCKER_IMAGES" | jq '.' > /dev/null

echo "matrix=$(echo "$DOCKER_IMAGES" | jq -c '.')" >> $GITHUB_OUTPUT

echo "Docker build matrix:"
echo "$DOCKER_IMAGES" | jq '.'

