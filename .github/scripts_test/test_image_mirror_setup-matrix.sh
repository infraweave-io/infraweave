#!/bin/bash
set -euo pipefail

# Test script for image_mirror_setup-matrix.sh
# Uses the shared github_wrapper.sh to create a temporary GITHUB_OUTPUT file,
# execute the script, and print the output
# Prompts user to test with or without IMAGE_MIRROR override

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ORIGINAL_SCRIPT="$REPO_ROOT/.github/scripts/image_mirror_setup-matrix.sh"
WRAPPER_SCRIPT="$SCRIPT_DIR/github_wrapper.sh"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🪞 Image Mirror Setup Matrix - Local Runner"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Ask user for input
echo "Please provide the following information:"
echo ""

# SET_IMAGE_MIRROR
read -p "Should IMAGE_MIRROR variable be set for testing override? (true/false) [default: false]: " SET_IMAGE_MIRROR
SET_IMAGE_MIRROR=${SET_IMAGE_MIRROR:-false}

# Convert to lowercase for comparison
SET_IMAGE_MIRROR=$(echo "$SET_IMAGE_MIRROR" | tr '[:upper:]' '[:lower:]')

if [[ ! "$SET_IMAGE_MIRROR" =~ ^(true|false|t|f|yes|no|y|n)$ ]]; then
    echo "Error: SET_IMAGE_MIRROR must be 'true' or 'false'"
    exit 1
fi

# Normalize to true/false
if [ "$SET_IMAGE_MIRROR" = "true" ] || [ "$SET_IMAGE_MIRROR" = "t" ] || [ "$SET_IMAGE_MIRROR" = "yes" ] || [ "$SET_IMAGE_MIRROR" = "y" ]; then
    SET_IMAGE_MIRROR="true"
    # Set a test IMAGE_MIRROR value that will override one entry and add a new one
    # This will override the minio entry and add a new test entry
    export IMAGE_MIRROR='[
        {
            "from": "minio/minio:latest",
            "to": "minio:overridden"
        },
        {
            "from": "test/image:latest",
            "to": "test-image:latest"
        }
    ]'
else
    SET_IMAGE_MIRROR="false"
    # Unset IMAGE_MIRROR to test default-only behavior
    unset IMAGE_MIRROR
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📋 Configuration:"
echo "  SET_IMAGE_MIRROR: $SET_IMAGE_MIRROR"
if [ "$SET_IMAGE_MIRROR" = "true" ]; then
    echo ""
    echo "  IMAGE_MIRROR is set to:"
    echo "$IMAGE_MIRROR" | jq '.'
else
    echo "  IMAGE_MIRROR: (not set - using default from .github/vars/default.image_mirror.json)"
fi
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Change to repository root to run the script
cd "$REPO_ROOT"

# Execute using the shared wrapper
exec "$WRAPPER_SCRIPT" "$ORIGINAL_SCRIPT"

