#!/bin/bash
set -euo pipefail

# Test script for docker_setup-build-matrix.sh
# Uses the shared github_wrapper.sh to create a temporary GITHUB_OUTPUT file,
# execute the script, and print the output
# Prompts user to test with or without DOCKER_IMAGES override

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORIGINAL_SCRIPT="$REPO_ROOT/.github/scripts/docker_setup-build-matrix.sh"
WRAPPER_SCRIPT="$SCRIPT_DIR/github_wrapper.sh"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🐳 Docker Setup Build Matrix - Local Runner"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Ask user for input
echo "Please provide the following information:"
echo ""

# SET_DOCKER_IMAGES
read -p "Should DOCKER_IMAGES variable be set for testing override? (true/false) [default: false]: " SET_DOCKER_IMAGES
SET_DOCKER_IMAGES=${SET_DOCKER_IMAGES:-false}

# Convert to lowercase for comparison
SET_DOCKER_IMAGES=$(echo "$SET_DOCKER_IMAGES" | tr '[:upper:]' '[:lower:]')

if [[ ! "$SET_DOCKER_IMAGES" =~ ^(true|false|t|f|yes|no|y|n)$ ]]; then
    echo "Error: SET_DOCKER_IMAGES must be 'true' or 'false'"
    exit 1
fi

# Normalize to true/false
if [ "$SET_DOCKER_IMAGES" = "true" ] || [ "$SET_DOCKER_IMAGES" = "t" ] || [ "$SET_DOCKER_IMAGES" = "yes" ] || [ "$SET_DOCKER_IMAGES" = "y" ]; then
    SET_DOCKER_IMAGES="true"
    # Set a test DOCKER_IMAGES value that will override the default
    # This will use a subset of the default images for testing
    export DOCKER_IMAGES='[
        {
            "bin": "cli",
            "bake-file": "cli/bake.hcl",
            "platforms": [
                "linux/amd64"
            ]
        },
        {
            "bin": "operator",
            "bake-file": "operator/bake.hcl",
            "platforms": [
                "linux/amd64",
                "linux/arm64"
            ]
        }
    ]'
else
    SET_DOCKER_IMAGES="false"
    # Unset DOCKER_IMAGES to test default-only behavior
    unset DOCKER_IMAGES
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📋 Configuration:"
echo "  SET_DOCKER_IMAGES: $SET_DOCKER_IMAGES"
if [ "$SET_DOCKER_IMAGES" = "true" ]; then
    echo ""
    echo "  DOCKER_IMAGES is set to:"
    echo "$DOCKER_IMAGES" | jq '.'
else
    echo "  DOCKER_IMAGES: (not set - using default from .github/vars/default.docker.json)"
fi
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Change to repository root to run the script
cd "$REPO_ROOT"

# Execute using the shared wrapper
exec "$WRAPPER_SCRIPT" "$ORIGINAL_SCRIPT"

