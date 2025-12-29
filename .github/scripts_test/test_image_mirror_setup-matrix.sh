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

# Ask if IMAGE_MIRROR should be set
echo "Should IMAGE_MIRROR variable be set for testing override? (true/false)"
read -r SET_IMAGE_MIRROR

# Convert to lowercase for comparison
SET_IMAGE_MIRROR=$(echo "$SET_IMAGE_MIRROR" | tr '[:upper:]' '[:lower:]')

if [ "$SET_IMAGE_MIRROR" = "true" ] || [ "$SET_IMAGE_MIRROR" = "t" ] || [ "$SET_IMAGE_MIRROR" = "yes" ] || [ "$SET_IMAGE_MIRROR" = "y" ]; then
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
    echo ""
    echo "IMAGE_MIRROR is set to:"
    echo "$IMAGE_MIRROR" | jq '.'
    echo ""
else
    # Unset IMAGE_MIRROR to test default-only behavior
    unset IMAGE_MIRROR
    echo ""
    echo "IMAGE_MIRROR is not set - testing default-only behavior"
    echo ""
fi

# Execute using the shared wrapper
exec "$WRAPPER_SCRIPT" "$ORIGINAL_SCRIPT"

