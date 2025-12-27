#!/bin/bash
set -euo pipefail

# Shared wrapper script for GitHub Actions scripts
# Creates a temporary GITHUB_OUTPUT file, executes the script, and prints the output
#
# Usage: github_wrapper.sh <script_path> [script_args...]

if [ $# -lt 1 ]; then
    echo "Usage: $0 <script_path> [script_args...]" >&2
    exit 1
fi

SCRIPT_TO_RUN="$1"
shift  # Remove first argument, rest are passed to the script

# Create temporary file for GITHUB_OUTPUT
GITHUB_OUTPUT=$(mktemp)
export GITHUB_OUTPUT

# Cleanup function
cleanup() {
    rm -f "$GITHUB_OUTPUT"
}
trap cleanup EXIT

# Execute the original script
echo "Executing: $SCRIPT_TO_RUN"
if bash "$SCRIPT_TO_RUN" "$@"; then
    EXIT_CODE=0
else
    EXIT_CODE=$?
fi

# Print the content of GITHUB_OUTPUT (if any)
echo ""
echo "=== GITHUB_OUTPUT content ==="
if [ -s "$GITHUB_OUTPUT" ]; then
    cat "$GITHUB_OUTPUT"
else
    echo "(empty or not set)"
fi
echo "============================="
echo ""
echo "Script exit code: $EXIT_CODE"

# Exit with the same code as the original script
exit $EXIT_CODE
